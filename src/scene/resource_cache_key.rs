//! CesiumJS `ResourceCacheKey`: the strings ResourceCache shares loaders under.
//!
//! Keys are built from glTF JSON with JavaScript's semantics, because two
//! loaders share a resource exactly when their keys are equal:
//! - template literals print values as JavaScript does (`undefined` for a
//!   missing `byteOffset`, numbers through `Number#toString`);
//! - `byteOffset + byteLength` is JavaScript `+` (a missing operand is NaN);
//! - reading a property of a missing buffer, view, accessor or image is the
//!   `TypeError` JavaScript throws.
//!
//! The options' debug-build `DeveloperError` checks are not ported: Rust
//! builds the key as release CesiumJS does. The schema source is an enum, so
//! "exactly one of schema and resource" holds by construction. Upstream reads
//! `frameState.context.id`; Rust callers pass the context id.

use super::gltf_loader_util::{create_sampler, get_image_id_from_texture};
use super::gltf_pipeline::{js, PipelineError};
use super::{has_extension, SupportedImageFormats};
use crate::core::math::js_number_to_string;
use crate::core::resource::DerivedResourceOptions;
use crate::core::uri_helpers::get_absolute_uri;
use crate::core::Resource;
use serde_json::Value;

/// `${value}` in a template literal.
fn template(value: Option<&Value>) -> String {
    value.map_or_else(|| "undefined".into(), js::key)
}

/// JavaScript `a + b` for the JSON values a key adds: numeric addition, or
/// concatenation when either side is a string, array or object.
fn add(a: Option<&Value>, b: Option<&Value>) -> String {
    let stringy = |v: Option<&Value>| matches!(v, Some(Value::String(_) | Value::Array(_) | Value::Object(_)));
    if stringy(a) || stringy(b) {
        return template(a) + &template(b);
    }
    js_number_to_string(js::to_number(a) + js::to_number(b))
}

