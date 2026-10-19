//! Native network, timers and worker tasks. Blocking HTTP and CPU callbacks
//! run on background threads; callers may use any Future executor.
use super::request::RequestFuture;
use super::resource::{HttpRequest, HttpResponse, ResourceTransport};
use super::task_processor::{TaskFuture, TaskWorker, WebAssemblyOptions};
use super::{RequestErrorEvent, RuntimeError};
use futures_channel::oneshot;
use std::{
    collections::HashMap,
    future::Future,
    pin::Pin,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc, Mutex,
    },
    task::{Context, Poll},
};

#[derive(Clone)]
pub struct NativeTransport {
    agent: ureq::Agent,
    active: Arc<Mutex<HashMap<u64, Arc<AtomicBool>>>>,
}
impl Default for NativeTransport {
    fn default() -> Self {
        let config = ureq::Agent::config_builder().http_status_as_error(false).build();
        Self { agent: ureq::Agent::new_with_config(config), active: Arc::new(Mutex::new(HashMap::new())) }
    }
}
struct HttpJob {
    id: u64,
    active: Arc<Mutex<HashMap<u64, Arc<AtomicBool>>>>,
    cancelled: Arc<AtomicBool>,
    receiver: oneshot::Receiver<Result<HttpResponse, RequestErrorEvent>>,
}
impl Future for HttpJob {
    type Output = Result<HttpResponse, RequestErrorEvent>;
    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let job = self.get_mut();
        Pin::new(&mut job.receiver)
            .poll(cx)
            .map(|r| r.unwrap_or_else(|_| Err(super::resource_client::request_error("HTTP request cancelled."))))
    }
}
impl Drop for HttpJob {
    fn drop(&mut self) {
        self.cancelled.store(true, Ordering::Relaxed);
        self.active.lock().unwrap_or_else(|e| e.into_inner()).remove(&self.id);
    }
}
impl ResourceTransport for NativeTransport {
    fn execute(&self, request: HttpRequest) -> RequestFuture<HttpResponse> {
        let agent = self.agent.clone();
        let active = self.active.clone();
        let cancelled = Arc::new(AtomicBool::new(false));
        active.lock().unwrap_or_else(|e| e.into_inner()).insert(request.id, cancelled.clone());
        let (sender, receiver) = oneshot::channel();
        let signal = cancelled.clone();
        let id = request.id;
        std::thread::spawn(move || {
            let run = || -> Result<HttpResponse, RequestErrorEvent> {
                let mut builder = ureq::http::Request::builder().method(request.method.as_str()).uri(&request.url);
                for (name, value) in &request.headers {
                    builder = builder.header(name, value);
                }
                let request = builder
                    .body(request.body.unwrap_or_default())
                    .map_err(|e| super::resource_client::request_error(e.to_string()))?;
                let mut response =
                    agent.run(request).map_err(|e| super::resource_client::request_error(e.to_string()))?;
                let status = response.status().as_u16();
                let headers: Vec<_> = response
                    .headers()
                    .iter()
                    .map(|(key, value)| (key.to_string(), String::from_utf8_lossy(value.as_bytes()).into_owned()))
                    .collect();
                let mut body = Vec::new();
                let mut reader = response.body_mut().as_reader();
                let mut chunk = [0; 16384];
                loop {
                    if signal.load(Ordering::Relaxed) {
                        return Err(super::resource_client::request_error("HTTP request cancelled."));
                    }
                    let length = std::io::Read::read(&mut reader, &mut chunk)
                        .map_err(|e| super::resource_client::request_error(e.to_string()))?;
                    if length == 0 {
                        break;
                    }
                    body.extend_from_slice(&chunk[..length]);
                }
                if !(200..300).contains(&status) {
                    return Err(RequestErrorEvent::new(Some(status), Some(body), Some(headers)));
                }
                Ok(HttpResponse { status, body, headers })
            };
            let result = run();
            let _ = sender.send(result);
        });
        Box::pin(HttpJob { id, active, cancelled, receiver })
    }
    fn cancel(&self, request: &HttpRequest) {
        if let Some(signal) = self.active.lock().unwrap_or_else(|e| e.into_inner()).get(&request.id) {
            signal.store(true, Ordering::Relaxed);
        }
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct NativeTimer;
impl super::sample_terrain::TerrainSamplingHost for NativeTimer {
    type Delay<'a> = Pin<Box<dyn Future<Output = ()>>>;
    fn delay(&self, milliseconds: u32) -> Self::Delay<'_> {
        let (sender, receiver) = oneshot::channel();
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(milliseconds as u64));
            let _ = sender.send(());
        });
        Box::pin(async move {
            let _ = receiver.await;
        })
    }
}

