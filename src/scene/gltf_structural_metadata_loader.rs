//! CesiumJS `GltfStructuralMetadataLoader`: loads the buffer views, textures
//! and schema a glTF's `EXT_structural_metadata` (or legacy
//! `EXT_feature_metadata`) extension uses, through the shared
//! [`ResourceCache`], then parses them into [`StructuralMetadata`].
//!
//! As in CesiumJS, the loader is owned by the glTF loader rather than
//! cached. Buffer views, textures and the schema are cache leases: buffer
//! views are released once the metadata is parsed, and everything on unload.

use super::gltf_pipeline::js;
use super::metadata_table_property::ByteView;
use super::metadata_value::JsMap;
use super::resource_loader::loader_error;
use super::structural_metadata::StructuralMetadataWarning;
use super::*;
use crate::core::resource::DerivedResourceOptions;
use crate::core::RuntimeError;
use crate::renderer::{Context, Gl};
use futures_util::future::FutureExt;
use serde_json::{Map, Value};
use std::cell::{Cell, RefCell};
use std::rc::{Rc, Weak};

type Result<T> = std::result::Result<T, RuntimeError>;

/// Texture leases by glTF texture index, as a JavaScript property key.
pub(crate) type TextureLeases<G, H> = RefCell<Vec<(String, CacheLease<GltfTextureLoader<G, H>>)>>;

pub struct GltfStructuralMetadataLoaderOptions<G: Gl, H> {
    pub cache: ResourceCache,
    /// The glTF and the resources its URIs resolve against.
    pub request: GltfResource,
    /// `EXT_structural_metadata`; this or `extension_legacy` is required.
    pub extension: Option<Value>,
    /// `EXT_feature_metadata`.
    pub extension_legacy: Option<Value>,
    pub context: Context<G>,
    pub host: Rc<H>,
    /// Supported formats and whether texture uploads are spread over frames.
    pub texture_options: GltfTextureOptions,
    pub cache_key: Option<String>,
}

struct Inner<G: Gl + 'static, H: ModelImageHost<G> + 'static>
where
    H::Image: 'static,
{
    cache: ResourceCache,
    request: RefCell<Option<GltfResource>>,
    base: crate::core::Resource,
    extension: Option<Value>,
    extension_legacy: Option<Value>,
    context: Context<G>,
    host: Rc<H>,
    texture_options: GltfTextureOptions,
    cache_key: Option<String>,
    state: Cell<ResourceLoaderState>,
    destroyed: Cell<bool>,
    promise: RefCell<Option<ResourceLoadFuture>>,
    buffer_views: RefCell<Vec<(String, CacheLease<GltfBufferViewLoader>)>>,
    textures: TextureLeases<G, H>,
    schema: RefCell<Option<CacheLease<MetadataSchemaLoader>>>,
    structural_metadata: RefCell<Option<Rc<StructuralMetadata<G>>>>,
    warnings: RefCell<Vec<StructuralMetadataWarning>>,
}

pub struct GltfStructuralMetadataLoader<G: Gl + 'static, H: ModelImageHost<G> + 'static>
where
    H::Image: 'static,
{
    inner: Rc<Inner<G, H>>,
}

/// The ids a property set names, in JavaScript object key order:
/// `set[value] = true` for each, then `for (const id in set)`.
pub(crate) struct IdSet(Map<String, Value>);

impl IdSet {
    pub(crate) fn new() -> Self {
        Self(Map::new())
    }

    /// `if (defined(id)) set[id] = value`.
    pub(crate) fn insert(&mut self, id: Option<&Value>, value: Value) {
        if js::defined(id) {
            self.insert_any(id, value);
        }
    }

    /// `set[id] = value`, where a missing id is the key `"undefined"`. A
    /// repeated id keeps its place and takes the later value.
    pub(crate) fn insert_any(&mut self, id: Option<&Value>, value: Value) {
        self.0.insert(id.map_or_else(|| "undefined".to_string(), js::key), value);
    }

    pub(crate) fn entries(&self) -> Vec<(String, Value)> {
        js::object_keys(&self.0).into_iter().map(|k| (k.clone(), self.0[&k].clone())).collect()
    }
}

/// `parseInt(id)` as a glTF index.
fn parse_index(id: &str) -> Option<usize> {
    let digits: String = id.trim_start().chars().take_while(char::is_ascii_digit).collect();
    digits.parse().ok()
}

fn properties_of(json: &Value) -> Vec<&Value> {
    let properties = json.get("properties").filter(|p| !p.is_null());
    js::for_in(properties).into_iter().filter_map(|k| js::get(properties, &k)).collect()
}

fn gather_used_buffer_view_ids(extension: &Value) -> IdSet {
    let mut ids = IdSet::new();
    for table in extension.get("propertyTables").and_then(Value::as_array).into_iter().flatten() {
        for property in properties_of(table) {
            for key in ["values", "arrayOffsets", "stringOffsets"] {
                ids.insert(js::get(Some(property), key), Value::Bool(true));
            }
        }
    }
    ids
}

