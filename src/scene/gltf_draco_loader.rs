//! Buffer-view loading followed by frame-driven, concurrency-limited Draco work.
use super::{
    gltf_pipeline::BufferSource,
    resource_loader::{loader_error, ManagedLoader},
    CacheLease, DracoLoader, FrameState, GltfBufferViewLoader, LoadedGltfJson, ResourceLoadFuture, ResourceLoader,
    ResourceLoaderState, SceneRenderRequest,
};
use crate::core::{ProcessedTask, RuntimeError, TaskFuture, TaskProcessor, TaskWorker, WebAssemblyOptions};
use futures_util::FutureExt;
use serde_json::{json, Value};
use std::{
    cell::RefCell,
    future::Future,
    pin::Pin,
    rc::Rc,
    sync::Arc,
    task::{Context, Poll, Waker},
};
/// Portable fallback. Native/browser hosts can supply a TaskProcessor backed by
/// their worker implementation; the same capacity and cancellation rules apply.
pub struct DracoDecodeWorker;
impl TaskWorker<Value, Value> for DracoDecodeWorker {
    fn run(&self, _: u64, parameters: Value) -> TaskFuture<Value> {
        Box::pin(async move { crate::workers::decode_draco(&parameters) })
    }
    fn init_web_assembly_module(&self, _: WebAssemblyOptions) -> TaskFuture<Value> {
        Box::pin(async { Ok(Value::Bool(true)) })
    }
    fn cancel_task(&self, _: u64) {}
    fn terminate(&self) {}
}
#[derive(Clone, Debug, PartialEq)]
pub struct DecodedDraco {
    pub indices: Value,
    pub vertex_attributes: Value,
}
enum ViewLease {
    Cached(CacheLease<GltfBufferViewLoader>),
    Owned(Rc<GltfBufferViewLoader>),
}
impl ViewLease {
    fn loader(&self) -> Rc<GltfBufferViewLoader> {
        match self {
            Self::Cached(l) => l.loader(),
            Self::Owned(l) => l.clone(),
        }
    }
}
type ViewFactory = Box<dyn FnOnce() -> Result<Rc<GltfBufferViewLoader>, RuntimeError>>;
enum Decoder {
    Processor(Rc<RefCell<TaskProcessor<Value, Value>>>),
    Initialized(Rc<DracoLoader>),
}
pub struct GltfDracoLoader {
    data: ManagedLoader<BufferSource>,
    child: Rc<RefCell<Option<ViewLease>>>,
    factory: RefCell<Option<ViewFactory>>,
    parameters: Value,
    decoder: Decoder,
    task: RefCell<Option<ProcessedTask<Value, Value>>>,
    decoded: RefCell<Option<Arc<Value>>>,
}
impl GltfDracoLoader {
    pub fn new(
        gltf: &Value,
        primitive: &Value,
        extension: &Value,
        child: CacheLease<GltfBufferViewLoader>,
        decoder: Rc<RefCell<TaskProcessor<Value, Value>>>,
        key: Option<String>,
    ) -> Result<Self, RuntimeError> {
        Self::with_source(
            gltf,
            primitive,
            extension,
            Some(ViewLease::Cached(child)),
            None,
            Decoder::Processor(decoder),
            key,
        )
    }
    pub fn from_host(
        gltf: Arc<LoadedGltfJson>,
        primitive: Value,
        extension: Value,
        factory: impl FnOnce() -> Result<Rc<GltfBufferViewLoader>, RuntimeError> + 'static,
        decoder: Rc<DracoLoader>,
        key: Option<String>,
    ) -> Result<Self, RuntimeError> {
        Self::with_source(
            &gltf.json,
            &primitive,
            &extension,
            None,
            Some(Box::new(factory)),
            Decoder::Initialized(decoder),
            key,
        )
    }
    fn with_source(
        gltf: &Value,
        primitive: &Value,
        extension: &Value,
        child: Option<ViewLease>,
        factory: Option<ViewFactory>,
        decoder: Decoder,
        key: Option<String>,
    ) -> Result<Self, RuntimeError> {
        let id =
            extension["bufferView"].as_u64().ok_or_else(|| RuntimeError::new("Invalid Draco buffer view"))? as usize;
        let view = gltf["bufferViews"].get(id).ok_or_else(|| RuntimeError::new("Invalid Draco buffer view"))?;
        let mut skip = Vec::<String>::new();
        if let Some(attributes) = primitive["attributes"].as_object() {
            for (name, id) in attributes {
                let Some(id) = id.as_u64() else {
                    continue;
                };
                if gltf["accessors"][id as usize]["componentType"] != 5126 {
                    continue;
                }
                let kind = if name.starts_with("POSITION") {
                    Some("POSITION")
                } else if name.starts_with("NORMAL") {
                    Some("NORMAL")
                } else if name.starts_with("COLOR") {
                    Some("COLOR")
                } else if name.starts_with("TEXCOORD") {
                    Some("TEX_COORD")
                } else {
                    None
                };
                if let Some(kind) = kind {
                    if !skip.iter().any(|s| s == kind) {
                        skip.push(kind.into());
                    }
                }
            }
        }
        Ok(Self {
            data: ManagedLoader::new(key, None),
            child: Rc::new(RefCell::new(child)),
            factory: RefCell::new(factory),
            parameters: json!({"bufferView":view,"compressedAttributes":extension["attributes"],"dequantizeInShader":true,"attributesToSkipTransform":skip}),
            decoder,
            task: RefCell::new(None),
            decoded: RefCell::new(None),
        })
    }
    pub fn decoded_draco(&self) -> Option<DecodedDraco> {
        self.decoded_data()
            .map(|v| DecodedDraco { indices: v["indices"].clone(), vertex_attributes: v["vertexAttributes"].clone() })
    }
    pub fn decoded_data(&self) -> Option<Arc<Value>> {
        self.decoded.borrow().clone()
    }
    fn complete(&self, result: Result<Value, RuntimeError>) -> Result<bool, RuntimeError> {
        self.child.borrow_mut().take();
        self.data.unload();
        match result {
            Ok(result) => {
                *self.decoded.borrow_mut() =
                    Some(Arc::new(json!({"indices":result["indexArray"],"vertexAttributes":result["attributeData"]})));
                self.data.set_state(ResourceLoaderState::Ready);
                Ok(true)
            }
            Err(error) => {
                self.data.set_state(ResourceLoaderState::Failed);
                Err(loader_error("Failed to load Draco", Some(&error)))
            }
        }
    }
}
impl ResourceLoader for GltfDracoLoader {
    fn cache_key(&self) -> Option<&str> {
        self.data.key.as_deref()
    }
    fn state(&self) -> ResourceLoaderState {
        self.data.state()
    }
    fn is_destroyed(&self) -> bool {
        self.data.is_destroyed()
    }
    fn unload(&self) {
        self.task.borrow_mut().take();
        self.factory.borrow_mut().take();
        self.child.borrow_mut().take();
        self.data.unload();
        self.decoded.borrow_mut().take();
    }
    fn destroy(&self) {
        self.unload();
        self.data.destroy();
    }
    fn load(&self) -> ResourceLoadFuture {
        let started = if self.is_destroyed() {
            Ok(())
        } else if let Some(factory) = self.factory.borrow_mut().take() {
            factory().map(|view| {
                *self.child.borrow_mut() = Some(ViewLease::Owned(view));
            })
        } else {
            Ok(())
        };
        let child = self.child.borrow().as_ref().map(ViewLease::loader);
        let owner = Rc::downgrade(&self.child);
        self.data.load_with_state("Failed to load Draco".into(), ResourceLoaderState::Processing, move || {
            let request = child.as_ref().map(|c| c.load());
            let weak = child.as_ref().map(Rc::downgrade);
            async move {
                let result = async move {
                    started?;
                    request.ok_or_else(|| RuntimeError::new("Draco buffer view was unloaded"))?.await?;
                    weak.and_then(|w| w.upgrade())
                        .and_then(|l| l.typed_array())
                        .ok_or_else(|| RuntimeError::new("Draco buffer view was unloaded"))
                }
                .await;
                if result.is_err() {
                    if let Some(owner) = owner.upgrade() {
                        owner.borrow_mut().take();
                    }
                }
                result
            }
            .boxed_local()
        })
    }
    fn process(&self, frame: &FrameState) -> Result<bool, RuntimeError> {
        if self.is_destroyed() {
            return Err(RuntimeError::new("Draco loader was destroyed"));
        }
        if self.state() == ResourceLoaderState::Ready {
            return Ok(true);
        }
        if self.state() != ResourceLoaderState::Processing {
            return Ok(false);
        }
        let pending = self.task.borrow_mut().take();
        if let Some(mut task) = pending {
            let waker = frame.render_request.as_ref().map(SceneRenderRequest::waker);
            let mut context = Context::from_waker(waker.as_ref().unwrap_or(Waker::noop()));
            match Pin::new(&mut task).poll(&mut context) {
                Poll::Ready(result) => return self.complete(result),
                Poll::Pending => {
                    *self.task.borrow_mut() = Some(task);
                    return Ok(false);
                }
            }
        }
        let Some(bytes) = self.data.value() else {
            return Ok(false);
        };
        let mut parameters = self.parameters.clone();
        parameters["array"] = json!(bytes.bytes());
        let task = match &self.decoder {
            Decoder::Processor(decoder) => decoder.borrow_mut().schedule_task(parameters),
            Decoder::Initialized(decoder) => {
                let task = decoder.decode_buffer_view(parameters)?;
                if task.is_none() && !decoder.is_ready() {
                    if let Some(request) = &frame.render_request {
                        request.request_render();
                    }
                }
                task
            }
        };
        if let Some(task) = task {
            *self.task.borrow_mut() = Some(task);
            if let Some(request) = &frame.render_request {
                request.request_render();
            }
        }
        Ok(false)
    }
}
