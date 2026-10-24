//! CesiumJS `GltfMeshPrimitiveGpmLoader`: loads the per-point error (PPE)
//! textures of a mesh primitive's `NGA_gpm_local` extension and exposes them
//! as [`MeshPrimitiveGpmLocal`] and as [`StructuralMetadata`] property
//! textures, one class per texture.
//!
//! The metadata schemas are cached for the thread by the PPE textures'
//! class definitions, as CesiumJS caches them for the page; a new schema's
//! id counts the schemas already cached.

use super::{MeshPrimitiveGpmLocal, PpeMetadata, PpeTexture};
use crate::core::math::js_number_to_string;
use crate::core::RuntimeError;
use crate::renderer::{Context, Gl};
use crate::scene::gltf_pipeline::js;
use crate::scene::gltf_structural_metadata_loader::{IdSet, TextureLeases};
use crate::scene::resource_loader::loader_error;
use crate::scene::*;
use futures_util::future::FutureExt;
use serde_json::{json, Map, Value};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::{Rc, Weak};
use std::sync::Arc;

type Result<T> = std::result::Result<T, RuntimeError>;

thread_local! {
    static PPE_TEXTURES_METADATA_SCHEMA_CACHE: RefCell<HashMap<String, Arc<MetadataSchema>>> =
        RefCell::new(HashMap::new());
}

/// `GltfMeshPrimitiveGpmLoader.ppeTexturesMetadataSchemaCache.size`.
pub fn ppe_textures_metadata_schema_cache_len() -> usize {
    PPE_TEXTURES_METADATA_SCHEMA_CACHE.with(|c| c.borrow().len())
}

/// Empties the schema cache, so the next schema is `PPE_TEXTURE_SCHEMA_0`.
pub fn clear_ppe_textures_metadata_schema_cache() {
    PPE_TEXTURES_METADATA_SCHEMA_CACHE.with(|c| c.borrow_mut().clear());
}

/// `JSON.stringify`, with JavaScript's number formatting.
fn stringify(value: &Value) -> String {
    match value {
        Value::Number(n) => {
            let x = n.as_f64().unwrap_or(f64::NAN);
            if x.is_finite() {
                js_number_to_string(x)
            } else {
                "null".into()
            }
        }
        Value::Array(items) => format!("[{}]", items.iter().map(stringify).collect::<Vec<_>>().join(",")),
        Value::Object(map) => format!(
            "{{{}}}",
            map.iter()
                .map(|(k, v)| format!("{}:{}", Value::String(k.clone()), stringify(v)))
                .collect::<Vec<_>>()
                .join(",")
        ),
        other => other.to_string(),
    }
}

fn number(x: f64) -> Value {
    serde_json::Number::from_f64(x).map_or(Value::Null, Value::Number)
}

/// The metadata class of PPE texture `index`: one normalized UINT8 property
/// named after the texture's source, with its offset and its scale times 255.
pub fn create_ppe_texture_class_json(ppe_texture: &PpeTexture, index: usize) -> Value {
    let traits = &ppe_texture.traits;
    let mut property = Map::new();
    property.insert("name".into(), json!("PPE"));
    property.insert("type".into(), json!("SCALAR"));
    property.insert("componentType".into(), json!("UINT8"));
    property.insert("normalized".into(), json!(true));
    property.insert("offset".into(), number(ppe_texture.offset.unwrap_or(0.0)));
    property.insert("scale".into(), number(ppe_texture.scale.unwrap_or(1.0) * 255.0));
    if let Some(min) = traits.min {
        property.insert("min".into(), number(min));
    }
    if let Some(max) = traits.max {
        property.insert("max".into(), number(max));
    }
    let mut properties = Map::new();
    properties.insert(traits.source.clone(), Value::Object(property));
    json!({ "name": format!("PPE texture class {index}"), "properties": properties })
}

