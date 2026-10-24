//! CesiumJS `GltfDracoLoader`: loads a `KHR_draco_mesh_compression` buffer
//! view, then decodes it on the [`DracoLoader`] during `process`.
//!
//! `load` takes the buffer view loader from the host's resource cache (the
//! `get_buffer_view_loader` callback, upstream's `resourceCache.getBufferViewLoader`)
//! and leaves the loader PROCESSING. Each `process` call first settles a
//! decode that has finished — where JavaScript's promise callbacks would have
//! run between frames — then schedules the decode when the bytes are ready.
//! FLOAT attributes skip Draco's dequantization transforms so they stay
//! quantized for the shader (`dequantizeInShader`).
//!
//! Unloading releases this loader's buffer view lease, as upstream's
//! `resourceCache.unload` does. Decode results are the `decodeDraco` worker's
//! JSON `indexArray` and `attributeData`.

use super::gltf_json_loader::LoadedGltfJson;
use super::gltf_pipeline::{js, BufferSource};
use super::resource_loader::loader_error;
use super::{DracoLoader, FrameState, GltfBufferViewLoader, ResourceLoadFuture, ResourceLoader, ResourceLoaderState};
use crate::core::task_processor::ProcessedTask;
use crate::core::{ComponentDatatype, RuntimeError};
use futures_util::FutureExt;
use serde_json::{json, Value};
use std::cell::RefCell;
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;
use std::sync::Arc;
use std::task::{Context, Poll, Waker};

/// `decodedData`: the decoded index array and vertex attributes.
#[derive(Clone, Debug, PartialEq)]
pub struct DecodedDraco {
    pub indices: Value,
    pub vertex_attributes: Value,
}

type ViewLoaderFactory = Box<dyn FnOnce() -> Result<Rc<GltfBufferViewLoader>, RuntimeError>>;

struct Inner {
    gltf: Option<Arc<LoadedGltfJson>>,
    primitive: Option<Value>,
    draco: Value,
    get_buffer_view_loader: Option<ViewLoaderFactory>,
    buffer_view_loader: Option<Rc<GltfBufferViewLoader>>,
    buffer_view_typed_array: Option<BufferSource>,
    decode: Option<ProcessedTask<Value, Value>>,
    decode_started: bool,
    decoded_data: Option<DecodedDraco>,
    draco_error: Option<RuntimeError>,
    state: ResourceLoaderState,
    promise: Option<ResourceLoadFuture>,
    destroyed: bool,
}

impl Inner {
    fn unload(&mut self) {
        self.buffer_view_loader = None;
        self.buffer_view_typed_array = None;
        self.decoded_data = None;
        self.gltf = None;
        self.primitive = None;
    }

    /// `handleError`: unload, fail, and return the error to throw.
    fn fail(&mut self, error: &RuntimeError) -> RuntimeError {
        self.unload();
        self.state = ResourceLoaderState::Failed;
        loader_error("Failed to load Draco", Some(error))
    }
}

pub struct GltfDracoLoader {
    key: Option<String>,
    decoder: Rc<DracoLoader>,
    inner: Rc<RefCell<Inner>>,
}

/// The Draco attribute type a glTF attribute name maps to, by semantic prefix.
fn draco_attribute_type(attribute: &str) -> Option<&'static str> {
    [("POSITION", "POSITION"), ("NORMAL", "NORMAL"), ("COLOR", "COLOR"), ("TEXCOORD", "TEX_COORD")]
        .into_iter()
        .find(|(semantic, _)| attribute.starts_with(semantic))
        .map(|(_, draco)| draco)
}

impl GltfDracoLoader {
    /// `primitive` is the glTF primitive and `draco` its `KHR_draco_mesh_compression` object.
    pub fn new(
        gltf: Arc<LoadedGltfJson>,
        primitive: Value,
        draco: Value,
        get_buffer_view_loader: impl FnOnce() -> Result<Rc<GltfBufferViewLoader>, RuntimeError> + 'static,
        decoder: Rc<DracoLoader>,
        cache_key: Option<String>,
    ) -> Self {
        Self {
            key: cache_key,
            decoder,
            inner: Rc::new(RefCell::new(Inner {
                gltf: Some(gltf),
                primitive: Some(primitive),
                draco,
                get_buffer_view_loader: Some(Box::new(get_buffer_view_loader)),
                buffer_view_loader: None,
                buffer_view_typed_array: None,
                decode: None,
                decode_started: false,
                decoded_data: None,
                draco_error: None,
                state: ResourceLoaderState::Unloaded,
                promise: None,
                destroyed: false,
            })),
        }
    }

    pub fn decoded_data(&self) -> Option<DecodedDraco> {
        self.inner.borrow().decoded_data.clone()
    }

