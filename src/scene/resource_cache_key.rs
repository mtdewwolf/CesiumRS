//! Pinned ResourceCacheKey identities for shared glTF resources and contexts.
use super::{GltfLoaderUtil, SupportedImageFormats};
use crate::core::{resource::DerivedResourceOptions, uri_helpers::get_absolute_uri, Resource, RuntimeError};
use serde_json::Value;
type Result<T> = std::result::Result<T, RuntimeError>;
#[derive(Clone, Copy)]
pub struct GltfKeyContext<'a> {
    pub gltf: &'a Value,
    pub gltf_resource: &'a Resource,
    pub base_resource: &'a Resource,
}
#[derive(Clone, Copy, Default)]
pub struct BufferKeyOptions<'a> {
    pub load_buffer: bool,
    pub load_typed_array: bool,
    pub context_id: Option<&'a str>,
    pub dequantize: bool,
}
impl BufferKeyOptions<'_> {
    fn suffix(self) -> Result<String> {
        if !self.load_buffer && !self.load_typed_array {
            return Err(RuntimeError::new("At least one of loadBuffer and loadTypedArray must be true."));
        }
        let mut suffix = String::new();
        if self.dequantize {
            suffix.push_str("-dequantize");
        }
        if self.load_buffer {
            suffix += &format!(
                "-buffer-context-{}",
                self.context_id.ok_or_else(|| RuntimeError::new("GPU buffer keys require a context ID"))?
            );
        }
        if self.load_typed_array {
            suffix += "-typed-array";
        }
        Ok(suffix)
    }
}
pub enum VertexKeySource<'a> {
    BufferView(usize),
    Draco { extension: &'a Value, semantic: &'a str },
    Spz { extension: &'a Value, semantic: &'a str },
}
pub struct ResourceCacheKey;
fn external(resource: &Resource) -> String {
    get_absolute_uri(&resource.url(), None)
}
fn embedded(parent: &Resource, id: usize) -> String {
    format!("{}-buffer-id-{id}", external(parent))
}
fn at(array: &Value, id: usize) -> Result<&Value> {
    array.get(id).ok_or_else(|| RuntimeError::new("glTF cache key index out of bounds"))
}
fn id(value: &Value) -> Result<usize> {
    value
        .as_u64()
        .and_then(|v| usize::try_from(v).ok())
        .ok_or_else(|| RuntimeError::new("Invalid glTF cache key index"))
}
fn number(value: &Value) -> String {
    value.as_f64().map(crate::core::math::js_number_to_string).unwrap_or_else(|| {
        if value.is_null() {
            "undefined".into()
        } else {
            value.to_string()
        }
    })
}
fn view_range(view: &Value) -> String {
    let (offset, length) =
        if let Some(meshopt) = view.pointer("/extensions/EXT_meshopt_compression").filter(|v| !v.is_null()) {
            (meshopt["byteOffset"].as_f64().unwrap_or(0.), meshopt["byteLength"].as_f64())
        } else {
            (view["byteOffset"].as_f64().unwrap_or(f64::NAN), view["byteLength"].as_f64())
        };
    format!(
        "{}-{}",
        crate::core::math::js_number_to_string(offset),
        crate::core::math::js_number_to_string(offset + length.unwrap_or(f64::NAN))
    )
}
fn buffer_key(context: GltfKeyContext<'_>, buffer: &Value, buffer_id: usize) -> Result<String> {
    if let Some(uri) = buffer["uri"].as_str() {
        let resource = context
            .base_resource
            .get_derived_resource(DerivedResourceOptions { url: Some(uri.into()), ..Default::default() })
            .ok_or_else(|| RuntimeError::new("Invalid glTF buffer URI"))?;
        Ok(external(&resource))
    } else {
        Ok(embedded(context.gltf_resource, buffer_id))
    }
}
fn draco_key(context: GltfKeyContext<'_>, extension: &Value) -> Result<String> {
    let view = at(&context.gltf["bufferViews"], id(&extension["bufferView"])?)?;
    let buffer_id = id(&view["buffer"])?;
    Ok(format!(
        "{}-range-{}",
        buffer_key(context, at(&context.gltf["buffers"], buffer_id)?, buffer_id)?,
        view_range(view)
    ))
}
fn image_key(context: GltfKeyContext<'_>, image_id: usize) -> Result<String> {
    let image = at(&context.gltf["images"], image_id)?;
    if let Some(uri) = image["uri"].as_str() {
        return context
            .base_resource
            .get_derived_resource(DerivedResourceOptions { url: Some(uri.into()), ..Default::default() })
            .map(|r| external(&r))
            .ok_or_else(|| RuntimeError::new("Invalid image URI"));
    }
    let view = at(&context.gltf["bufferViews"], id(&image["bufferView"])?)?;
    let buffer_id = id(&view["buffer"])?;
    Ok(format!(
        "{}-range-{}",
        buffer_key(context, at(&context.gltf["buffers"], buffer_id)?, buffer_id)?,
        view_range(view)
    ))
}
impl ResourceCacheKey {
    pub fn schema(schema: Option<&Value>, resource: Option<&Resource>) -> Result<String> {
        match (schema, resource) {
            (Some(schema), None) => Ok(format!("embedded-schema:{}", stringify(schema))),
            (None, Some(resource)) => Ok(format!("external-schema:{}", external(resource))),
            _ => Err(RuntimeError::new("One of options.schema and options.resource must be defined.")),
        }
    }
    pub fn external_buffer(resource: &Resource) -> String {
        format!("external-buffer:{}", external(resource))
    }
    pub fn embedded_buffer(parent: &Resource, buffer_id: usize) -> String {
        format!("embedded-buffer:{}", embedded(parent, buffer_id))
    }
    pub fn gltf(resource: &Resource) -> String {
        format!("gltf:{}", external(resource))
    }
    pub fn buffer_view(context: GltfKeyContext<'_>, view_id: usize) -> Result<String> {
        let view = at(&context.gltf["bufferViews"], view_id)?;
        let original = id(&view["buffer"])?;
        // Preserve the pinned key helper's original-buffer lookup before
        // applying meshopt's buffer ID override. Loading uses the override.
        let buffer = at(&context.gltf["buffers"], original)?;
        let buffer_id =
            view.pointer("/extensions/EXT_meshopt_compression/buffer").map(id).transpose()?.unwrap_or(original);
        Ok(format!("buffer-view:{}-range-{}", buffer_key(context, buffer, buffer_id)?, view_range(view)))
    }
    pub fn draco(context: GltfKeyContext<'_>, extension: &Value) -> Result<String> {
        Ok(format!("draco:{}", draco_key(context, extension)?))
    }
    pub fn spz(context: GltfKeyContext<'_>) -> Result<String> {
        Ok(format!("spz:{}", draco_key(context, &serde_json::json!({"bufferView":0}))?))
    }
    pub fn vertex_buffer(
        context: GltfKeyContext<'_>,
        source: VertexKeySource<'_>,
        options: BufferKeyOptions<'_>,
    ) -> Result<String> {
        let suffix = options.suffix()?;
        let key = match source {
            VertexKeySource::BufferView(id) => {
                let view = at(&context.gltf["bufferViews"], id)?;
                let buffer_id = crate::scene::resource_cache_key::id(&view["buffer"])?;
                format!(
                    "{}-range-{}",
                    buffer_key(context, at(&context.gltf["buffers"], buffer_id)?, buffer_id)?,
                    view_range(view)
                )
            }
            VertexKeySource::Draco { extension, semantic } => {
                format!("{}-draco-{semantic}", draco_key(context, extension)?)
            }
            VertexKeySource::Spz { extension: _, semantic } => {
                format!("{}-spz-{semantic}", draco_key(context, &serde_json::json!({"bufferView":0}))?)
            }
        };
        Ok(format!("vertex-buffer:{key}{suffix}"))
    }
    pub fn index_buffer(
        context: GltfKeyContext<'_>,
        accessor_id: usize,
        draco: Option<&Value>,
        mut options: BufferKeyOptions<'_>,
    ) -> Result<String> {
        options.dequantize = false;
        let suffix = options.suffix()?;
        let key = if let Some(draco) = draco {
            format!("{}-draco", draco_key(context, draco)?)
        } else {
            let accessor = at(&context.gltf["accessors"], accessor_id)?;
            let view = at(&context.gltf["bufferViews"], id(&accessor["bufferView"])?)?;
            let buffer_id = id(&view["buffer"])?;
            let offset =
                view["byteOffset"].as_f64().unwrap_or(f64::NAN) + accessor["byteOffset"].as_f64().unwrap_or(f64::NAN);
            format!(
                "{}-accessor-{}-{}-{}-{}",
                buffer_key(context, at(&context.gltf["buffers"], buffer_id)?, buffer_id)?,
                crate::core::math::js_number_to_string(offset),
                number(&accessor["componentType"]),
                accessor["type"].as_str().unwrap_or("undefined"),
                number(&accessor["count"])
            )
        };
        Ok(format!("index-buffer:{key}{suffix}"))
    }
    pub fn image(context: GltfKeyContext<'_>, image_id: usize) -> Result<String> {
        Ok(format!("image:{}", image_key(context, image_id)?))
    }
    pub fn texture(
        context: GltfKeyContext<'_>,
        info: &Value,
        formats: SupportedImageFormats,
        context_id: &str,
    ) -> Result<String> {
        let image_id = GltfLoaderUtil::image_id(context.gltf, id(&info["index"])?, formats)?;
        let sampler = GltfLoaderUtil::create_sampler(context.gltf, info, false)?;
        Ok(format!(
            "texture:{}-sampler-{}-{}-{}-{}-context-{context_id}",
            image_key(context, image_id)?,
            sampler.wrap_s as u32,
            sampler.wrap_t as u32,
            sampler.minification_filter as u32,
            sampler.magnification_filter as u32
        ))
    }
}

fn stringify(value: &Value) -> String {
    match value {
        Value::Number(n) => crate::core::math::js_number_to_string(n.as_f64().expect("JSON number")),
        Value::Array(values) => format!("[{}]", values.iter().map(stringify).collect::<Vec<_>>().join(",")),
        Value::Object(_) => format!(
            "{{{}}}",
            super::gltf_pipeline::js::for_in(Some(value))
                .iter()
                .map(|key| format!("{}:{}", serde_json::to_string(key).expect("key"), stringify(&value[key])))
                .collect::<Vec<_>>()
                .join(",")
        ),
        _ => value.to_string(),
    }
}