/// The schema for these PPE textures, from the cache when an identical set
/// of classes was seen before.
pub fn obtain_ppe_textures_metadata_schema(local: &MeshPrimitiveGpmLocal) -> Result<Arc<MetadataSchema>> {
    let classes: Vec<Value> =
        local.ppe_textures.iter().enumerate().map(|(i, t)| create_ppe_texture_class_json(t, i)).collect();
    let key = classes.iter().map(stringify).collect::<Vec<_>>().join(",");
    if let Some(schema) = PPE_TEXTURES_METADATA_SCHEMA_CACHE.with(|c| c.borrow().get(&key).cloned()) {
        return Ok(schema);
    }
    let mut class_map = Map::new();
    for (i, class) in classes.into_iter().enumerate() {
        class_map.insert(format!("ppeTexture_{i}"), class);
    }
    let id = format!("PPE_TEXTURE_SCHEMA_{}", ppe_textures_metadata_schema_cache_len());
    let schema = Arc::new(
        MetadataSchema::from_json(&json!({ "id": id, "classes": class_map }))
            .map_err(|e| RuntimeError::new(e.to_string()))?,
    );
    PPE_TEXTURES_METADATA_SCHEMA_CACHE.with(|c| c.borrow_mut().insert(key, schema.clone()));
    Ok(schema)
}

/// The PPE textures as property textures of their classes.
pub fn convert_to_structural_metadata<G: Gl>(
    local: &MeshPrimitiveGpmLocal,
    textures: &GltfTextures<G>,
) -> Result<StructuralMetadata<G>> {
    let schema = obtain_ppe_textures_metadata_schema(local)?;
    let mut property_textures = Vec::new();
    for (i, ppe_texture) in local.ppe_textures.iter().enumerate() {
        let class_id = format!("ppeTexture_{i}");
        let class = schema.classes.get(&class_id).cloned().ok_or_else(|| RuntimeError::new("Missing PPE class"))?;
        let mut info = Map::new();
        info.insert("index".into(), json!(ppe_texture.index));
        if let Some(tex_coord) = ppe_texture.tex_coord {
            info.insert("texCoord".into(), json!(tex_coord));
        }
        let mut properties = Map::new();
        properties.insert(ppe_texture.traits.source.clone(), Value::Object(info));
        let json = json!({ "class": class_id, "properties": properties });
        // PpeTexture has no name, so upstream's `ppeTexture.name` is undefined.
        property_textures.push(
            PropertyTexture::new(&json, class, textures, None, Some(MetadataId::Index(i)))
                .map_err(|e| RuntimeError::new(e.to_string()))?,
        );
    }
    Ok(StructuralMetadata::new(StructuralMetadataOptions {
        schema,
        property_tables: Some(Vec::new()),
        property_textures,
        property_attributes: Vec::new(),
        statistics: None,
        extras: None,
        extensions: None,
    }))
}

/// `ppeTextures` JSON as `PpeTexture`s.
fn ppe_textures_from_json(extension: &Value) -> Result<Vec<PpeTexture>> {
    let Some(list) = extension.get("ppeTextures").filter(|v| !v.is_null()) else { return Ok(Vec::new()) };
    let items = list.as_array().ok_or_else(|| RuntimeError::new("ppeTexturesJson is not iterable"))?;
    items
        .iter()
        .map(|t| {
            let traits = t
                .get("traits")
                .filter(|v| !v.is_null())
                .ok_or_else(|| RuntimeError::new("Cannot read properties of undefined (reading 'min')"))?;
            let index = t["index"].as_u64().ok_or_else(|| RuntimeError::new("Invalid PPE texture index"))? as usize;
            Ok(PpeTexture {
                traits: PpeMetadata {
                    min: traits["min"].as_f64(),
                    max: traits["max"].as_f64(),
                    source: traits.get("source").map_or_else(|| "undefined".into(), js::key),
                },
                no_data: t["noData"].as_f64(),
                offset: t["offset"].as_f64(),
                scale: t["scale"].as_f64(),
                index,
                tex_coord: t["texCoord"].as_u64().map(|c| c as u32),
            })
        })
        .collect()
}

