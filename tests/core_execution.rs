//! Lifecycle checks use controlled pending futures, plus the real native host.
use cesium_rs::core::image_helpers::*;
use cesium_rs::core::ktx2_transcoder::*;
use cesium_rs::core::*;
use serde_json::{json, Value};
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    future::Future,
    pin::Pin,
    rc::Rc,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc,
    },
    task::{Context, Poll, Wake, Waker},
};
#[cfg(feature = "native-host")]
use std::{
    pin::pin,
    time::{Duration, Instant},
};
fn poll<F: Future + Unpin>(f: &mut F, w: &Waker) -> Poll<F::Output> {
    Pin::new(f).poll(&mut Context::from_waker(w))
}
#[derive(Default)]
struct Worker {
    tasks: RefCell<HashMap<u64, futures_channel::oneshot::Sender<Result<u32, RuntimeError>>>>,
    cancelled: RefCell<Vec<u64>>,
    terminated: Cell<usize>,
    init: RefCell<Option<futures_channel::oneshot::Receiver<Result<Value, RuntimeError>>>>,
    init_count: Cell<usize>,
}
impl TaskWorker<u32, u32> for Worker {
    fn run(&self, id: u64, _: u32) -> TaskFuture<u32> {
        let (tx, rx) = futures_channel::oneshot::channel();
        self.tasks.borrow_mut().insert(id, tx);
        Box::pin(async { rx.await.unwrap() })
    }
    fn init_web_assembly_module(&self, _: WebAssemblyOptions) -> TaskFuture<Value> {
        self.init_count.set(self.init_count.get() + 1);
        let rx = self.init.borrow_mut().take().unwrap();
        Box::pin(async { rx.await.unwrap() })
    }
    fn cancel_task(&self, id: u64) {
        self.cancelled.borrow_mut().push(id);
        self.tasks.borrow_mut().remove(&id);
    }
    fn terminate(&self) {
        self.terminated.set(self.terminated.get() + 1);
        self.tasks.borrow_mut().clear();
    }
}
struct CountWake(AtomicUsize);
impl Wake for CountWake {
    fn wake(self: Arc<Self>) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
    fn wake_by_ref(self: &Arc<Self>) {
        self.0.fetch_add(1, Ordering::SeqCst);
    }
}
#[test]
fn task_capacity_errors_drop_and_destroy() {
    let worker = Rc::new(Worker::default());
    let mut p = TaskProcessor::new(worker.clone(), Some(2));
    let a = p.schedule_task(1).unwrap();
    let mut b = p.schedule_task(2).unwrap();
    assert!(p.schedule_task(3).is_none());
    assert_eq!(p.active_tasks(), 2);
    drop(a);
    assert_eq!(*worker.cancelled.borrow(), vec![0]);
    assert_eq!(p.active_tasks(), 1);
    let mut c = p.schedule_task(3).unwrap();
    worker.tasks.borrow_mut().remove(&1).unwrap().send(Err(RuntimeError::new("expected failure"))).unwrap();
    assert!(matches!(poll(&mut b,Waker::noop()),Poll::Ready(Err(e)) if e.message=="expected failure"));
    assert_eq!(p.active_tasks(), 1);
    let wake = Arc::new(CountWake(AtomicUsize::new(0)));
    assert!(poll(&mut c, &Waker::from(wake.clone())).is_pending());
    p.destroy();
    assert!(wake.0.load(Ordering::SeqCst) >= 1);
    assert!(matches!(poll(&mut c, Waker::noop()), Poll::Ready(Err(_))));
    assert_eq!(p.active_tasks(), 0);
    assert!(p.schedule_task(4).is_none());
    p.destroy();
    drop(p);
    assert_eq!(worker.terminated.get(), 1);
}
#[test]
fn module_initialization_fans_out_wakes_and_caches_result() {
    for fail in [false, true] {
        let (tx, rx) = futures_channel::oneshot::channel();
        let worker = Rc::new(Worker::default());
        *worker.init.borrow_mut() = Some(rx);
        let mut p = TaskProcessor::new(worker.clone(), None);
        let mut a = p.init_web_assembly_module(Default::default());
        let mut b = p.init_web_assembly_module(Default::default());
        assert_eq!(worker.init_count.get(), 1);
        let wa = Arc::new(CountWake(AtomicUsize::new(0)));
        let wb = Arc::new(CountWake(AtomicUsize::new(0)));
        assert!(poll(&mut a, &Waker::from(wa.clone())).is_pending());
        assert!(poll(&mut b, &Waker::from(wb.clone())).is_pending());
        tx.send(if fail { Err(RuntimeError::new("bad module")) } else { Ok(json!({"ready":true})) }).unwrap();
        assert_eq!(wa.0.load(Ordering::SeqCst), 1);
        assert_eq!(wb.0.load(Ordering::SeqCst), 1);
        let av = poll(&mut a, Waker::noop());
        let bv = poll(&mut b, Waker::noop());
        assert_eq!(format!("{av:?}"), format!("{bv:?}"));
        assert!(matches!(av, Poll::Ready(_)));
    }
}
#[test]
fn dropping_processor_resolves_pending_initialization() {
    let (_tx, rx) = futures_channel::oneshot::channel();
    let worker = Rc::new(Worker::default());
    *worker.init.borrow_mut() = Some(rx);
    let mut p = TaskProcessor::new(worker.clone(), None);
    let mut init = p.init_web_assembly_module(Default::default());
    assert!(poll(&mut init, Waker::noop()).is_pending());
    drop(p);
    assert!(matches!(poll(&mut init, Waker::noop()), Poll::Ready(Err(_))));
    assert_eq!(worker.terminated.get(), 1);
}
struct ImageHostLog(RefCell<Vec<Value>>);
impl ImageHost for ImageHostLog {
    type Image = RgbaImage;
    fn dimensions(&self, i: &RgbaImage) -> (u32, u32) {
        (i.width, i.height)
    }
    fn image_from_rgba(&self, i: RgbaImage) -> Result<RgbaImage, RuntimeError> {
        Ok(i)
    }
    fn image_pixels(&self, _: &RgbaImage, w: u32, h: u32) -> Result<Vec<u8>, RuntimeError> {
        self.0.borrow_mut().push(json!(["pixels", w, h]));
        Ok(vec![])
    }
    fn resize_image(&self, _: &RgbaImage, w: u32, h: u32) -> Result<RgbaImage, RuntimeError> {
        self.0.borrow_mut().push(json!(["resize", w, h]));
        Ok(RgbaImage::new(w, h, vec![0; (w * h * 4) as usize]).unwrap())
    }
    fn decode_image<'a>(
        &'a self,
        _: &'a [u8],
        mime: &'a str,
        o: ImageDecodeOptions,
    ) -> Option<ImageFuture<'a, RgbaImage>> {
        self.0.borrow_mut().push(json!([mime, o.flip_y, o.skip_color_space_conversion]));
        Some(Box::pin(async { Err(RuntimeError::new("invalid image")) }))
    }
}
#[test]
fn image_helpers_preserve_view_and_options() {
    let host = ImageHostLog(RefCell::new(vec![]));
    let i = get_image_from_typed_array_view(
        &host,
        PixelArrayView { buffer: &[1, 2, 3, 4], byte_offset: 1, byte_length: 2 },
        1,
        1,
    )
    .unwrap();
    assert_eq!(i.pixels, vec![1, 2, 3, 4]);
    assert!(get_image_from_typed_array(&host, &[1, 2, 3], 1, 1).is_err());
    let image = RgbaImage::new(3, 5, vec![0; 60]).unwrap();
    get_image_pixels(&host, &image, None, Some(9)).unwrap();
    let resized = resize_image_to_next_power_of_two(&host, &image).unwrap();
    assert_eq!((resized.width, resized.height), (4, 8));
    let mut decode = load_image_from_typed_array(
        &host,
        &[],
        "image/png",
        ImageDecodeOptions { flip_y: true, skip_color_space_conversion: true },
    )
    .unwrap();
    assert!(matches!(poll(&mut decode, Waker::noop()), Poll::Ready(Err(_))));
    assert_eq!(
        *host.0.borrow(),
        vec![json!(["pixels", 3, 9]), json!(["resize", 4, 8]), json!(["image/png", true, true])]
    );
}
#[test]
fn load_ktx2_uses_scheduled_resource_bytes() {
    struct Loader(Cell<usize>);
    impl Ktx2Loader for Loader {
        fn load<'a>(&'a self, _: &'a Resource) -> Option<Ktx2Future<'a, Vec<u8>>> {
            self.0.set(self.0.get() + 1);
            Some(Box::pin(async { Ok(include_bytes!("data/ktx2/Green4x4.ktx2").to_vec()) }))
        }
    }
    let loader = Loader(Cell::new(0));
    let codec = Ktx2Transcoder::default();
    let r = Resource::new("texture.ktx2").unwrap();
    let mut f = load_ktx2(&codec, Ktx2Source::Resource(&r), &loader).unwrap();
    assert!(matches!(poll(&mut f, Waker::noop()), Poll::Ready(Ok(Ktx2Texture::Image(_)))));
    assert_eq!(loader.0.get(), 1);
}
#[cfg(feature = "native-host")]
struct ThreadWake(std::thread::Thread);
#[cfg(feature = "native-host")]
impl Wake for ThreadWake {
    fn wake(self: Arc<Self>) {
        self.0.unpark();
    }
    fn wake_by_ref(self: &Arc<Self>) {
        self.0.unpark();
    }
}
#[cfg(feature = "native-host")]
fn block<T>(f: impl Future<Output = T>) -> T {
    let mut f = pin!(f);
    let w = Waker::from(Arc::new(ThreadWake(std::thread::current())));
    let end = Instant::now() + Duration::from_secs(10);
    loop {
        if let Poll::Ready(v) = f.as_mut().poll(&mut Context::from_waker(&w)) {
            return v;
        }
        assert!(Instant::now() < end, "host future timed out");
        std::thread::park_timeout(Duration::from_millis(20));
    }
}
#[cfg(feature = "native-host")]
#[test]
fn native_worker_executes_tasks_and_initializes_once() {
    use cesium_rs::core::native_host::NativeTaskWorker;
    let count = Arc::new(AtomicUsize::new(0));
    let c = count.clone();
    let worker = Rc::new(NativeTaskWorker::new(
        |v: u32, _| if v == 0 { Err(RuntimeError::new("zero")) } else { Ok(v * v) },
        move |_| {
            c.fetch_add(1, Ordering::SeqCst);
            Ok(json!(true))
        },
    ));
    let mut p = TaskProcessor::new(worker, Some(2));
    assert_eq!(block(p.schedule_task(7).unwrap()).unwrap(), 49);
    assert!(block(p.schedule_task(0).unwrap()).is_err());
    assert_eq!(block(p.init_web_assembly_module(Default::default())).unwrap(), json!(true));
    assert_eq!(block(p.init_web_assembly_module(Default::default())).unwrap(), json!(true));
    assert_eq!(count.load(Ordering::SeqCst), 1);
}
#[cfg(feature = "native-host")]
#[test]
fn native_http_runs_through_resource_scheduler() {
    use cesium_rs::core::native_host::NativeTransport;
    use std::{
        io::{Read, Write},
        net::TcpListener,
    };
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let addr = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        for status in [200, 404, 204] {
            let (mut stream, _) = listener.accept().unwrap();
            stream.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
            let mut request = vec![];
            let mut byte = [0];
            while !request.ends_with(b"\r\n\r\n") {
                stream.read_exact(&mut byte).unwrap();
                request.push(byte[0]);
            }
            assert!(String::from_utf8_lossy(&request).starts_with("GET /test"));
            let body = if status == 204 { "" } else { "{\"ok\":true}" };
            write!(stream,"HTTP/1.1 {status} result\r\nContent-Length: {}\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n{body}",body.len()).unwrap();
        }
    });
    let client = ResourceClient::new(Rc::new(NativeTransport::default()));
    let resource = Resource::new(format!("http://{addr}/test")).unwrap();
    assert_eq!(block(resource.fetch_json(&client).unwrap()).unwrap(), Some(json!({"ok":true})));
    let error = block(resource.fetch_json(&client).unwrap()).unwrap_err();
    assert_eq!(error.status_code, Some(404));
    assert_eq!(error.response, Some(b"{\"ok\":true}".to_vec()));
    assert_eq!(block(resource.fetch_json(&client).unwrap()).unwrap(), None);
    server.join().unwrap();
}
