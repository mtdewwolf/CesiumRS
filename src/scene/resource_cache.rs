//! Per-owner resource pool. CacheLease is the native reference-counted handle;
//! dropping its last lease removes the entry and destroys its loader/resources.
use super::{gltf_pipeline::BufferSource, *};
use crate::core::{Resource, ResourceClient, RuntimeError};
use serde_json::Value;
use std::{
    any::Any,
    cell::{Cell, RefCell},
    collections::HashMap,
    ops::Deref,
    rc::{Rc, Weak},
    sync::Arc,
};
type Result<T> = std::result::Result<T, RuntimeError>;
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum CacheResourceKind {
    Other,
    VertexBuffer,
    IndexBuffer,
    Draco,
    Texture,
    Image,
    BufferView,
    Buffer,
    Schema,
    GltfJson,
}
struct Entry {
    references: usize,
    generation: u64,
    kind: CacheResourceKind,
    loader: Rc<dyn ResourceLoader>,
    typed: Rc<dyn Any>,
}
struct Pool {
    entries: RefCell<HashMap<String, Entry>>,
    generation: Cell<u64>,
    statistics: Rc<RefCell<ResourceCacheStatistics>>,
    decoder: Rc<RefCell<crate::core::TaskProcessor<Value, Value>>>,
}
impl Drop for Pool {
    fn drop(&mut self) {
        let mut entries = std::mem::take(self.entries.get_mut()).into_values().collect::<Vec<_>>();
        entries.sort_by_key(|e| e.kind);
        for entry in entries {
            entry.loader.destroy();
        }
        self.statistics.borrow_mut().clear();
    }
}
#[derive(Clone)]
pub struct ResourceCache {
    inner: Rc<Pool>,
    client: ResourceClient,
}
#[derive(Clone)]
pub struct WeakResourceCache {
    inner: Weak<Pool>,
    client: ResourceClient,
}
impl WeakResourceCache {
    pub fn upgrade(&self) -> Option<ResourceCache> {
        self.inner.upgrade().map(|inner| ResourceCache { inner, client: self.client.clone() })
    }
}
pub struct CacheLease<T: ResourceLoader + 'static> {
    loader: Rc<T>,
    pool: Weak<Pool>,
    key: String,
    generation: u64,
}
impl<T: ResourceLoader + 'static> Deref for CacheLease<T> {
    type Target = T;
    fn deref(&self) -> &T {
        &self.loader
    }
}
impl<T: ResourceLoader + 'static> CacheLease<T> {
    /// Observes the loader without extending its cache lease. Keep a lease alive
    /// while using the resource; releasing the last lease destroys this handle too.
    pub fn loader(&self) -> Rc<T> {
        self.loader.clone()
    }
    pub fn ptr_eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.loader, &other.loader)
    }
}
impl<T: ResourceLoader + 'static> Clone for CacheLease<T> {
    fn clone(&self) -> Self {
        if let Some(pool) = self.pool.upgrade() {
            if let Some(entry) =
                pool.entries.borrow_mut().get_mut(&self.key).filter(|e| e.generation == self.generation)
            {
                entry.references += 1;
            }
        }
        Self {
            loader: self.loader.clone(),
            pool: self.pool.clone(),
            key: self.key.clone(),
            generation: self.generation,
        }
    }
}
impl<T: ResourceLoader + 'static> Drop for CacheLease<T> {
    fn drop(&mut self) {
        let Some(pool) = self.pool.upgrade() else {
            return;
        };
        let removed = {
            let mut entries = pool.entries.borrow_mut();
            let Some(entry) = entries.get_mut(&self.key).filter(|e| e.generation == self.generation) else {
                return;
            };
            entry.references -= 1;
            if entry.references == 0 {
                entries.remove(&self.key)
            } else {
                None
            }
        };
        if let Some(entry) = removed {
            pool.statistics.borrow_mut().remove_loader(&self.key);
            entry.loader.destroy();
        }
    }
}
impl ResourceCache {
    pub fn new(client: ResourceClient) -> Self {
        Self {
            inner: Rc::new(Pool {
                entries: RefCell::new(HashMap::new()),
                generation: Cell::new(0),
                statistics: Rc::new(RefCell::new(ResourceCacheStatistics::default())),
                decoder: Rc::new(RefCell::new(crate::core::TaskProcessor::new(Rc::new(DracoDecodeWorker), Some(5)))),
            }),
            client,
        }
    }
    pub(crate) fn client(&self) -> ResourceClient {
        self.client.clone()
    }
    pub fn downgrade(&self) -> WeakResourceCache {
        WeakResourceCache { inner: Rc::downgrade(&self.inner), client: self.client.clone() }
    }
    pub fn statistics(&self) -> Rc<RefCell<ResourceCacheStatistics>> {
        self.inner.statistics.clone()
    }
    pub fn reference_count(&self, key: &str) -> Option<usize> {
        self.inner.entries.borrow().get(key).map(|e| e.references)
    }
    pub fn len(&self) -> usize {
        self.inner.entries.borrow().len()
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
    pub fn get<T: ResourceLoader + 'static>(&self, key: &str) -> Result<Option<CacheLease<T>>> {
        let mut entries = self.inner.entries.borrow_mut();
        let Some(entry) = entries.get_mut(key) else {
            return Ok(None);
        };
        let loader = entry
            .typed
            .clone()
            .downcast::<T>()
            .map_err(|_| RuntimeError::new(format!("Cache key has a different loader type: {key}")))?;
        entry.references += 1;
        Ok(Some(CacheLease { loader, pool: Rc::downgrade(&self.inner), key: key.into(), generation: entry.generation }))
    }
    pub fn add<T: ResourceLoader + 'static>(&self, loader: T, kind: CacheResourceKind) -> Result<CacheLease<T>> {
        let key =
            loader.cache_key().ok_or_else(|| RuntimeError::new("Resource loader must have a cache key"))?.to_string();
        let mut entries = self.inner.entries.borrow_mut();
        if entries.contains_key(&key) {
            return Err(RuntimeError::new(format!("Resource with this cacheKey is already in the cache: {key}")));
        }
        let generation = self.inner.generation.get().wrapping_add(1);
        self.inner.generation.set(generation);
        let loader = Rc::new(loader);
        entries.insert(
            key.clone(),
            Entry { references: 1, generation, kind, loader: loader.clone(), typed: loader.clone() },
        );
        Ok(CacheLease { loader, pool: Rc::downgrade(&self.inner), key, generation })
    }
    pub fn get_or_insert<T: ResourceLoader + 'static>(
        &self,
        key: &str,
        kind: CacheResourceKind,
        create: impl FnOnce() -> Result<T>,
    ) -> Result<CacheLease<T>> {
        if let Some(loader) = self.get(key)? {
            return Ok(loader);
        }
        let loader = create()?;
        if loader.cache_key() != Some(key) {
            return Err(RuntimeError::new("Factory returned a different resource cache key"));
        }
        self.add(loader, kind)
    }
    pub fn clear_for_specs(&self) {
        let mut keys = self.inner.entries.borrow().iter().map(|(key, e)| (e.kind, key.clone())).collect::<Vec<_>>();
        keys.sort();
        for (_, key) in keys {
            let entry = self.inner.entries.borrow_mut().remove(&key);
            if let Some(entry) = entry {
                entry.loader.destroy();
            }
        }
        self.inner.statistics.borrow_mut().clear();
    }
    pub fn schema_loader(
        &self,
        schema: Option<Value>,
        resource: Option<Resource>,
    ) -> Result<CacheLease<MetadataSchemaLoader>> {
        let key = ResourceCacheKey::schema(schema.as_ref(), resource.as_ref())?;
        self.get_or_insert(&key, CacheResourceKind::Schema, || {
            MetadataSchemaLoader::new(schema, resource, Some(self.client.clone()), Some(key.clone()))
        })
    }
    pub fn embedded_buffer_loader(
        &self,
        parent: &Resource,
        id: usize,
        bytes: Option<BufferSource>,
    ) -> Result<CacheLease<BufferLoader>> {
        let key = ResourceCacheKey::embedded_buffer(parent, id);
        self.get_or_insert(&key, CacheResourceKind::Buffer, || BufferLoader::new(bytes, None, None, Some(key.clone())))
    }
    pub fn external_buffer_loader(&self, resource: Resource) -> Result<CacheLease<BufferLoader>> {
        let key = ResourceCacheKey::external_buffer(&resource);
        self.get_or_insert(&key, CacheResourceKind::Buffer, || {
            BufferLoader::new(None, Some(resource), Some(self.client.clone()), Some(key.clone()))
        })
    }
    pub fn gltf_json_loader(
        &self,
        resource: Resource,
        base: Resource,
        input: Option<GltfJsonInput>,
    ) -> Result<CacheLease<GltfJsonLoader>> {
        let key = ResourceCacheKey::gltf(&resource);
        self.get_or_insert(&key, CacheResourceKind::GltfJson, || {
            Ok(GltfJsonLoader::new(resource, base, self.client.clone(), input, Some(key.clone()))
                .with_resource_cache(self.downgrade()))
        })
    }
    pub fn draco_loader(
        &self,
        gltf: Arc<LoadedGltfJson>,
        primitive: &Value,
        extension: &Value,
        parent: &Resource,
        base: &Resource,
    ) -> Result<CacheLease<GltfDracoLoader>> {
        let key = ResourceCacheKey::draco(
            GltfKeyContext { gltf: &gltf.json, gltf_resource: parent, base_resource: base },
            extension,
        )?;
        self.get_or_insert(&key, CacheResourceKind::Draco, || {
            let id = extension["bufferView"].as_u64().ok_or_else(|| RuntimeError::new("Invalid Draco buffer view"))?
                as usize;
            let view = self.buffer_view_loader(gltf.clone(), id, parent, base)?;
            GltfDracoLoader::new(&gltf.json, primitive, extension, view, self.inner.decoder.clone(), Some(key.clone()))
        })
    }
    pub fn set_draco_decoder(&self, decoder: crate::core::TaskProcessor<Value, Value>) -> Result<()> {
        if self.inner.decoder.borrow().active_tasks() > 0 {
            return Err(RuntimeError::new("Cannot replace an active Draco decoder"));
        }
        *self.inner.decoder.borrow_mut() = decoder;
        Ok(())
    }
    pub fn buffer_view_loader(
        &self,
        gltf: Arc<LoadedGltfJson>,
        id: usize,
        parent: &Resource,
        base: &Resource,
    ) -> Result<CacheLease<GltfBufferViewLoader>> {
        let key = ResourceCacheKey::buffer_view(
            GltfKeyContext { gltf: &gltf.json, gltf_resource: parent, base_resource: base },
            id,
        )?;
        self.get_or_insert(&key, CacheResourceKind::BufferView, || {
            let view = gltf.json["bufferViews"].get(id).ok_or_else(|| RuntimeError::new("Invalid buffer view ID"))?;
            let layout = view.pointer("/extensions/EXT_meshopt_compression").filter(|v| !v.is_null()).unwrap_or(view);
            let buffer_id = layout["buffer"].as_u64().ok_or_else(|| RuntimeError::new("Invalid buffer ID"))? as usize;
            let buffer = gltf.json["buffers"].get(buffer_id).ok_or_else(|| RuntimeError::new("Invalid buffer ID"))?;
            let child = if let Some(uri) = buffer["uri"].as_str() {
                let resource = base
                    .get_derived_resource(crate::core::resource::DerivedResourceOptions {
                        url: Some(uri.into()),
                        ..Default::default()
                    })
                    .ok_or_else(|| RuntimeError::new("Invalid buffer URI"))?;
                if let Some(bytes) = gltf.buffer_sources.get(&buffer_id) {
                    let buffer_key = ResourceCacheKey::external_buffer(&resource);
                    self.get_or_insert(&buffer_key, CacheResourceKind::Buffer, || {
                        BufferLoader::new(Some(bytes.clone()), None, None, Some(buffer_key.clone()))
                    })?
                } else {
                    self.external_buffer_loader(resource)?
                }
            } else {
                self.embedded_buffer_loader(parent, buffer_id, gltf.buffer_sources.get(&buffer_id).cloned())?
            };
            GltfBufferViewLoader::from_cached_buffer(view, child, Some(key.clone()))
        })
    }
}