fn gather_used_buffer_view_ids_legacy(extension: &Value) -> IdSet {
    let mut ids = IdSet::new();
    let tables = extension.get("featureTables").filter(|t| !t.is_null());
    for id in js::for_in(tables) {
        let table = js::get(tables, &id).expect("own key");
        for property in properties_of(table) {
            for key in ["bufferView", "arrayOffsetBufferView", "stringOffsetBufferView"] {
                ids.insert(js::get(Some(property), key), Value::Bool(true));
            }
        }
    }
    ids
}

fn gather_used_texture_ids(extension: &Value) -> IdSet {
    let mut ids = IdSet::new();
    for texture in extension.get("propertyTextures").and_then(Value::as_array).into_iter().flatten() {
        for info in properties_of(texture) {
            ids.insert_any(js::get(Some(info), "index"), info.clone());
        }
    }
    ids
}

fn gather_used_texture_ids_legacy(extension: &Value) -> IdSet {
    let mut ids = IdSet::new();
    let textures = extension.get("featureTextures").filter(|t| !t.is_null());
    for id in js::for_in(textures) {
        let texture = js::get(textures, &id).expect("own key");
        for property in properties_of(texture) {
            let info = js::get(Some(property), "texture").cloned().unwrap_or(Value::Null);
            ids.insert_any(js::get(Some(&info), "index"), info.clone());
        }
    }
    ids
}