/// `JSON.stringify` of parsed JSON: object keys in JavaScript property order,
/// numbers as `Number#toString` writes them, non-finite numbers as `null`.
pub fn json_stringify(value: &Value) -> String {
    match value {
        Value::Null => "null".into(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => {
            let v = n.as_f64().unwrap_or(f64::NAN);
            if v.is_finite() {
                js_number_to_string(v)
            } else {
                "null".into()
            }
        }
        Value::String(_) => serde_json::to_string(value).unwrap_or_default(),
        Value::Array(items) => format!("[{}]", items.iter().map(json_stringify).collect::<Vec<_>>().join(",")),
        Value::Object(map) => {
            let fields = js::object_keys(map)
                .into_iter()
                .map(|k| format!("{}:{}", serde_json::to_string(&k).unwrap_or_default(), json_stringify(&map[&k])))
                .collect::<Vec<_>>();
            format!("{{{}}}", fields.join(","))
        }
    }
}

fn external_resource_key(resource: &Resource) -> String {
    get_absolute_uri(&resource.url(), None)
}

fn embedded_buffer_key(parent: &Resource, buffer_id: Option<&Value>) -> String {
    format!("{}-buffer-id-{}", external_resource_key(parent), template(buffer_id))
}

fn derived_key(base: &Resource, uri: &Value) -> Result<String, PipelineError> {
    let options = DerivedResourceOptions { url: Some(js::key(uri)), ..Default::default() };
    let resource = base
        .get_derived_resource(options)
        .ok_or_else(|| PipelineError::Type(format!("Invalid URL: {}", js::key(uri))))?;
    Ok(external_resource_key(&resource))
}

fn buffer_key(
    buffer: Option<&Value>,
    buffer_id: Option<&Value>,
    gltf_resource: &Resource,
    base_resource: &Resource,
) -> Result<String, PipelineError> {
    let buffer = js::object(buffer, "uri")?;
    match js::get(Some(buffer), "uri").filter(|v| !v.is_null()) {
        Some(uri) => derived_key(base_resource, uri),
        None => Ok(embedded_buffer_key(gltf_resource, buffer_id)),
    }
}

/// `${byteOffset}-${byteOffset + byteLength}`, read from `EXT_meshopt_compression` when present.
fn buffer_view_range(buffer_view: &Value) -> String {
    let (offset, length) = if has_extension(buffer_view, "EXT_meshopt_compression") {
        let meshopt = js::get(js::get(Some(buffer_view), "extensions"), "EXT_meshopt_compression");
        let offset = js::get(meshopt, "byteOffset").filter(|v| !v.is_null()).cloned().unwrap_or(Value::from(0));
        (Some(offset), js::get(meshopt, "byteLength").cloned())
    } else {
        (js::get(Some(buffer_view), "byteOffset").cloned(), js::get(Some(buffer_view), "byteLength").cloned())
    };
    format!("{}-{}", template(offset.as_ref()), add(offset.as_ref(), length.as_ref()))
}

/// The buffer and range of buffer view `id`: `<buffer key>-range-<range>`.
fn buffer_view_source_key(
    gltf: &Value,
    buffer_view_id: &Value,
    gltf_resource: &Resource,
    base_resource: &Resource,
) -> Result<String, PipelineError> {
    let buffer_view = js::object(js::index(js::get(Some(gltf), "bufferViews"), buffer_view_id), "buffer")?;
    let buffer_id = js::get(Some(buffer_view), "buffer");
    let buffer = buffer_id.and_then(|id| js::index(js::get(Some(gltf), "buffers"), id));
    let buffer_key = buffer_key(buffer, buffer_id, gltf_resource, base_resource)?;
    Ok(format!("{buffer_key}-range-{}", buffer_view_range(buffer_view)))
}

fn draco_key(
    gltf: &Value,
    draco: &Value,
    gltf_resource: &Resource,
    base_resource: &Resource,
) -> Result<String, PipelineError> {
    let view = js::get(Some(draco), "bufferView").cloned().unwrap_or(Value::Null);
    buffer_view_source_key(gltf, &view, gltf_resource, base_resource)
}

/// SPZ always reads buffer view 0.
fn spz_key(gltf: &Value, gltf_resource: &Resource, base_resource: &Resource) -> Result<String, PipelineError> {
    buffer_view_source_key(gltf, &Value::from(0), gltf_resource, base_resource)
}

fn image_key(
    gltf: &Value,
    image_id: Option<&Value>,
    gltf_resource: &Resource,
    base_resource: &Resource,
) -> Result<String, PipelineError> {
    let image = js::object(image_id.and_then(|id| js::index(js::get(Some(gltf), "images"), id)), "bufferView")?;
    if let Some(uri) = js::get(Some(image), "uri").filter(|v| !v.is_null()) {
        return derived_key(base_resource, uri);
    }
    let view = js::get(Some(image), "bufferView").cloned().unwrap_or(Value::Null);
    buffer_view_source_key(gltf, &view, gltf_resource, base_resource)
}

/// A metadata schema, given inline or as an external resource.
#[derive(Clone, Copy)]
pub enum SchemaSource<'a> {
    Embedded(&'a Value),
    External(&'a Resource),
}

/// `ResourceCacheKey.getSchemaCacheKey`.
pub fn get_schema_cache_key(source: SchemaSource) -> String {
    match source {
        SchemaSource::Embedded(schema) => format!("embedded-schema:{}", json_stringify(schema)),
        SchemaSource::External(resource) => format!("external-schema:{}", external_resource_key(resource)),
    }
}

/// `ResourceCacheKey.getExternalBufferCacheKey`.
pub fn get_external_buffer_cache_key(resource: &Resource) -> String {
    format!("external-buffer:{}", external_resource_key(resource))
}

/// `ResourceCacheKey.getEmbeddedBufferCacheKey`.
pub fn get_embedded_buffer_cache_key(parent_resource: &Resource, buffer_id: usize) -> String {
    format!("embedded-buffer:{}", embedded_buffer_key(parent_resource, Some(&Value::from(buffer_id))))
}

/// `ResourceCacheKey.getGltfCacheKey`.
pub fn get_gltf_cache_key(gltf_resource: &Resource) -> String {
    format!("gltf:{}", external_resource_key(gltf_resource))
}

/// `ResourceCacheKey.getBufferViewCacheKey`. As upstream, a meshopt view keys
/// on the compressed range and the meshopt buffer's id, but on the view's own
/// `buffer` for whether that buffer is external.
pub fn get_buffer_view_cache_key(
    gltf: &Value,
    buffer_view_id: usize,
    gltf_resource: &Resource,
    base_resource: &Resource,
) -> Result<String, PipelineError> {
    let buffer_view =
        js::object(js::index(js::get(Some(gltf), "bufferViews"), &Value::from(buffer_view_id)), "buffer")?;
    let mut buffer_id = js::get(Some(buffer_view), "buffer");
    let buffer = buffer_id.and_then(|id| js::index(js::get(Some(gltf), "buffers"), id));
    if has_extension(buffer_view, "EXT_meshopt_compression") {
        buffer_id = js::get(js::get(js::get(Some(buffer_view), "extensions"), "EXT_meshopt_compression"), "buffer");
    }
    let buffer_key = buffer_key(buffer, buffer_id, gltf_resource, base_resource)?;
    Ok(format!("buffer-view:{buffer_key}-range-{}", buffer_view_range(buffer_view)))
}

/// `ResourceCacheKey.getDracoCacheKey` for a `KHR_draco_mesh_compression` extension object.
pub fn get_draco_cache_key(
    gltf: &Value,
    draco: &Value,
    gltf_resource: &Resource,
    base_resource: &Resource,
) -> Result<String, PipelineError> {
    Ok(format!("draco:{}", draco_key(gltf, draco, gltf_resource, base_resource)?))
}

/// `ResourceCacheKey.getSpzCacheKey`.
pub fn get_spz_cache_key(
    gltf: &Value,
    gltf_resource: &Resource,
    base_resource: &Resource,
) -> Result<String, PipelineError> {
    Ok(format!("spz:{}", spz_key(gltf, gltf_resource, base_resource)?))
}

/// Options of `ResourceCacheKey.getVertexBufferCacheKey`.
#[derive(Clone, Copy)]
pub struct VertexBufferKeyOptions<'a> {
    pub gltf: &'a Value,
    pub gltf_resource: &'a Resource,
    pub base_resource: &'a Resource,
    pub context_id: &'a str,
    pub buffer_view_id: Option<usize>,
    /// The primitive's `KHR_draco_mesh_compression` extension object.
    pub draco: Option<&'a Value>,
    /// Whether the primitive is `KHR_spz_gaussian_splats_compression`-compressed.
    pub spz: bool,
    pub attribute_semantic: Option<&'a str>,
    pub dequantize: bool,
    pub load_buffer: bool,
    pub load_typed_array: bool,
}

fn suffix(dequantize: bool, load_buffer: bool, load_typed_array: bool, context_id: &str) -> String {
    let mut suffix = String::new();
    if dequantize {
        suffix += "-dequantize";
    }
    if load_buffer {
        suffix += &format!("-buffer-context-{context_id}");
    }
    if load_typed_array {
        suffix += "-typed-array";
    }
    suffix
}

/// `ResourceCacheKey.getVertexBufferCacheKey`.
pub fn get_vertex_buffer_cache_key(o: VertexBufferKeyOptions) -> Result<String, PipelineError> {
    let suffix = suffix(o.dequantize, o.load_buffer, o.load_typed_array, o.context_id);
    let semantic = o.attribute_semantic.unwrap_or("undefined");
    if let Some(draco) = o.draco.filter(|v| !v.is_null()) {
        let key = draco_key(o.gltf, draco, o.gltf_resource, o.base_resource)?;
        return Ok(format!("vertex-buffer:{key}-draco-{semantic}{suffix}"));
    }
    if o.spz {
        let key = spz_key(o.gltf, o.gltf_resource, o.base_resource)?;
        return Ok(format!("vertex-buffer:{key}-spz-{semantic}{suffix}"));
    }
    let view = o.buffer_view_id.map_or(Value::Null, Value::from);
    let key = buffer_view_source_key(o.gltf, &view, o.gltf_resource, o.base_resource)?;
    Ok(format!("vertex-buffer:{key}{suffix}"))
}

/// Options of `ResourceCacheKey.getIndexBufferCacheKey`.
#[derive(Clone, Copy)]
pub struct IndexBufferKeyOptions<'a> {
    pub gltf: &'a Value,
    pub accessor_id: usize,
    pub gltf_resource: &'a Resource,
    pub base_resource: &'a Resource,
    pub context_id: &'a str,
    pub draco: Option<&'a Value>,
    pub load_buffer: bool,
    pub load_typed_array: bool,
}