pub struct GltfMeshPrimitiveGpmLoaderOptions<G: Gl, H> {
    pub cache: ResourceCache,
    pub request: GltfResource,
    /// The primitive's `NGA_gpm_local` extension.
    pub extension: Value,
    pub context: Context<G>,
    pub host: Rc<H>,
    pub texture_options: GltfTextureOptions,
    pub cache_key: Option<String>,
}

struct Inner<G: Gl + 'static, H: ModelImageHost<G> + 'static>
where
    H::Image: 'static,
{
    cache: ResourceCache,
    request: RefCell<Option<GltfResource>>,
    extension: RefCell<Option<Value>>,
    context: Context<G>,
    host: Rc<H>,
    texture_options: GltfTextureOptions,
    cache_key: Option<String>,
    state: Cell<ResourceLoaderState>,
    destroyed: Cell<bool>,
    promise: RefCell<Option<ResourceLoadFuture>>,
    textures: TextureLeases<G, H>,
    mesh_primitive_gpm_local: RefCell<Option<Rc<MeshPrimitiveGpmLocal>>>,
    structural_metadata: RefCell<Option<Rc<StructuralMetadata<G>>>>,
}

pub struct GltfMeshPrimitiveGpmLoader<G: Gl + 'static, H: ModelImageHost<G> + 'static>
where
    H::Image: 'static,
{
    inner: Rc<Inner<G, H>>,
}

impl<G: Gl + 'static, H: ModelImageHost<G> + 'static> GltfMeshPrimitiveGpmLoader<G, H>
where
    H::Image: 'static,
{
    pub fn new(options: GltfMeshPrimitiveGpmLoaderOptions<G, H>) -> Self {
        Self {
            inner: Rc::new(Inner {
                cache: options.cache,
                request: RefCell::new(Some(options.request)),
                extension: RefCell::new(Some(options.extension)),
                context: options.context,
                host: options.host,
                texture_options: options.texture_options,
                cache_key: options.cache_key,
                state: Cell::new(ResourceLoaderState::Unloaded),
                destroyed: Cell::new(false),
                promise: RefCell::new(None),
                textures: RefCell::new(Vec::new()),
                mesh_primitive_gpm_local: RefCell::new(None),
                structural_metadata: RefCell::new(None),
            }),
        }
    }

    pub fn mesh_primitive_gpm_local(&self) -> Option<Rc<MeshPrimitiveGpmLocal>> {
        self.inner.mesh_primitive_gpm_local.borrow().clone()
    }

    pub fn structural_metadata(&self) -> Option<Rc<StructuralMetadata<G>>> {
        self.inner.structural_metadata.borrow().clone()
    }

    /// One texture lease per distinct texture index; a later PPE texture with
    /// the same index replaces the texture info.
    fn load_textures(inner: &Inner<G, H>) -> Result<Vec<ResourceLoadFuture>> {
        let request = inner.request.borrow().clone().ok_or_else(|| RuntimeError::new("glTF was unloaded"))?;
        let mut ids = IdSet::new();
        if let Some(extension) = inner.extension.borrow().as_ref() {
            for t in extension.get("ppeTextures").and_then(Value::as_array).into_iter().flatten() {
                ids.insert_any(js::get(Some(t), "index"), t.clone());
            }
        }
        let mut futures = Vec::new();
        for (id, info) in ids.entries() {
            let lease = inner.cache.texture_loader(
                &request,
                info,
                inner.context.clone(),
                inner.host.clone(),
                inner.texture_options,
            )?;
            futures.push(lease.load());
            inner.textures.borrow_mut().push((id, lease));
        }
        Ok(futures)
    }

    fn unload_inner(inner: &Inner<G, H>) {
        inner.textures.borrow_mut().clear();
        inner.request.borrow_mut().take();
        inner.extension.borrow_mut().take();
        inner.structural_metadata.borrow_mut().take();
    }

    fn fail(weak: &Weak<Inner<G, H>>, error: RuntimeError) -> Result<bool> {
        let Some(inner) = weak.upgrade().filter(|i| !i.destroyed.get()) else { return Ok(false) };
        Self::fail_now(&inner, error)
    }

    fn fail_now(inner: &Inner<G, H>, error: RuntimeError) -> Result<bool> {
        Self::unload_inner(inner);
        inner.state.set(ResourceLoaderState::Failed);
        Err(loader_error("Failed to load GPM data", Some(&error)))
    }
}