    /// The decode request `process` sends to the `decodeDraco` worker.
    fn decode_options(inner: &Inner, bytes: &BufferSource) -> Result<Value, RuntimeError> {
        let type_error = |e: super::gltf_pipeline::PipelineError| RuntimeError::new(e.to_string());
        let gltf = &inner.gltf.as_ref().ok_or_else(|| RuntimeError::new("glTF was unloaded"))?.json;
        let draco = &inner.draco;
        let buffer_view_id = js::get(Some(draco), "bufferView").cloned().unwrap_or(Value::Null);
        let buffer_view = js::index(js::get(Some(gltf), "bufferViews"), &buffer_view_id).cloned();
        let attributes = js::get(inner.primitive.as_ref(), "attributes");
        let mut skip: Vec<&str> = Vec::new();
        for attribute in js::for_in(attributes) {
            let Some(draco_type) = draco_attribute_type(&attribute) else { continue };
            let accessor_id = js::get(attributes, &attribute).cloned().unwrap_or(Value::Null);
            let accessor = js::object(js::index(js::get(Some(gltf), "accessors"), &accessor_id), "componentType")
                .map_err(type_error)?;
            let float = js::get(Some(accessor), "componentType").and_then(Value::as_f64)
                == Some(ComponentDatatype::Float as u32 as f64);
            if float && !skip.contains(&draco_type) {
                skip.push(draco_type);
            }
        }
        let mut options = json!({
            "array": bytes.bytes(),
            "dequantizeInShader": true,
            "attributesToSkipTransform": skip,
        });
        if let Some(view) = buffer_view {
            options["bufferView"] = view;
        }
        if let Some(compressed) = js::get(Some(draco), "attributes") {
            options["compressedAttributes"] = compressed.clone();
        }
        Ok(options)
    }
}

impl ResourceLoader for GltfDracoLoader {
    fn cache_key(&self) -> Option<&str> {
        self.key.as_deref()
    }

    fn state(&self) -> ResourceLoaderState {
        self.inner.borrow().state
    }

    fn is_destroyed(&self) -> bool {
        self.inner.borrow().destroyed
    }

    fn unload(&self) {
        self.inner.borrow_mut().unload();
    }

    fn destroy(&self) {
        let mut inner = self.inner.borrow_mut();
        inner.unload();
        inner.decode = None;
        inner.destroyed = true;
    }

    fn load(&self) -> ResourceLoadFuture {
        let mut inner = self.inner.borrow_mut();
        if let Some(promise) = &inner.promise {
            return promise.clone();
        }
        inner.state = ResourceLoaderState::Loading;
        // Upstream's async function runs synchronously up to its first await:
        // it gets the buffer view loader and starts loading it here.
        let factory = inner.get_buffer_view_loader.take();
        let started = factory
            .ok_or_else(|| RuntimeError::new("Buffer view loader was already requested"))
            .and_then(|get| get())
            .map(|view| {
                let request = view.load();
                // The pending load must not keep the lease alive after unload.
                let weak_view = Rc::downgrade(&view);
                inner.buffer_view_loader = Some(view);
                (weak_view, request)
            });
        let weak = Rc::downgrade(&self.inner);
        let promise = match started {
            Err(error) => {
                let error = inner.fail(&error);
                futures_util::future::ready(Err(error)).boxed_local().shared()
            }
            Ok((view, request)) => async move {
                let result = request.await;
                let Some(inner) = weak.upgrade() else { return Ok(false) };
                let mut inner = inner.borrow_mut();
                if inner.destroyed {
                    return Ok(false);
                }
                match result {
                    Ok(_) => {
                        inner.buffer_view_typed_array = view.upgrade().and_then(|view| view.typed_array());
                        inner.state = ResourceLoaderState::Processing;
                        Ok(true)
                    }
                    Err(error) => Err(inner.fail(&error)),
                }
            }
            .boxed_local()
            .shared(),
        };
        inner.promise = Some(promise.clone());
        promise
    }

    fn process(&self, _frame: &FrameState) -> Result<bool, RuntimeError> {
        let mut inner = self.inner.borrow_mut();
        if let Some(task) = inner.decode.as_mut() {
            match Pin::new(task).poll(&mut Context::from_waker(Waker::noop())) {
                Poll::Ready(Ok(results)) => {
                    inner.decode = None;
                    // Unload everything except the decoded data.
                    inner.unload();
                    inner.decoded_data = Some(DecodedDraco {
                        indices: results.get("indexArray").cloned().unwrap_or(Value::Null),
                        vertex_attributes: results.get("attributeData").cloned().unwrap_or(Value::Null),
                    });
                    inner.state = ResourceLoaderState::Ready;
                }
                Poll::Ready(Err(error)) => {
                    inner.decode = None;
                    inner.draco_error = Some(error);
                }
                Poll::Pending => {}
            }
        }
        if inner.state == ResourceLoaderState::Ready {
            return Ok(true);
        }
        if inner.state != ResourceLoaderState::Processing {
            return Ok(false);
        }
        if let Some(error) = inner.draco_error.clone() {
            return Err(inner.fail(&error));
        }
        let Some(bytes) = inner.buffer_view_typed_array.clone() else {
            return Ok(false);
        };
        if inner.decode_started {
            return Ok(false);
        }
        let options = Self::decode_options(&inner, &bytes)?;
        match self.decoder.decode_buffer_view(options)? {
            // Cannot schedule the task this frame.
            None => Ok(false),
            Some(task) => {
                inner.decode = Some(task);
                inner.decode_started = true;
                Ok(false)
            }
        }
    }
}