impl<G: Gl + 'static, H: ModelImageHost<G> + 'static> GltfStructuralMetadataLoader<G, H>
where
    H::Image: 'static,
{
    pub fn new(options: GltfStructuralMetadataLoaderOptions<G, H>) -> Result<Self> {
        if options.extension.is_none() && options.extension_legacy.is_none() {
            return Err(RuntimeError::new("One of options.extension or options.extensionLegacy must be specified"));
        }
        Ok(Self {
            inner: Rc::new(Inner {
                cache: options.cache,
                base: options.request.base.clone(),
                request: RefCell::new(Some(options.request)),
                extension: options.extension,
                extension_legacy: options.extension_legacy,
                context: options.context,
                host: options.host,
                texture_options: options.texture_options,
                cache_key: options.cache_key,
                state: Cell::new(ResourceLoaderState::Unloaded),
                destroyed: Cell::new(false),
                promise: RefCell::new(None),
                buffer_views: RefCell::new(Vec::new()),
                textures: RefCell::new(Vec::new()),
                schema: RefCell::new(None),
                structural_metadata: RefCell::new(None),
                warnings: RefCell::new(Vec::new()),
            }),
        })
    }

    /// The parsed metadata, once ready.
    pub fn structural_metadata(&self) -> Option<Rc<StructuralMetadata<G>>> {
        self.inner.structural_metadata.borrow().clone()
    }

    /// What parsing logged (property table texture failures).
    pub fn warnings(&self) -> Vec<StructuralMetadataWarning> {
        self.inner.warnings.borrow().clone()
    }

    /// The leases' futures, created in upstream's order: buffer views, then
    /// textures, then the schema. A buffer view or schema failure is a
    /// rejected promise that `Promise.all` reports first, returned with the
    /// futures; a texture failure throws synchronously, before the schema is
    /// requested, and is the `Err`.
    fn start(inner: &Inner<G, H>) -> Result<(Vec<ResourceLoadFuture>, Option<RuntimeError>)> {
        let request = inner.request.borrow().clone().ok_or_else(|| RuntimeError::new("glTF was unloaded"))?;
        let mut futures = Vec::new();
        let mut first_error = None;

        let buffer_view_ids = match (&inner.extension, &inner.extension_legacy) {
            (Some(extension), _) => gather_used_buffer_view_ids(extension),
            (None, Some(legacy)) => gather_used_buffer_view_ids_legacy(legacy),
            (None, None) => IdSet::new(),
        };
        for (id, _) in buffer_view_ids.entries() {
            let lease = parse_index(&id).ok_or_else(|| RuntimeError::new("Invalid buffer view ID")).and_then(|index| {
                inner.cache.buffer_view_loader(request.document.clone(), index, &request.resource, &request.base)
            });
            match lease {
                Ok(lease) => {
                    futures.push(lease.load());
                    inner.buffer_views.borrow_mut().push((id, lease));
                }
                Err(e) => {
                    first_error = Some(e);
                    break;
                }
            }
        }

        let texture_ids = match (&inner.extension, &inner.extension_legacy) {
            (Some(extension), _) => gather_used_texture_ids(extension),
            (None, Some(legacy)) => gather_used_texture_ids_legacy(legacy),
            (None, None) => IdSet::new(),
        };
        for (id, info) in texture_ids.entries() {
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

        let extension = inner.extension.as_ref().or(inner.extension_legacy.as_ref()).expect("an extension");
        let schema = match extension.get("schemaUri").filter(|u| !u.is_null()) {
            Some(uri) => inner
                .base
                .get_derived_resource(DerivedResourceOptions { url: Some(js::key(uri)), ..Default::default() })
                .ok_or_else(|| RuntimeError::new("Invalid schema URI"))
                .and_then(|resource| inner.cache.schema_loader(None, Some(resource))),
            None => inner.cache.schema_loader(extension.get("schema").cloned(), None),
        };
        match schema {
            Ok(lease) => {
                futures.push(lease.load());
                *inner.schema.borrow_mut() = Some(lease);
            }
            Err(e) => {
                first_error.get_or_insert(e);
            }
        }
        Ok((futures, first_error))
    }

    fn finish(
        weak: Weak<Inner<G, H>>,
        futures: Vec<ResourceLoadFuture>,
        deferred: Option<RuntimeError>,
    ) -> ResourceLoadFuture {
        async move {
            if let Some(e) = deferred {
                return Self::fail(&weak, e);
            }
            if let Err(e) = futures_util::future::try_join_all(futures).await {
                return Self::fail(&weak, e);
            }
            let Some(inner) = weak.upgrade().filter(|i| !i.destroyed.get()) else { return Ok(false) };
            // The glTF is no longer needed.
            inner.request.borrow_mut().take();
            inner.state.set(ResourceLoaderState::Loaded);
            Ok(true)
        }
        .boxed_local()
        .shared()
    }

    fn unload_buffer_views(inner: &Inner<G, H>) {
        inner.buffer_views.borrow_mut().clear();
    }

    fn unload_inner(inner: &Inner<G, H>) {
        Self::unload_buffer_views(inner);
        inner.textures.borrow_mut().clear();
        inner.schema.borrow_mut().take();
        inner.structural_metadata.borrow_mut().take();
    }

    fn fail(weak: &Weak<Inner<G, H>>, error: RuntimeError) -> Result<bool> {
        let Some(inner) = weak.upgrade().filter(|i| !i.destroyed.get()) else { return Ok(false) };
        Self::fail_now(&inner, error)
    }

    fn fail_now(inner: &Inner<G, H>, error: RuntimeError) -> Result<bool> {
        Self::unload_inner(inner);
        inner.state.set(ResourceLoaderState::Failed);
        Err(loader_error("Failed to load structural metadata", Some(&error)))
    }
}

impl<G: Gl + 'static, H: ModelImageHost<G> + 'static> ResourceLoader for GltfStructuralMetadataLoader<G, H>
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
        let promise = match Self::start(&self.inner) {
            // A synchronous throw fails before load returns.
            Err(e) => futures_util::future::ready(Self::fail_now(&self.inner, e)).boxed_local().shared(),
            Ok((futures, deferred)) => Self::finish(weak, futures, deferred),
        };
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

        let schema = inner
            .schema
            .borrow()
            .as_ref()
            .and_then(|s| s.schema())
            .ok_or_else(|| RuntimeError::new("Cannot read properties of undefined (reading 'classes')"))?;
        // Each view is copied into its own Uint8Array upstream.
        let sources: Vec<(String, gltf_pipeline::BufferSource)> = inner
            .buffer_views
            .borrow()
            .iter()
            .filter(|(_, view)| !view.is_destroyed())
            .filter_map(|(id, view)| view.typed_array().map(|bytes| (id.clone(), bytes)))
            .collect();
        let buffer_views: JsMap<ByteView<'_>> = sources
            .iter()
            .map(|(id, source)| (id.clone(), ByteView { buffer: source.bytes(), byte_offset: 0 }))
            .collect();
        let mut textures = GltfTextures::new();
        for (id, texture) in inner.textures.borrow().iter() {
            if !texture.is_destroyed() {
                if let Some(t) = texture.texture() {
                    textures.insert(id.clone(), t);
                }
            }
        }

        let metadata = match &inner.extension {
            Some(extension) => {
                let options = ParseStructuralMetadataOptions {
                    extension,
                    schema,
                    buffer_views: &buffer_views,
                    textures: &textures,
                    context: Some(&inner.context),
                };
                parse_structural_metadata(&options, &mut inner.warnings.borrow_mut())
            }
            None => {
                let legacy = inner.extension_legacy.as_ref().expect("an extension");
                let options = ParseFeatureMetadataLegacyOptions {
                    extension: legacy,
                    schema,
                    buffer_views: &buffer_views,
                    textures: &textures,
                };
                parse_feature_metadata_legacy(&options)
            }
        }
        .map_err(|e| RuntimeError::new(e.to_string()))?;
        drop(buffer_views);
        drop(sources);
        Self::unload_buffer_views(inner);
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

impl<G: Gl + 'static, H: ModelImageHost<G> + 'static> Drop for GltfStructuralMetadataLoader<G, H>
where
    H::Image: 'static,
{
    fn drop(&mut self) {
        self.destroy();
    }
}