/// `ResourceCacheKey.getIndexBufferCacheKey`.
pub fn get_index_buffer_cache_key(o: IndexBufferKeyOptions) -> Result<String, PipelineError> {
    let suffix = suffix(false, o.load_buffer, o.load_typed_array, o.context_id);
    if let Some(draco) = o.draco.filter(|v| !v.is_null()) {
        let key = draco_key(o.gltf, draco, o.gltf_resource, o.base_resource)?;
        return Ok(format!("index-buffer:{key}-draco{suffix}"));
    }
    let accessor =
        js::object(js::index(js::get(Some(o.gltf), "accessors"), &Value::from(o.accessor_id)), "bufferView")?;
    let view_id = js::get(Some(accessor), "bufferView");
    let buffer_view = js::object(view_id.and_then(|id| js::index(js::get(Some(o.gltf), "bufferViews"), id)), "buffer")?;
    let buffer_id = js::get(Some(buffer_view), "buffer");
    let buffer = buffer_id.and_then(|id| js::index(js::get(Some(o.gltf), "buffers"), id));
    let buffer_key = buffer_key(buffer, buffer_id, o.gltf_resource, o.base_resource)?;
    fn field<'a>(v: &'a Value, k: &str) -> Option<&'a Value> {
        js::get(Some(v), k)
    }
    let accessor_key = format!(
        "{}-{}-{}-{}",
        add(field(buffer_view, "byteOffset"), field(accessor, "byteOffset")),
        template(field(accessor, "componentType")),
        template(field(accessor, "type")),
        template(field(accessor, "count")),
    );
    Ok(format!("index-buffer:{buffer_key}-accessor-{accessor_key}{suffix}"))
}