#[derive(Clone)]
pub struct TaskCancellation(Arc<AtomicBool>);
impl TaskCancellation {
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Relaxed)
    }
}
type TaskCallback<P, R> = dyn Fn(P, TaskCancellation) -> Result<R, RuntimeError> + Send + Sync;
type ModuleCallback = dyn Fn(WebAssemblyOptions) -> Result<serde_json::Value, RuntimeError> + Send + Sync;
/// Owns native worker threads. Native task functions receive a cooperative
/// cancellation token; completion is discarded after cancel/terminate.
/// The optional module callback configures a native/Wasm runtime chosen by the
/// application; browser module loading is provided by BrowserTaskWorker.
pub struct NativeTaskWorker<P, R> {
    run: Arc<TaskCallback<P, R>>,
    init: Arc<ModuleCallback>,
    active: Arc<Mutex<HashMap<u64, Arc<AtomicBool>>>>,
    terminated: Arc<AtomicBool>,
}
impl<P, R> NativeTaskWorker<P, R> {
    pub fn new(
        run: impl Fn(P, TaskCancellation) -> Result<R, RuntimeError> + Send + Sync + 'static,
        initialize: impl Fn(WebAssemblyOptions) -> Result<serde_json::Value, RuntimeError> + Send + Sync + 'static,
    ) -> Self {
        Self {
            run: Arc::new(run),
            init: Arc::new(initialize),
            active: Arc::new(Mutex::new(HashMap::new())),
            terminated: Arc::new(AtomicBool::new(false)),
        }
    }
}
impl<P: Send + 'static, R: Send + 'static> TaskWorker<P, R> for NativeTaskWorker<P, R> {
    fn run(&self, id: u64, parameters: P) -> TaskFuture<R> {
        let callback = self.run.clone();
        let active = self.active.clone();
        let terminated = self.terminated.clone();
        let signal = Arc::new(AtomicBool::new(false));
        let token = TaskCancellation(signal.clone());
        active.lock().unwrap_or_else(|e| e.into_inner()).insert(id, signal.clone());
        let (sender, receiver) = oneshot::channel();
        std::thread::spawn(move || {
            let result = if terminated.load(Ordering::Relaxed) {
                Err(RuntimeError::new("TaskProcessor was destroyed."))
            } else {
                callback(parameters, token)
            };
            active.lock().unwrap_or_else(|e| e.into_inner()).remove(&id);
            let _ = sender.send(if signal.load(Ordering::Relaxed) {
                Err(RuntimeError::new("Task cancelled."))
            } else {
                result
            });
        });
        Box::pin(async move { receiver.await.unwrap_or_else(|_| Err(RuntimeError::new("Worker terminated."))) })
    }
    fn init_web_assembly_module(&self, options: WebAssemblyOptions) -> TaskFuture<serde_json::Value> {
        let callback = self.init.clone();
        let (sender, receiver) = oneshot::channel();
        std::thread::spawn(move || {
            let _ = sender.send(callback(options));
        });
        Box::pin(
            async move { receiver.await.unwrap_or_else(|_| Err(RuntimeError::new("Module initialization failed."))) },
        )
    }
    fn cancel_task(&self, id: u64) {
        if let Some(signal) = self.active.lock().unwrap_or_else(|e| e.into_inner()).get(&id) {
            signal.store(true, Ordering::Relaxed);
        }
    }
    fn terminate(&self) {
        self.terminated.store(true, Ordering::Relaxed);
        for signal in self.active.lock().unwrap_or_else(|e| e.into_inner()).values() {
            signal.store(true, Ordering::Relaxed);
        }
    }
}

impl<P, R> Drop for NativeTaskWorker<P, R> {
    fn drop(&mut self) {
        self.terminated.store(true, Ordering::Relaxed);
        for signal in self.active.lock().unwrap_or_else(|e| e.into_inner()).values() {
            signal.store(true, Ordering::Relaxed);
        }
    }
}
