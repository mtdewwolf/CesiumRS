//! `KHR_gaussian_splatting_compression_spz_2` buffers, decoded with the ported
//! [`spz`](super::spz) decoder.
use super::{
    gltf_pipeline::BufferSource,
    resource_loader::{loader_error, ManagedLoader},
    spz::{load_spz, GaussianCloud},
    CacheLease, CacheResourceKind, FrameState, GltfBufferViewLoader, GltfKeyContext, GltfResource, ResourceCache,
    ResourceCacheKey, ResourceLoadFuture, ResourceLoader, ResourceLoaderState, WeakResourceCache,
};
use crate::core::RuntimeError;
use futures_util::FutureExt;
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    sync::Arc,
};

const FAILED: &str = "Failed to load SPZ";

pub struct GltfSpzLoader {
    data: ManagedLoader<BufferSource>,
    cache: WeakResourceCache,
    gltf: RefCell<Option<GltfResource>>,
    view: Rc<RefCell<Option<CacheLease<GltfBufferViewLoader>>>>,
    decoding: Cell<bool>,
    /// The decode result and the frame that started it.
    pending: RefCell<Option<(u32, Result<GaussianCloud, RuntimeError>)>>,
    decoded: RefCell<Option<Arc<GaussianCloud>>>,
}

impl GltfSpzLoader {
    /// SPZ data is always read from buffer view 0, whatever the primitive and
    /// extension name. The buffer view is requested from `cache` when loading.
    pub fn new(cache: WeakResourceCache, gltf: GltfResource, key: Option<String>) -> Self {
        Self {
            data: ManagedLoader::new(key, None),
            cache,
            gltf: RefCell::new(Some(gltf)),
            view: Rc::new(RefCell::new(None)),
            decoding: Cell::new(false),
            pending: RefCell::new(None),
            decoded: RefCell::new(None),
        }
    }

    /// `decodedData.gcloud`, once the loader is ready.
    pub fn decoded_data(&self) -> Option<Arc<GaussianCloud>> {
        self.decoded.borrow().clone()
    }

    fn fail(&self, error: &RuntimeError) -> RuntimeError {
        self.unload();
        self.data.set_state(ResourceLoaderState::Failed);
        loader_error(FAILED, Some(error))
    }
}

impl ResourceLoader for GltfSpzLoader {
    fn cache_key(&self) -> Option<&str> {
        self.data.key.as_deref()
    }
    fn state(&self) -> ResourceLoaderState {
        self.data.state()
    }
    fn is_destroyed(&self) -> bool {
        self.data.is_destroyed()
    }
    fn load(&self) -> ResourceLoadFuture {
        let view = self.gltf.borrow().as_ref().map(|gltf| {
            let cache = self.cache.upgrade().ok_or_else(|| RuntimeError::new("Resource cache was destroyed"))?;
            cache.buffer_view_loader(gltf.document.clone(), 0, &gltf.resource, &gltf.base)
        });
        let owner = self.view.clone();
        self.data.load_with_state(FAILED.into(), ResourceLoaderState::Processing, move || {
            async move {
                let result = async {
                    let view = view.ok_or_else(|| RuntimeError::new("SPZ loader was unloaded"))??;
                    let loader = view.loader();
                    *owner.borrow_mut() = Some(view);
                    loader.load().await?;
                    loader.typed_array().ok_or_else(|| RuntimeError::new("SPZ buffer view was unloaded"))
                }
                .await;
                if result.is_err() {
                    owner.borrow_mut().take();
                }
                result
            }
            .boxed_local()
        })
    }
    /// Starts decoding on the first call after loading and returns false.
    /// Upstream's decode promise settles before the next frame, so the result
    /// applies at the first call with a later frame number, which returns
    /// true or the decode error. Calls in the starting frame return false.
    fn process(&self, frame: &FrameState) -> Result<bool, RuntimeError> {
        if self.is_destroyed() {
            return Err(RuntimeError::new("SPZ loader was destroyed"));
        }
        match self.state() {
            ResourceLoaderState::Ready => return Ok(true),
            ResourceLoaderState::Processing => {}
            _ => return Ok(false),
        }
        let settled = self.pending.borrow().as_ref().is_some_and(|(started, _)| *started != frame.frame_number);
        if settled {
            let (_, result) = self.pending.borrow_mut().take().expect("settled decode");
            return match result {
                Ok(cloud) => {
                    self.unload();
                    *self.decoded.borrow_mut() = Some(Arc::new(cloud));
                    self.data.set_state(ResourceLoaderState::Ready);
                    Ok(true)
                }
                Err(error) => Err(self.fail(&error)),
            };
        }
        let Some(bytes) = self.data.value() else {
            return Ok(false);
        };
        if self.decoding.replace(true) {
            return Ok(false);
        }
        *self.pending.borrow_mut() = Some((frame.frame_number, load_spz(bytes.bytes())));
        Ok(false)
    }
    fn unload(&self) {
        self.pending.borrow_mut().take();
        self.view.borrow_mut().take();
        self.data.unload();
        self.decoded.borrow_mut().take();
        self.gltf.borrow_mut().take();
    }
    fn destroy(&self) {
        self.unload();
        self.data.destroy();
    }
}

impl ResourceCache {
    /// `ResourceCache.getSpzLoader`.
    pub fn spz_loader(&self, request: &GltfResource) -> Result<CacheLease<GltfSpzLoader>, RuntimeError> {
        let key = ResourceCacheKey::spz(GltfKeyContext {
            gltf: &request.document.json,
            gltf_resource: &request.resource,
            base_resource: &request.base,
        })?;
        self.get_or_insert(&key, CacheResourceKind::Spz, || {
            Ok(GltfSpzLoader::new(self.downgrade(), request.clone(), Some(key.clone())))
        })
    }
}
