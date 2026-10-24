//! CesiumJS `GltfLoaderUtil`: texture image selection, glTF samplers and
//! model texture readers.
//!
//! glTF JSON is read with JavaScript's property semantics (`gltf_pipeline::js`).
//! Reading a property of a missing texture or sampler is the `TypeError`
//! JavaScript throws. A sampler enum value outside its GL constants is the
//! `Sampler` constructor's debug-build `DeveloperError`: release CesiumJS
//! would store the raw number, which the typed [`Sampler`] cannot hold.

use super::gltf_pipeline::{js, PipelineError};
use super::model_components::TextureReader;
use super::SupportedImageFormats;
use crate::core::js_math::JsMath;
use crate::core::Matrix3;
use crate::renderer::{Gl, Sampler, Texture, TextureMagnificationFilter, TextureMinificationFilter, TextureWrap};
use serde_json::Value;
use std::rc::Rc;

/// `GltfLoaderUtil.getImageIdFromTexture`: the image a texture samples, preferring
/// `EXT_texture_webp` and then `KHR_texture_basisu` when the context supports them.
/// `None` is an undefined `source`.
pub fn get_image_id_from_texture<'a>(
    gltf: &'a Value,
    texture_id: &Value,
    supported_image_formats: SupportedImageFormats,
) -> Result<Option<&'a Value>, PipelineError> {
    let texture = js::object(js::index(js::get(Some(gltf), "textures"), texture_id), "extensions")?;
    let extensions = js::get(Some(texture), "extensions");
    if js::defined(extensions) {
        let webp = js::get(extensions, "EXT_texture_webp");
        let basis = js::get(extensions, "KHR_texture_basisu");
        if supported_image_formats.webp && js::defined(webp) {
            return Ok(js::get(webp, "source"));
        } else if supported_image_formats.basis && js::defined(basis) {
            return Ok(js::get(basis, "source"));
        }
    }
    Ok(js::get(Some(texture), "source"))
}

/// `value ?? fallback`, then the strict `validate` of the Sampler constructor.
fn sampler_enum<T>(
    value: Option<&Value>,
    fallback: T,
    name: &str,
    from_gl: impl Fn(u32) -> Option<T>,
) -> Result<T, PipelineError> {
    if !js::defined(value) {
        return Ok(fallback);
    }
    value
        .and_then(Value::as_f64)
        .filter(|v| v.fract() == 0.0 && *v >= 0.0 && *v <= u32::MAX as f64)
        .and_then(|v| from_gl(v as u32))
        .ok_or_else(|| PipelineError::Developer(format!("Invalid sampler.{name}.")))
}

/// `GltfLoaderUtil.createSampler`: the texture's glTF sampler, or REPEAT/LINEAR
/// defaults. With `compressed_texture_no_mipmap`, mipmapped minification filters
/// fall back to NEAREST or LINEAR, since compressed textures without mip levels
/// cannot be sampled with them.
pub fn create_sampler(
    gltf: &Value,
    texture_info: &Value,
    compressed_texture_no_mipmap: bool,
) -> Result<Sampler, PipelineError> {
    use TextureMinificationFilter as Min;
    let texture_id = js::get(Some(texture_info), "index").cloned().unwrap_or(Value::Null);
    let texture = js::object(js::index(js::get(Some(gltf), "textures"), &texture_id), "sampler")?;
    let sampler_id = js::get(Some(texture), "sampler");

    let mut wrap_s = TextureWrap::Repeat;
    let mut wrap_t = TextureWrap::Repeat;
    let mut min_filter = Min::Linear;
    let mut mag_filter = TextureMagnificationFilter::Linear;
    if let Some(sampler_id) = sampler_id.filter(|v| !v.is_null()) {
        let sampler = js::object(js::index(js::get(Some(gltf), "samplers"), sampler_id), "wrapS")?;
        let get = |key| js::get(Some(sampler), key);
        wrap_s = sampler_enum(get("wrapS"), wrap_s, "wrapS", TextureWrap::from_gl)?;
        wrap_t = sampler_enum(get("wrapT"), wrap_t, "wrapT", TextureWrap::from_gl)?;
        min_filter = sampler_enum(get("minFilter"), min_filter, "minificationFilter", Min::from_gl)?;
        mag_filter =
            sampler_enum(get("magFilter"), mag_filter, "magnificationFilter", TextureMagnificationFilter::from_gl)?;
    }

    if compressed_texture_no_mipmap && min_filter != Min::Linear && min_filter != Min::Nearest {
        min_filter = match min_filter {
            Min::NearestMipmapNearest | Min::NearestMipmapLinear => Min::Nearest,
            _ => Min::Linear,
        };
    }

    Ok(Sampler {
        wrap_s,
        wrap_t,
        minification_filter: min_filter,
        magnification_filter: mag_filter,
        ..Sampler::default()
    })
}

