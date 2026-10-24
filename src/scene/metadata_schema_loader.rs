//! Cesium MetadataSchemaLoader with shared requests and destruction cancellation.
use super::{resource_loader::ManagedLoader, MetadataSchema, ResourceLoadFuture, ResourceLoader, ResourceLoaderState};
use crate::core::{Resource, ResourceClient, RuntimeError};
use futures_util::FutureExt;
use serde_json::Value;
use std::sync::Arc;
pub struct MetadataSchemaLoader {
    data: ManagedLoader<Arc<MetadataSchema>>,
    external: Option<(Resource, ResourceClient)>,
}
impl MetadataSchemaLoader {
    pub fn new(
        schema: Option<Value>,
        resource: Option<Resource>,
        client: Option<ResourceClient>,
        cache_key: Option<String>,
    ) -> Result<Self, RuntimeError> {
        if schema.is_some() == resource.is_some() {
            return Err(RuntimeError::new("One of options.schema and options.resource must be defined."));
        }
        let schema = schema
            .map(|schema| {
                MetadataSchema::from_json(&schema).map(Arc::new).map_err(|e| RuntimeError::new(e.to_string()))
            })
            .transpose()?;
        let external = resource
            .map(|resource| {
                client
                    .map(|client| (resource, client))
                    .ok_or_else(|| RuntimeError::new("External schema loading requires a ResourceClient"))
            })
            .transpose()?;
        Ok(Self { data: ManagedLoader::new(cache_key, schema), external })
    }
    pub fn schema(&self) -> Option<Arc<MetadataSchema>> {
        self.data.value()
    }
}
impl ResourceLoader for MetadataSchemaLoader {
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
            external.as_ref().map_or_else(String::new, |(r, _)| format!("Failed to load schema: {}", r.url()));
        self.data.load(message, move || {
            let request = external.ok_or_else(|| RuntimeError::new("Embedded schema was unloaded")).and_then(
                |(resource, client)| {
                    client
                        .fetch_json(&resource)
                        .map(|request| super::resource_loader::guard_request(&resource, request))
                        .ok_or_else(|| RuntimeError::new("Schema request was throttled"))
                },
            );
            async move {
                let json = request?.await.map_err(|e| RuntimeError::new(e.to_string()))?;
                MetadataSchema::from_json(&json).map(Arc::new).map_err(|e| RuntimeError::new(e.to_string()))
            }
            .boxed_local()
        })
    }
}