impl<G: Gl + 'static, H: ModelImageHost<G> + 'static> ResourceLoader for GltfMeshPrimitiveGpmLoader<G, H>
where
    H::Image: 'static,
{
    fn cache_key(&self) -> Option<&str> {
        self.inner.cache_key.as_deref()
    }

    fn state(&self) -> ResourceLoaderState {
        self.inner.state.get()
    }

    fn is_destroyed(&self) -> bool {
        self.inner.destroyed.get()
    }

    fn load(&self) -> ResourceLoadFuture {
        if let Some(promise) = self.inner.promise.borrow().as_ref() {
            return promise.clone();
        }
        self.inner.state.set(ResourceLoaderState::Loading);
        let weak = Rc::downgrade(&self.inner);
        let futures = match Self::load_textures(&self.inner) {
            Ok(futures) => futures,
            // A synchronous throw fails before load returns.
            Err(e) => {
                let promise = futures_util::future::ready(Self::fail_now(&self.inner, e)).boxed_local().shared();
                *self.inner.promise.borrow_mut() = Some(promise.clone());
                return promise;
            }
        };
        let promise = async move {
            for future in futures {
                if let Err(e) = future.await {
                    return Self::fail(&weak, e);
                }
            }
            let Some(inner) = weak.upgrade().filter(|i| !i.destroyed.get()) else { return Ok(false) };
            // The glTF is no longer needed.
            inner.request.borrow_mut().take();
            inner.state.set(ResourceLoaderState::Loaded);
            Ok(true)
        }
        .boxed_local()
        .shared();
        *self.inner.promise.borrow_mut() = Some(promise.clone());
        promise
    }

    fn process(&self, frame: &FrameState) -> Result<bool> {
        let inner = &self.inner;
        if inner.state.get() == ResourceLoaderState::Ready {
            return Ok(true);
        }
        if inner.state.get() != ResourceLoaderState::Loaded {
            return Ok(false);
        }
        let mut ready = true;
        for (_, texture) in inner.textures.borrow().iter() {
            let texture_ready = texture.process(frame)?;
            ready = ready && texture_ready;
        }
        if !ready {
            return Ok(false);
        }
        let mut textures = GltfTextures::new();
        for (id, texture) in inner.textures.borrow().iter() {
            if !texture.is_destroyed() {
                if let Some(t) = texture.texture() {
                    textures.insert(id.clone(), t);
                }
            }
        }
        let extension = inner
            .extension
            .borrow()
            .clone()
            .ok_or_else(|| RuntimeError::new("Cannot read properties of undefined (reading 'ppeTextures')"))?;
        let local = Rc::new(MeshPrimitiveGpmLocal { ppe_textures: ppe_textures_from_json(&extension)? });
        // Set before converting, as upstream does.
        *inner.mesh_primitive_gpm_local.borrow_mut() = Some(local.clone());
        let metadata = convert_to_structural_metadata(&local, &textures)?;
        *inner.structural_metadata.borrow_mut() = Some(Rc::new(metadata));
        inner.state.set(ResourceLoaderState::Ready);
        Ok(true)
    }

    fn unload(&self) {
        Self::unload_inner(&self.inner);
    }

    fn destroy(&self) {
        self.unload();
        self.inner.destroyed.set(true);
    }
}

impl<G: Gl + 'static, H: ModelImageHost<G> + 'static> Drop for GltfMeshPrimitiveGpmLoader<G, H>
where
    H::Image: 'static,
{
    fn drop(&mut self) {
        self.destroy();
    }
}
