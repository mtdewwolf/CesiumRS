//! Concrete browser capabilities behind the `browser` feature. The existing
//! raw C-ABI viewer does not need wasm-bindgen; consumers of these adapters run
//! wasm-bindgen on a build made with `--features browser`.
use super::credit::{Credit, CreditRenderer};
use super::image_helpers::{FetchImageOptions, ImageDecodeOptions, ImageFuture, ImageHost, RgbaImage};
use super::resource::{HttpRequest, HttpResponse, RequestKind, ResourceTransport};
use super::task_processor::{TaskFuture, TaskWorker, WebAssemblyOptions};
use super::{RequestErrorEvent, RuntimeError};
use js_sys::{Array, Function, Promise, Reflect, Uint8Array};
use std::{cell::RefCell, collections::HashMap, rc::Rc};
use wasm_bindgen::{prelude::*, JsCast};
use wasm_bindgen_futures::JsFuture;

#[wasm_bindgen(module = "/web/host/core.mjs")]
extern "C" {
    #[wasm_bindgen(catch,js_name=startRequest)]
    fn start_request(request: &JsValue) -> Result<JsValue, JsValue>;
    #[wasm_bindgen(js_name=abortJob)]
    fn abort_job(job: &JsValue);
    #[wasm_bindgen(js_name=dimensions)]
    fn dimensions(image: &JsValue) -> Array;
    #[wasm_bindgen(catch,js_name=imageFromRgba)]
    fn image_from_rgba(bytes: &[u8], width: u32, height: u32) -> Result<JsValue, JsValue>;
    #[wasm_bindgen(catch,js_name=imagePixels)]
    fn image_pixels(image: &JsValue, width: u32, height: u32) -> Result<Uint8Array, JsValue>;
    #[wasm_bindgen(catch,js_name=resizeImage)]
    fn resize_image(image: &JsValue, width: u32, height: u32) -> Result<JsValue, JsValue>;
    #[wasm_bindgen(catch,js_name=decodeImage)]
    async fn decode_image(
        bytes: &[u8],
        mime_type: &str,
        flip_y: bool,
        skip_color_space_conversion: bool,
    ) -> Result<JsValue, JsValue>;
    #[wasm_bindgen(catch,js_name=decodeResourceImage)]
    async fn decode_resource_image_js(bytes: &[u8], mime_type: &str, options: &JsValue) -> Result<JsValue, JsValue>;
    #[wasm_bindgen(catch,js_name=startImageRequest)]
    fn start_image_request(url: &str, credentials: bool, options: &JsValue) -> Result<JsValue, JsValue>;
    #[wasm_bindgen(js_name=creditElement)]
    fn credit_element(html: &str, id: f64) -> JsValue;
    #[wasm_bindgen(catch,js_name=setPlaybackRate)]
    fn set_playback_rate(video: &JsValue, rate: f64) -> Result<(), JsValue>;
    fn delay(milliseconds: u32) -> Promise;
    #[wasm_bindgen(catch,js_name=createTaskWorker)]
    fn create_task_worker(path: &str, base_url: &str) -> Result<JsValue, JsValue>;
}
#[wasm_bindgen(module = "/web/host/vendor/purify.es.mjs")]
extern "C" {
    #[wasm_bindgen(js_name=default)]
    fn create_purifier() -> JsValue;
}
fn field(object: &JsValue, key: &str) -> JsValue {
    Reflect::get(object, &key.into()).unwrap_or(JsValue::UNDEFINED)
}
fn set(object: &JsValue, key: &str, value: &JsValue) {
    Reflect::set(object, &key.into(), value).expect("plain JS object");
}
fn error(value: JsValue) -> RuntimeError {
    RuntimeError::new(
        field(&value, "message")
            .as_string()
            .or_else(|| value.as_string())
            .unwrap_or_else(|| "Browser operation failed.".into()),
    )
}
fn method(object: &JsValue, name: &str, args: &Array) -> Result<JsValue, JsValue> {
    field(object, name).dyn_into::<Function>()?.apply(object, args)
}
fn json(value: &serde_json::Value) -> Result<JsValue, JsValue> {
    js_sys::JSON::parse(&value.to_string())
}
fn headers(value: JsValue) -> Vec<(String, String)> {
    if let Some(text) = value.as_string() {
        super::query::parse_response_headers(&text)
    } else {
        Vec::new()
    }
}
fn request_error(value: JsValue) -> RequestErrorEvent {
    RequestErrorEvent::new(
        field(&value, "statusCode").as_f64().map(|s| s as u16),
        (!field(&value, "response").is_undefined()).then(|| Uint8Array::new(&field(&value, "response")).to_vec()),
        Some(headers(field(&value, "responseHeaders"))),
    )
}
#[derive(Clone, Default)]
pub struct BrowserTransport {
    active: Rc<RefCell<HashMap<u64, JsValue>>>,
}
struct BrowserJob {
    id: u64,
    job: JsValue,
    active: Rc<RefCell<HashMap<u64, JsValue>>>,
}
impl Drop for BrowserJob {
    fn drop(&mut self) {
        self.active.borrow_mut().remove(&self.id);
        abort_job(&self.job);
    }
}
impl ResourceTransport for BrowserTransport {
    fn execute(&self, request: HttpRequest) -> super::RequestFuture<HttpResponse> {
        let object = js_sys::Object::new();
        set(&object, "url", &request.url.into());
        set(&object, "method", &request.method.into());
        set(&object, "withCredentials", &request.with_credentials.into());
        let h = Array::new();
        for (name, value) in request.headers {
            let pair = Array::new();
            pair.push(&name.into());
            pair.push(&value.into());
            h.push(&pair);
        }
        set(&object, "headers", &h);
        if let Some(body) = request.body {
            set(&object, "body", &Uint8Array::from(body.as_slice()));
        }
        if let RequestKind::Jsonp { function_name, .. } = request.kind {
            set(&object, "kind", &"jsonp".into());
            set(&object, "functionName", &function_name.into());
        }
        let job = match start_request(&object) {
            Ok(job) => job,
            Err(e) => return Box::pin(std::future::ready(Err(request_error(e)))),
        };
        self.active.borrow_mut().insert(request.id, job.clone());
        let guard = BrowserJob { id: request.id, job: job.clone(), active: self.active.clone() };
        Box::pin(async move {
            let result = JsFuture::from(field(&job, "promise").dyn_into::<Promise>().map_err(request_error)?)
                .await
                .map_err(request_error)?;
            let response = HttpResponse {
                status: field(&result, "status").as_f64().unwrap_or(0.0) as u16,
                body: Uint8Array::new(&field(&result, "body")).to_vec(),
                headers: headers(field(&result, "headers")),
            };
            drop(guard);
            Ok(response)
        })
    }
    fn cancel(&self, request: &HttpRequest) {
        if let Some(job) = self.active.borrow().get(&request.id) {
            abort_job(job);
        }
    }
}
#[derive(Clone, Copy, Default)]
pub struct BrowserImageHost;
impl ImageHost for BrowserImageHost {
    type Image = JsValue;
    fn dimensions(&self, image: &JsValue) -> (u32, u32) {
        let d = dimensions(image);
        (d.get(0).as_f64().unwrap_or(0.0) as u32, d.get(1).as_f64().unwrap_or(0.0) as u32)
    }
    fn image_from_rgba(&self, image: RgbaImage) -> Result<JsValue, RuntimeError> {
        image_from_rgba(&image.pixels, image.width, image.height).map_err(error)
    }
    fn image_pixels(&self, image: &JsValue, width: u32, height: u32) -> Result<Vec<u8>, RuntimeError> {
        image_pixels(image, width, height).map(|a| a.to_vec()).map_err(error)
    }
    fn resize_image(&self, image: &JsValue, width: u32, height: u32) -> Result<JsValue, RuntimeError> {
        resize_image(image, width, height).map_err(error)
    }
    fn decode_image<'a>(
        &'a self,
        bytes: &'a [u8],
        mime_type: &'a str,
        options: ImageDecodeOptions,
    ) -> Option<ImageFuture<'a, JsValue>> {
        Some(Box::pin(async move {
            decode_image(bytes, mime_type, options.flip_y, options.skip_color_space_conversion).await.map_err(error)
        }))
    }
    fn load_resource_url<'a>(
        &'a self,
        url: &'a str,
        with_credentials: bool,
        options: FetchImageOptions,
    ) -> Option<ImageFuture<'a, JsValue>> {
        let job = start_image_request(url, with_credentials, &image_options(options, false));
        Some(Box::pin(async move {
            let guard = ImageJob(job.map_err(error)?);
            let promise = field(&guard.0, "promise").dyn_into::<Promise>().map_err(error)?;
            let result = JsFuture::from(promise).await.map_err(error);
            drop(guard);
            result
        }))
    }
    fn decode_resource_image<'a>(
        &'a self,
        bytes: &'a [u8],
        mime_type: &'a str,
        options: FetchImageOptions,
        attach_blob: bool,
    ) -> Option<ImageFuture<'a, JsValue>> {
        Some(Box::pin(async move {
            decode_resource_image_js(bytes, mime_type, &image_options(options, attach_blob)).await.map_err(error)
        }))
    }
}
pub struct BrowserCreditRenderer {
    purifier: JsValue,
    elements: HashMap<u64, (std::sync::Weak<()>, JsValue)>,
}
impl Default for BrowserCreditRenderer {
    fn default() -> Self {
        Self { purifier: create_purifier(), elements: HashMap::new() }
    }
}
impl CreditRenderer for BrowserCreditRenderer {
    type Element = JsValue;
    fn element(&mut self, credit: &Credit) -> JsValue {
        self.elements.retain(|_, (lifetime, _)| lifetime.strong_count() > 0);
        self.elements
            .entry(credit.instance_id())
            .or_insert_with(|| {
                let args = Array::new();
                args.push(&credit.html().into());
                let html = method(&self.purifier, "sanitize", &args)
                    .expect("DOMPurify initialization")
                    .as_string()
                    .unwrap_or_default();
                (credit.element_lifetime(), credit_element(&html, credit.id() as f64))
            })
            .1
            .clone()
    }
}
#[derive(Clone, Copy, Default)]
pub struct BrowserTimer;
impl super::sample_terrain::TerrainSamplingHost for BrowserTimer {
    type Delay<'a> = std::pin::Pin<Box<dyn std::future::Future<Output = ()>>>;
    fn delay(&self, milliseconds: u32) -> Self::Delay<'_> {
        Box::pin(async move {
            let _ = JsFuture::from(delay(milliseconds)).await;
        })
    }
}
impl super::video_synchronizer::VideoElement for web_sys::HtmlVideoElement {
    fn ready_state(&self) -> u8 {
        web_sys::HtmlMediaElement::ready_state(self) as u8
    }
    fn paused(&self) -> bool {
        web_sys::HtmlMediaElement::paused(self)
    }
    fn current_time(&self) -> f64 {
        web_sys::HtmlMediaElement::current_time(self)
    }
    fn duration(&self) -> f64 {
        web_sys::HtmlMediaElement::duration(self)
    }
    fn looping(&self) -> bool {
        self.loop_()
    }
    fn play(&mut self) {
        let _ = web_sys::HtmlMediaElement::play(self);
    }
    fn pause(&mut self) {
        let _ = web_sys::HtmlMediaElement::pause(self);
    }
    fn set_playback_rate(&mut self, rate: f64) -> Result<(), RuntimeError> {
        set_playback_rate(self.as_ref(), rate).map_err(error)
    }
    fn set_current_time(&mut self, time: f64) {
        web_sys::HtmlMediaElement::set_current_time(self, time);
    }
}
/// Cesium worker message protocol over a lazily created module Worker.
/// Rust JSON values replace structured-clone parameters; buffers can be owned
/// by the task without needing a JavaScript transferable-object list.
pub struct BrowserTaskWorker {
    path: String,
    base_url: String,
    worker: RefCell<Option<JsValue>>,
}
impl BrowserTaskWorker {
    pub fn new(path: impl Into<String>, base_url: impl Into<String>) -> Self {
        Self { path: path.into(), base_url: base_url.into(), worker: RefCell::new(None) }
    }
    fn worker(&self) -> Result<JsValue, RuntimeError> {
        if let Some(worker) = &*self.worker.borrow() {
            return Ok(worker.clone());
        }
        let worker = create_task_worker(&self.path, &self.base_url).map_err(error)?;
        *self.worker.borrow_mut() = Some(worker.clone());
        Ok(worker)
    }
}
impl TaskWorker<serde_json::Value, serde_json::Value> for BrowserTaskWorker {
    fn run(&self, id: u64, parameters: serde_json::Value) -> TaskFuture<serde_json::Value> {
        let result = (|| {
            let worker = self.worker()?;
            let args = Array::new();
            args.push(&(id as f64).into());
            args.push(&json(&parameters).map_err(error)?);
            method(&worker, "run", &args).map_err(error)
        })();
        Box::pin(async move {
            let promise = result?.dyn_into::<Promise>().map_err(error)?;
            let value = JsFuture::from(promise).await.map_err(error)?;
            let text = js_sys::JSON::stringify(&value).map_err(error)?.as_string().unwrap_or_else(|| "null".into());
            serde_json::from_str(&text).map_err(|e| RuntimeError::new(e.to_string()))
        })
    }
    fn init_web_assembly_module(&self, options: WebAssemblyOptions) -> TaskFuture<serde_json::Value> {
        let result = (|| {
            let worker = self.worker()?;
            let mut config = serde_json::Map::new();
            for (name, value) in [
                ("modulePath", options.module_path),
                ("wasmBinaryFile", options.wasm_binary_file),
                ("fallbackModulePath", options.fallback_module_path),
            ] {
                if let Some(value) = value {
                    config.insert(name.into(), value.into());
                }
            }
            let args = Array::new();
            args.push(&json(&config.into()).map_err(error)?);
            method(&worker, "initialize", &args).map_err(error)
        })();
        Box::pin(async move {
            let promise = result?.dyn_into::<Promise>().map_err(error)?;
            let value = JsFuture::from(promise).await.map_err(error)?;
            let text = js_sys::JSON::stringify(&value).map_err(error)?.as_string().unwrap_or_else(|| "null".into());
            serde_json::from_str(&text).map_err(|e| RuntimeError::new(e.to_string()))
        })
    }
    fn cancel_task(&self, id: u64) {
        if let Some(worker) = &*self.worker.borrow() {
            let args = Array::new();
            args.push(&(id as f64).into());
            let _ = method(worker, "cancel", &args);
        }
    }
    fn terminate(&self) {
        if let Some(worker) = self.worker.borrow_mut().take() {
            let _ = method(&worker, "terminate", &Array::new());
        }
    }
}