/// `ResourceCacheKey.getImageCacheKey`.
pub fn get_image_cache_key(
    gltf: &Value,
    image_id: usize,
    gltf_resource: &Resource,
    base_resource: &Resource,
) -> Result<String, PipelineError> {
    Ok(format!("image:{}", image_key(gltf, Some(&Value::from(image_id)), gltf_resource, base_resource)?))
}

/// `ResourceCacheKey.getTextureCacheKey`: the image the context will load
/// (see `GltfLoaderUtil.getImageIdFromTexture`) and its sampler.
pub fn get_texture_cache_key(
    gltf: &Value,
    texture_info: &Value,
    gltf_resource: &Resource,
    base_resource: &Resource,
    supported_image_formats: SupportedImageFormats,
    context_id: &str,
) -> Result<String, PipelineError> {
    let texture_id = js::get(Some(texture_info), "index").cloned().unwrap_or(Value::Null);
    let image_id = get_image_id_from_texture(gltf, &texture_id, supported_image_formats)?;
    let image_key = image_key(gltf, image_id, gltf_resource, base_resource)?;
    let sampler = create_sampler(gltf, texture_info, false)?;
    Ok(format!(
        "texture:{image_key}-sampler-{}-{}-{}-{}-context-{context_id}",
        sampler.wrap_s as u32,
        sampler.wrap_t as u32,
        sampler.minification_filter as u32,
        sampler.magnification_filter as u32
    ))
}
