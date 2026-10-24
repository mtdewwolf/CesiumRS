//! Parse, upgrade and normalize glTF JSON/GLB before model resource loading.
//! Embedded buffers remain owned alongside the cleaned JSON; a resource cache
//! can share the loader itself without exposing pipeline source markers.
use super::{
    buffer_loader::BufferLoader,
    gltf_pipeline::{
        self as pipeline,
        for_each::{self, Collection},
        BufferSource, Gltf, PipelineError,
    },
    resource_loader::ManagedLoader,
    ResourceLoadFuture, ResourceLoader, ResourceLoaderState,
};
use crate::core::{
    get_string_from_typed_array::get_json_from_typed_array, resource::DerivedResourceOptions, Resource, ResourceClient,
    RuntimeError,
};
use futures_util::{future::join_all, FutureExt};
use serde_json::Value;
use std::{cell::RefCell, collections::BTreeMap, rc::Rc, sync::Arc};
#[derive(Clone, Debug)]
pub enum GltfJsonInput {
    Document(Gltf),
    Bytes(BufferSource),
}
#[derive(Clone, Debug)]
pub struct LoadedGltfJson {
    pub json: Value,
    pub embedded_buffers: BTreeMap<usize, BufferSource>,
    /// Includes external buffers loaded and possibly rewritten during legacy
    /// conversion. Buffer views must consume these bytes rather than refetch.
    pub buffer_sources: BTreeMap<usize, BufferSource>,
}
pub struct GltfJsonLoader {
    data: ManagedLoader<Arc<LoadedGltfJson>>,
    resource: Resource,
    base: Resource,
    client: ResourceClient,
    input: Option<GltfJsonInput>,
    cache: Option<super::WeakResourceCache>,
    buffer_leases: Rc<RefCell<Vec<super::CacheLease<BufferLoader>>>>,
}
impl GltfJsonLoader {
    pub fn new(
        resource: Resource,
        base: Resource,
        client: ResourceClient,
        input: Option<GltfJsonInput>,
        cache_key: Option<String>,
    ) -> Self {
        Self {
            data: ManagedLoader::new(cache_key, None),
            resource,
            base,
            client,
            input,
            cache: None,
            buffer_leases: Rc::new(RefCell::new(Vec::new())),
        }
    }
    pub fn with_resource_cache(mut self, cache: super::WeakResourceCache) -> Self {
        self.cache = Some(cache);
        self
    }
    pub fn gltf(&self) -> Option<Arc<LoadedGltfJson>> {
        self.data.value()
    }
}
fn pipeline_error(error: PipelineError) -> RuntimeError {
    let (PipelineError::Runtime(message)
    | PipelineError::Developer(message)
    | PipelineError::Type(message)
    | PipelineError::Range(message)
    | PipelineError::Syntax(message)) = error;
    RuntimeError::new(message)
}
fn derive(base: &Resource, uri: &str) -> Result<Resource, RuntimeError> {
    base.get_derived_resource(DerivedResourceOptions { url: Some(uri.into()), ..Default::default() })
        .ok_or_else(|| RuntimeError::new("Invalid glTF buffer URI"))
}
enum SourceBuffer {
    Owned(BufferLoader),
    Cached(super::CacheLease<BufferLoader>),
}
impl std::ops::Deref for SourceBuffer {
    type Target = BufferLoader;
    fn deref(&self) -> &BufferLoader {
        match self {
            Self::Owned(loader) => loader,
            Self::Cached(lease) => lease,
        }
    }
}
async fn prepare(
    mut gltf: Gltf,
    parent: Resource,
    base: Resource,
    client: ResourceClient,
    cache: Option<super::WeakResourceCache>,
) -> Result<(LoadedGltfJson, Vec<super::CacheLease<BufferLoader>>), RuntimeError> {
    let mut leases = Vec::new();
    pipeline::add_pipeline_extras(&mut gltf.json).map_err(pipeline_error)?;
    let buffers = Collection::TopLevel("buffers");
    let mut embedded = Vec::new();
    for id in for_each::ids(&gltf.json, buffers) {
        let buffer = for_each::get(&gltf.json, buffers, &id).expect("enumerated buffer");
        if gltf.buffer_source(&id.value()).is_none() {
            if let Some(uri) = buffer["uri"].as_str().filter(|uri| uri.starts_with("data:")) {
                let resource = Resource::new(uri).ok_or_else(|| RuntimeError::new("Invalid data URI"))?;
                let request = client
                    .fetch_array_buffer(&resource)
                    .map(|request| super::resource_loader::guard_request(&resource, request))
                    .ok_or_else(|| RuntimeError::new("Data URI request was throttled"))?;
                embedded.push((id, request));
            }
        }
    }
    let results = join_all(embedded.into_iter().map(|(id, request)| async move {
        request.await.map(|bytes| (id, bytes)).map_err(|e| RuntimeError::new(e.to_string()))
    }))
    .await;
    for result in results {
        let (id, bytes) = result?;
        let marker = gltf.add_source(BufferSource::new(bytes));
        let buffer = for_each::get_mut(&mut gltf.json, buffers, &id).expect("enumerated buffer");
        buffer["extras"]["_pipeline"]["source"] = marker;
        buffer.as_object_mut().ok_or_else(|| RuntimeError::new("Invalid glTF buffer"))?.remove("uri");
    }
    if gltf.json.pointer("/asset/version").and_then(Value::as_str) != Some("2.0")
        || pipeline::uses_extension(&gltf.json, "KHR_techniques_webgl")
        || pipeline::uses_extension(&gltf.json, "KHR_materials_common")
    {
        let mut external = Vec::new();
        for id in for_each::ids(&gltf.json, buffers) {
            let buffer = for_each::get(&gltf.json, buffers, &id).expect("enumerated buffer");
            if gltf.buffer_source(&id.value()).is_none() {
                if let Some(uri) = buffer["uri"].as_str() {
                    let resource = derive(&base, uri)?;
                    let loader = if let Some(cache) = cache.as_ref().and_then(|cache| cache.upgrade()) {
                        SourceBuffer::Cached(cache.external_buffer_loader(resource)?)
                    } else {
                        SourceBuffer::Owned(BufferLoader::new(None, Some(resource), Some(client.clone()), None)?)
                    };
                    let request = loader.load();
                    external.push((id, loader, request));
                }
            }
        }
        let results = join_all(external.into_iter().map(|(id, loader, request)| async move {
            request.await?;
            loader
                .typed_array()
                .map(|bytes| (id, bytes, loader))
                .ok_or_else(|| RuntimeError::new("Buffer loader was destroyed"))
        }))
        .await;
        for result in results {
            let (id, bytes, loader) = result?;
            if let SourceBuffer::Cached(lease) = loader {
                leases.push(lease);
            }
            let marker = gltf.add_source(bytes);
            for_each::get_mut(&mut gltf.json, buffers, &id).expect("enumerated buffer")["extras"]["_pipeline"]
                ["source"] = marker;
        }
        pipeline::update_version(&mut gltf, &pipeline::UpdateVersionOptions::default()).map_err(pipeline_error)?;
    }
    pipeline::add_defaults(&mut gltf.json).map_err(pipeline_error)?;
    let mut embedded_buffers = BTreeMap::new();
    let mut buffer_sources = BTreeMap::new();
    for id in for_each::ids(&gltf.json, buffers) {
        let buffer = for_each::get(&gltf.json, buffers, &id).expect("enumerated buffer");
        if let Some(source) = gltf.buffer_source(&id.value()) {
            let index = id.key().parse().map_err(|_| RuntimeError::new("Invalid upgraded glTF buffer ID"))?;
            buffer_sources.insert(index, source.clone());
            if let Some(uri) = buffer["uri"].as_str() {
                let key = super::ResourceCacheKey::external_buffer(&derive(&base, uri)?);
                if let Some(lease) = leases.iter().find(|lease| lease.cache_key() == Some(&key)) {
                    lease.replace_typed_array(source.clone());
                }
            }
            if buffer.get("uri").is_none_or(Value::is_null) {
                embedded_buffers.insert(index, source.clone());
            }
        }
    }
    pipeline::remove_pipeline_extras(&mut gltf.json).map_err(pipeline_error)?;
    let version = gltf.json.pointer("/asset/version").and_then(Value::as_str).unwrap_or("undefined");
    if !matches!(version, "1.0" | "2.0") {
        return Err(RuntimeError::new(format!("Unsupported glTF version: {version}")));
    }
    if let Some(required) = gltf.json["extensionsRequired"].as_array() {
        for extension in required {
            let extension = extension.as_str().unwrap_or("undefined");
            if !supported_extension(extension) {
                return Err(RuntimeError::new(format!("Unsupported glTF Extension: {extension}")));
            }
        }
    }
    if let Some(cache) = cache.as_ref().and_then(|cache| cache.upgrade()) {
        for (id, source) in &embedded_buffers {
            let lease = cache.embedded_buffer_loader(&parent, *id, Some(source.clone()))?;
            lease.load().await?;
            leases.push(lease);
        }
    }
    Ok((LoadedGltfJson { json: gltf.json, embedded_buffers, buffer_sources }, leases))
}
// ModelUtility's pinned JSON compatibility check. Rendering support is supplied
// by the later Model pipeline; recognizing an extension does not implement it.
fn supported_extension(extension: &str) -> bool {
    matches!(
        extension,
        "AGI_articulations"
            | "CESIUM_primitive_outline"
            | "CESIUM_RTC"
            | "EXT_feature_metadata"
            | "EXT_implicit_cylinder_region"
            | "EXT_implicit_ellipsoid_region"
            | "EXT_instance_features"
            | "EXT_mesh_features"
            | "EXT_mesh_gpu_instancing"
            | "EXT_meshopt_compression"
            | "EXT_primitive_voxels"
            | "EXT_structural_metadata"
            | "EXT_texture_webp"
            | "KHR_blend"
            | "KHR_draco_mesh_compression"
            | "KHR_implicit_shapes"
            | "KHR_materials_common"
            | "KHR_materials_pbrSpecularGlossiness"
            | "KHR_materials_specular"
            | "KHR_materials_anisotropy"
            | "KHR_materials_clearcoat"
            | "KHR_materials_unlit"
            | "KHR_mesh_quantization"
            | "KHR_techniques_webgl"
            | "KHR_texture_basisu"
            | "KHR_texture_transform"
            | "KHR_gaussian_splatting"
            | "KHR_gaussian_splatting_compression_spz_2"
            | "WEB3D_quantized_attributes"
    )
}
impl ResourceLoader for GltfJsonLoader {
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
        self.buffer_leases.borrow_mut().clear();
        self.data.unload();
    }
    fn destroy(&self) {
        self.unload();
        self.data.destroy();
    }
    fn load(&self) -> ResourceLoadFuture {
        let (resource, base, client, input) =
            (self.resource.clone(), self.base.clone(), self.client.clone(), self.input.clone());
        let cache = self.cache.clone();
        let buffer_leases = Rc::downgrade(&self.buffer_leases);
        let message = format!("Failed to load glTF: {}", resource.url());
        self.data.load(message, move || {
            async move {
                let input = if let Some(input) = input {
                    input
                } else {
                    let request = client
                        .fetch_array_buffer(&resource)
                        .map(|request| super::resource_loader::guard_request(&resource, request))
                        .ok_or_else(|| RuntimeError::new("glTF request was throttled"))?;
                    GltfJsonInput::Bytes(BufferSource::new(
                        request.await.map_err(|e| RuntimeError::new(e.to_string()))?,
                    ))
                };
                let gltf = match input {
                    GltfJsonInput::Document(gltf) => gltf,
                    GltfJsonInput::Bytes(bytes) => {
                        if bytes.bytes().starts_with(b"glTF") {
                            pipeline::parse_glb(&bytes)
                                .map_err(pipeline_error)?
                                .ok_or_else(|| RuntimeError::new("Binary glTF has no JSON chunk"))?
                        } else {
                            Gltf::new(get_json_from_typed_array(bytes.bytes(), 0, None)?)
                        }
                    }
                };
                let (gltf, leases) = prepare(gltf, resource, base, client, cache).await?;
                if let Some(owner) = buffer_leases.upgrade() {
                    *owner.borrow_mut() = leases;
                }
                Ok(Arc::new(gltf))
            }
            .boxed_local()
        })
    }
}