thread_local! {
    static VIDEOS:RefCell<HashMap<u64,(std::rc::Weak<RefCell<super::VideoSynchronizer<web_sys::HtmlVideoElement>>>,u64)>>=RefCell::new(HashMap::new());
    static NEXT_VIDEO:std::cell::Cell<u64>=const { std::cell::Cell::new(0) };
}
pub struct BrowserVideoOptions {
    pub clock: Option<Rc<RefCell<super::Clock>>>,
    pub element: Option<web_sys::HtmlVideoElement>,
    pub epoch: Option<super::JulianDate>,
    pub tolerance: Option<f64>,
}
/// Automatically subscribes to clock ticks and media seeked events. Event
/// listeners capture a thread-local ID, keeping the public Clock events Send.
/// Destroying or replacing a clock/element releases its old subscription.
pub struct BrowserVideoBinding {
    id: u64,
    clock_generation: u64,
    pub synchronizer: Rc<RefCell<super::VideoSynchronizer<web_sys::HtmlVideoElement>>>,
    clock: Option<(std::rc::Weak<RefCell<super::Clock>>, u64)>,
    seeked: Option<(web_sys::HtmlVideoElement, Closure<dyn FnMut(web_sys::Event)>)>,
}
impl BrowserVideoBinding {
    pub fn new(options: BrowserVideoOptions) -> Result<Self, RuntimeError> {
        let id = NEXT_VIDEO.with(|next| {
            let id = next.get();
            next.set(id.wrapping_add(1));
            id
        });
        let synchronizer = Rc::new(RefCell::new(super::VideoSynchronizer::new(None, options.epoch, options.tolerance)));
        VIDEOS.with(|videos| videos.borrow_mut().insert(id, (Rc::downgrade(&synchronizer), 0)));
        let mut binding = Self { id, clock_generation: 0, synchronizer, clock: None, seeked: None };
        binding.set_clock(options.clock);
        binding.set_element(options.element)?;
        Ok(binding)
    }
    fn unsubscribe_clock(&mut self) {
        let Some((clock, listener)) = self.clock.take() else {
            return;
        };
        if let Some(value) = clock.upgrade() {
            if let Ok(mut value) = value.try_borrow_mut() {
                value.on_tick_state.remove_event_listener(listener);
            } else {
                wasm_bindgen_futures::spawn_local(async move {
                    let _ = JsFuture::from(Promise::resolve(&JsValue::UNDEFINED)).await;
                    if let Some(clock) = clock.upgrade() {
                        clock.borrow_mut().on_tick_state.remove_event_listener(listener);
                    }
                });
            }
        }
    }
    pub fn set_clock(&mut self, clock: Option<Rc<RefCell<super::Clock>>>) {
        if self.synchronizer.borrow().is_destroyed() {
            return;
        }
        self.unsubscribe_clock();
        self.clock_generation = self.clock_generation.wrapping_add(1);
        let generation = self.clock_generation;
        VIDEOS.with(|videos| {
            if let Some(entry) = videos.borrow_mut().get_mut(&self.id) {
                entry.1 = generation;
            }
        });
        if let Some(clock) = clock {
            let id = self.id;
            let listener = clock.borrow_mut().on_tick_state.add_event_listener(move |tick| {
                let video = VIDEOS.with(|videos| {
                    videos.borrow().get(&id).filter(|entry| entry.1 == generation).and_then(|entry| entry.0.upgrade())
                });
                if let Some(video) = video {
                    video.borrow_mut().on_tick_state(tick);
                }
            });
            self.clock = Some((Rc::downgrade(&clock), listener));
        }
    }
    pub fn set_element(&mut self, element: Option<web_sys::HtmlVideoElement>) -> Result<(), RuntimeError> {
        if self.synchronizer.borrow().is_destroyed() {
            return Err(RuntimeError::new("VideoSynchronizer was destroyed."));
        }
        if let Some((old, listener)) = self.seeked.take() {
            let _ = old.remove_event_listener_with_callback("seeked", listener.as_ref().unchecked_ref());
        }
        self.synchronizer.borrow_mut().set_element(element.clone());
        if let Some(element) = element {
            let weak = Rc::downgrade(&self.synchronizer);
            let listener = Closure::wrap(Box::new(move |_: web_sys::Event| {
                if let Some(video) = weak.upgrade() {
                    video.borrow_mut().on_seeked();
                }
            }) as Box<dyn FnMut(web_sys::Event)>);
            element.add_event_listener_with_callback("seeked", listener.as_ref().unchecked_ref()).map_err(error)?;
            self.seeked = Some((element, listener));
        }
        Ok(())
    }
    pub fn destroy(&mut self) {
        self.unsubscribe_clock();
        if let Some((element, listener)) = self.seeked.take() {
            let _ = element.remove_event_listener_with_callback("seeked", listener.as_ref().unchecked_ref());
        }
        self.synchronizer.borrow_mut().destroy();
        VIDEOS.with(|videos| videos.borrow_mut().remove(&self.id));
    }
}
impl Drop for BrowserVideoBinding {
    fn drop(&mut self) {
        self.destroy();
    }
}

