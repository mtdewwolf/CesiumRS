//! Cesium BufferLoader: embedded views or ResourceClient-backed external bytes.
use super::{
    gltf_pipeline::BufferSource, resource_loader::ManagedLoader, ResourceLoadFuture, ResourceLoader,
    ResourceLoaderState,
};
use crate::core::{Resource, ResourceClient, RuntimeError};
use futures_util::FutureExt;
pub struct BufferLoader {
    data: ManagedLoader<BufferSource>,
    external: Option<(Resource, ResourceClient)>,
}
impl BufferLoader {
    pub fn new(
        typed_array: Option<BufferSource>,
        resource: Option<Resource>,
        client: Option<ResourceClient>,
        cache_key: Option<String>,
    ) -> Result<Self, RuntimeError> {
        if typed_array.is_some() == resource.is_some() {
            return Err(RuntimeError::new("One of options.typedArray and options.resource must be defined."));
        }
        let external = resource
            .map(|resource| {
                client
                    .map(|client| (resource, client))
                    .ok_or_else(|| RuntimeError::new("External buffer loading requires a ResourceClient"))
            })
            .transpose()?;
        Ok(Self { data: ManagedLoader::new(cache_key, typed_array), external })
    }
    pub fn typed_array(&self) -> Option<BufferSource> {
        self.data.value()
    }
    // Legacy upgrades rewrite shared JS ArrayBuffers in place. Rust's pipeline
    // uses copy-on-write sources, so publish the upgraded view to the cache.
    pub(crate) fn replace_typed_array(&self, source: BufferSource) {
        self.data.replace_value(source);
    }
}
impl ResourceLoader for BufferLoader {
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
        self.data.unload();
    }
    fn destroy(&self) {
        self.data.destroy();
    }
    fn load(&self) -> ResourceLoadFuture {
        let external = self.external.clone();
        let message =
            external.as_ref().map_or_else(String::new, |(r, _)| format!("Failed to load external buffer: {}", r.url()));
        self.data.load(message, move || {
            let request = external.ok_or_else(|| RuntimeError::new("Embedded buffer was unloaded")).and_then(
                |(resource, client)| {
                    client
                        .fetch_array_buffer(&resource)
                        .map(|request| super::resource_loader::guard_request(&resource, request))
                        .ok_or_else(|| RuntimeError::new("Buffer request was throttled"))
                },
            );
            async move { request?.await.map(BufferSource::new).map_err(|error| RuntimeError::new(error.to_string())) }
                .boxed_local()
        })
    }
}
