//! glTF image selection, sampler defaults and KHR_texture_transform readers.
use super::{FrameResource, SupportedImageFormats};
use crate::{
    core::{js_math::JsMath, Matrix3, RuntimeError},
    renderer::{Sampler, TextureMagnificationFilter as Mag, TextureMinificationFilter as Min, TextureWrap},
};
use serde_json::Value;
#[derive(Clone, Debug, PartialEq)]
pub struct ModelTextureReader {
    pub index: Option<usize>,
    pub texture: Option<FrameResource>,
    pub tex_coord: u32,
    pub scale: Option<f64>,
    pub transform: Option<Matrix3>,
    pub channels: Option<String>,
}
pub struct GltfLoaderUtil;
impl GltfLoaderUtil {
    pub fn image_id(gltf: &Value, texture_id: usize, formats: SupportedImageFormats) -> Result<usize, RuntimeError> {
        let texture = gltf["textures"].get(texture_id).ok_or_else(|| RuntimeError::new("Invalid glTF texture ID"))?;
        let source = if formats.webp && !texture["extensions"]["EXT_texture_webp"].is_null() {
            &texture["extensions"]["EXT_texture_webp"]["source"]
        } else if formats.basis && !texture["extensions"]["KHR_texture_basisu"].is_null() {
            &texture["extensions"]["KHR_texture_basisu"]["source"]
        } else {
            &texture["source"]
        };
        source.as_u64().and_then(|v| usize::try_from(v).ok()).ok_or_else(|| RuntimeError::new("Invalid glTF image ID"))
    }
    pub fn create_sampler(gltf: &Value, info: &Value, compressed_no_mipmap: bool) -> Result<Sampler, RuntimeError> {
        let id = info["index"].as_u64().ok_or_else(|| RuntimeError::new("Invalid glTF texture index"))? as usize;
        let texture = gltf["textures"].get(id).ok_or_else(|| RuntimeError::new("Invalid glTF texture ID"))?;
        let source =
            texture["sampler"].as_u64().and_then(|id| gltf["samplers"].get(id as usize)).unwrap_or(&Value::Null);
        let wrap = |key: &str| {
            TextureWrap::from_gl(source[key].as_u64().unwrap_or(10497) as u32)
                .ok_or_else(|| RuntimeError::new("Invalid glTF texture wrap"))
        };
        let mut min = Min::from_gl(source["minFilter"].as_u64().unwrap_or(9729) as u32)
            .ok_or_else(|| RuntimeError::new("Invalid glTF minification filter"))?;
        if compressed_no_mipmap && !matches!(min, Min::Linear | Min::Nearest) {
            min = if matches!(min, Min::NearestMipmapNearest | Min::NearestMipmapLinear) {
                Min::Nearest
            } else {
                Min::Linear
            };
        }
        Ok(Sampler {
            wrap_s: wrap("wrapS")?,
            wrap_t: wrap("wrapT")?,
            minification_filter: min,
            magnification_filter: Mag::from_gl(source["magFilter"].as_u64().unwrap_or(9729) as u32)
                .ok_or_else(|| RuntimeError::new("Invalid glTF magnification filter"))?,
            ..Default::default()
        })
    }
    pub fn create_model_texture_reader(
        info: &Value,
        channels: Option<String>,
        texture: Option<FrameResource>,
    ) -> Result<ModelTextureReader, RuntimeError> {
        let mut tex_coord = info["texCoord"].as_u64().unwrap_or(0) as u32;
        let transform = info
            .pointer("/extensions/KHR_texture_transform")
            .filter(|v| !v.is_null())
            .map(|t| {
                tex_coord = t["texCoord"].as_u64().map_or(tex_coord, |v| v as u32);
                let vector = |key: &str, default: f64| -> Result<[f64; 2], RuntimeError> {
                    if t[key].is_null() {
                        Ok([default; 2])
                    } else {
                        Ok([
                            t[key][0].as_f64().ok_or_else(|| RuntimeError::new("Invalid texture transform vector"))?,
                            t[key][1].as_f64().ok_or_else(|| RuntimeError::new("Invalid texture transform vector"))?,
                        ])
                    }
                };
                let offset = vector("offset", 0.)?;
                let scale = vector("scale", 1.)?;
                let rotation = -t["rotation"].as_f64().unwrap_or(0.);
                let (c, s) = (rotation.js_cos(), rotation.js_sin());
                Ok(Matrix3::new(
                    c * scale[0],
                    -s * scale[1],
                    offset[0],
                    s * scale[0],
                    c * scale[1],
                    offset[1],
                    0.,
                    0.,
                    1.,
                ))
            })
            .transpose()?;
        Ok(ModelTextureReader {
            index: info["index"].as_u64().map(|v| v as usize),
            texture,
            tex_coord,
            scale: info["scale"].as_f64(),
            transform,
            channels,
        })
    }
}