/// Ready-to-use browser entry points. Rust applications can also use the
/// individual adapters without this wasm-bindgen facade.
#[wasm_bindgen]
pub struct BrowserCore {
    client: super::ResourceClient,
    credits: RefCell<BrowserCreditRenderer>,
}
#[wasm_bindgen]
impl BrowserCore {
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        let mut client = super::ResourceClient::new(Rc::new(BrowserTransport::default()));
        let location = field(&js_sys::global(), "location");
        client.page_protocol = field(&location, "protocol").as_string().unwrap_or_default();
        client.scheduler.borrow_mut().page_url = field(&location, "href").as_string().unwrap_or_default();
        Self { client, credits: RefCell::new(BrowserCreditRenderer::default()) }
    }
    #[wasm_bindgen(js_name=addTrustedServer)]
    pub fn add_trusted_server(&self, host: &str, port: u16) {
        self.client.trusted_servers.borrow_mut().add(host, port);
    }
    #[wasm_bindgen(js_name=fetchJson)]
    pub async fn fetch_json(&self, url: &str) -> Result<JsValue, JsValue> {
        let resource = super::Resource::new(url).ok_or_else(|| JsValue::from_str("Invalid resource URL."))?;
        let future =
            resource.fetch_json(&self.client).ok_or_else(|| JsValue::from_str("Resource request was throttled."))?;
        match future.await.map_err(|e| JsValue::from_str(&e.to_string()))? {
            Some(value) => json(&value),
            None => Ok(JsValue::UNDEFINED),
        }
    }
    #[wasm_bindgen(js_name=fetchImage)]
    pub async fn fetch_image(
        &self,
        url: &str,
        flip_y: bool,
        skip_color_space_conversion: bool,
    ) -> Result<JsValue, JsValue> {
        let resource = super::Resource::new(url).ok_or_else(|| JsValue::from_str("Invalid resource URL."))?;
        let future = self
            .client
            .fetch_image(
                &resource,
                Rc::new(BrowserImageHost),
                FetchImageOptions { flip_y, skip_color_space_conversion, ..Default::default() },
            )
            .ok_or_else(|| JsValue::from_str("Resource request was throttled."))?;
        future.await.map(|value| value.unwrap_or(JsValue::UNDEFINED)).map_err(|e| JsValue::from_str(&e.to_string()))
    }
    #[wasm_bindgen(js_name=creditElement)]
    pub fn credit(&self, html: &str, show_on_screen: bool) -> JsValue {
        self.credits.borrow_mut().element(&Credit::new(html, show_on_screen))
    }
    #[wasm_bindgen(js_name=rgbaCanvas)]
    pub fn rgba_canvas(&self, bytes: Vec<u8>, width: u32, height: u32) -> Result<JsValue, JsValue> {
        super::image_helpers::get_image_from_typed_array(&BrowserImageHost, &bytes, width, height)
            .map_err(|e| JsValue::from_str(&e.message))
    }
    #[wasm_bindgen(js_name=transcodeKtx2)]
    pub async fn transcode_ktx2(&self, bytes: Vec<u8>, formats: JsValue) -> Result<JsValue, JsValue> {
        use super::ktx2_transcoder::{Ktx2Transcoder, RustBasisTranscoder, SupportedTextureFormats};
        let supported = |name| field(&formats, name).as_bool().unwrap_or(false);
        let transcoder = Ktx2Transcoder {
            basis: RustBasisTranscoder,
            supported_formats: SupportedTextureFormats {
                s3tc: supported("s3tc"),
                pvrtc: supported("pvrtc"),
                astc: supported("astc"),
                etc: supported("etc"),
                etc1: supported("etc1"),
                bc7: supported("bc7"),
            },
        };
        transcoder.transcode(&bytes).await.map(texture_value).map_err(|e| JsValue::from_str(&e.message))
    }
}
impl Default for BrowserCore {
    fn default() -> Self {
        Self::new()
    }
}
fn texture_buffer_value(buffer: super::CompressedTextureBuffer) -> JsValue {
    let object = js_sys::Object::new();
    set(&object, "internalFormat", &(buffer.internal_format as f64).into());
    set(&object, "width", &(buffer.width as f64).into());
    set(&object, "height", &(buffer.height as f64).into());
    if let Some(datatype) = buffer.pixel_datatype {
        set(&object, "pixelDatatype", &(datatype as f64).into());
    }
    let bytes = Uint8Array::from(buffer.buffer_view());
    let view: JsValue = match buffer.pixel_datatype.and_then(super::PixelDatatype::from_value) {
        Some(super::PixelDatatype::HalfFloat) => js_sys::Uint16Array::new(&bytes.buffer()).into(),
        Some(super::PixelDatatype::Float) => js_sys::Float32Array::new(&bytes.buffer()).into(),
        _ => bytes.into(),
    };
    set(&object, "bufferView", &view);
    set(&object, "arrayBufferView", &view);
    object.into()
}
fn texture_value(texture: super::ktx2_transcoder::Ktx2Texture) -> JsValue {
    use super::ktx2_transcoder::{CubeFace, Ktx2Texture};
    match texture {
        Ktx2Texture::Image(buffer) => texture_buffer_value(buffer),
        Ktx2Texture::Mipmaps(levels) => {
            let array = Array::new();
            for level in levels {
                array.push(&texture_buffer_value(level));
            }
            array.into()
        }
        Ktx2Texture::Cubemap(levels) => {
            let array = Array::new();
            for level in levels {
                let object = js_sys::Object::new();
                for (face, buffer) in level {
                    let name = match face {
                        CubeFace::PositiveX => "positiveX",
                        CubeFace::NegativeX => "negativeX",
                        CubeFace::PositiveY => "positiveY",
                        CubeFace::NegativeY => "negativeY",
                        CubeFace::PositiveZ => "positiveZ",
                        CubeFace::NegativeZ => "negativeZ",
                    };
                    set(&object, name, &texture_buffer_value(buffer));
                }
                array.push(&object);
            }
            array.into()
        }
    }
}

struct ImageJob(JsValue);
impl Drop for ImageJob {
    fn drop(&mut self) {
        abort_job(&self.0);
    }
}
fn image_options(options: FetchImageOptions, attach_blob: bool) -> JsValue {
    let object = js_sys::Object::new();
    set(&object, "preferImageBitmap", &options.prefer_image_bitmap.into());
    set(&object, "flipY", &options.flip_y.into());
    set(&object, "skipColorSpaceConversion", &options.skip_color_space_conversion.into());
    set(&object, "attachBlob", &attach_blob.into());
    object.into()
}

impl Drop for BrowserTaskWorker {
    fn drop(&mut self) {
        if let Some(worker) = self.worker.get_mut().take() {
            let _ = method(&worker, "terminate", &Array::new());
        }
    }
}