fn number(value: Option<&Value>) -> Option<f64> {
    value.filter(|v| !v.is_null()).map(|v| js::to_number(Some(v)))
}

/// `Cartesian2.unpack(array)`: missing elements are `undefined`, which arithmetic reads as NaN.
fn unpack2(value: Option<&Value>) -> Option<(f64, f64)> {
    js::defined(value).then(|| (js::to_number(js::get(value, "0")), js::to_number(js::get(value, "1"))))
}

/// `GltfLoaderUtil.createModelTextureReader`. As upstream, the reader's
/// `transform` is `None` (undefined) without `KHR_texture_transform`, and its
/// `scale` is the texture info's `scale`, which may be undefined.
pub fn create_model_texture_reader<G: Gl>(
    texture_info: &Value,
    channels: Option<String>,
    texture: Option<Rc<Texture<G>>>,
) -> TextureReader<G> {
    let (tex_coord, transform) = texture_coordinates(texture_info);
    TextureReader {
        index: number(js::get(Some(texture_info), "index")),
        texture,
        tex_coord,
        scale: number(js::get(Some(texture_info), "scale")),
        transform,
        channels,
    }
}

fn texture_coordinates(texture_info: &Value) -> (f64, Option<Matrix3>) {
    let mut tex_coord = number(js::get(Some(texture_info), "texCoord")).unwrap_or(0.0);
    let mut transform = None;
    let texture_transform = js::get(js::get(Some(texture_info), "extensions"), "KHR_texture_transform");
    if js::defined(texture_transform) {
        let field = |key| js::get(texture_transform, key);
        tex_coord = number(field("texCoord")).unwrap_or(tex_coord);
        let (offset_x, offset_y) = unpack2(field("offset")).unwrap_or((0.0, 0.0));
        let (scale_x, scale_y) = unpack2(field("scale")).unwrap_or((1.0, 1.0));
        // glTF's UV origin is the top left (y-down), so rotations are reversed.
        let rotation = -number(field("rotation")).unwrap_or(0.0);
        let (sin, cos) = rotation.js_sin_cos();
        transform = Some(Matrix3::new(
            cos * scale_x,
            -sin * scale_y,
            offset_x,
            sin * scale_x,
            cos * scale_y,
            offset_y,
            0.0,
            0.0,
            1.0,
        ));
    }
    (tex_coord, transform)
}

/// Texture reader for hosts that expose renderer resources through FrameState.
#[derive(Clone, Debug, PartialEq)]
pub struct ModelTextureReader {
    pub index: Option<usize>,
    pub texture: Option<super::FrameResource>,
    pub tex_coord: u32,
    pub scale: Option<f64>,
    pub transform: Option<Matrix3>,
    pub channels: Option<String>,
}
pub struct GltfLoaderUtil;
impl GltfLoaderUtil {
    pub fn image_id(
        gltf: &Value,
        texture_id: usize,
        formats: SupportedImageFormats,
    ) -> Result<usize, crate::core::RuntimeError> {
        get_image_id_from_texture(gltf, &Value::from(texture_id), formats)
            .map_err(|e| crate::core::RuntimeError::new(e.to_string()))?
            .and_then(Value::as_u64)
            .and_then(|v| usize::try_from(v).ok())
            .ok_or_else(|| crate::core::RuntimeError::new("Invalid glTF image ID"))
    }
    pub fn create_sampler(
        gltf: &Value,
        info: &Value,
        compressed_no_mipmap: bool,
    ) -> Result<Sampler, crate::core::RuntimeError> {
        create_sampler(gltf, info, compressed_no_mipmap).map_err(|e| crate::core::RuntimeError::new(e.to_string()))
    }
    pub fn create_model_texture_reader(
        info: &Value,
        channels: Option<String>,
        texture: Option<super::FrameResource>,
    ) -> Result<ModelTextureReader, crate::core::RuntimeError> {
        let (tex_coord, transform) = texture_coordinates(info);
        Ok(ModelTextureReader {
            index: info["index"].as_u64().map(|v| v as usize),
            texture,
            tex_coord: tex_coord as u32,
            scale: info["scale"].as_f64(),
            transform,
            channels,
        })
    }
}
