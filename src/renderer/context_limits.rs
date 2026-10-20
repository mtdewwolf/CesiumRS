//! Context-local CesiumJS ContextLimits and extension capabilities.
use super::{context::*, ShaderCapabilities};
use crate::core::webgl_constants as gl;
use serde::{Deserialize, Serialize};
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextLimits {
    pub maximum_combined_texture_image_units: u32,
    pub maximum_cube_map_size: u32,
    pub maximum_fragment_uniform_vectors: u32,
    pub maximum_texture_image_units: u32,
    pub maximum_renderbuffer_size: u32,
    pub maximum_texture_size: u32,
    #[serde(rename = "maximum3DTextureSize")]
    pub maximum_3d_texture_size: u32,
    pub maximum_varying_vectors: u32,
    pub maximum_vertex_attributes: u32,
    pub maximum_vertex_texture_image_units: u32,
    pub maximum_vertex_uniform_vectors: u32,
    pub minimum_aliased_line_width: f64,
    pub maximum_aliased_line_width: f64,
    pub minimum_aliased_point_size: f64,
    pub maximum_aliased_point_size: f64,
    pub maximum_viewport_width: u32,
    pub maximum_viewport_height: u32,
    pub maximum_texture_filter_anisotropy: f64,
    pub maximum_draw_buffers: u32,
    pub maximum_color_attachments: u32,
    pub maximum_samples: u32,
    pub highp_float_supported: bool,
    pub highp_int_supported: bool,
}
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ContextCapabilities {
    pub webgl2: bool,
    pub floating_point_texture: bool,
    pub half_floating_point_texture: bool,
    pub texture_float_linear: bool,
    pub texture_half_float_linear: bool,
    pub color_buffer_float: bool,
    pub color_buffer_half_float: bool,
    pub depth_texture: bool,
    pub element_index_uint: bool,
    pub instanced_arrays: bool,
    pub vertex_array_object: bool,
    pub draw_buffers: bool,
    pub s3tc: bool,
    pub pvrtc: bool,
    pub astc: bool,
    pub etc: bool,
    pub etc1: bool,
    pub bc7: bool,
    pub anisotropic: bool,
}
impl<G: Gl> Context<G> {
    pub fn extension_supported(&self, name: &str) -> Result<bool> {
        Ok(!matches!(self.call("getExtension", &[name.into()])?, GlValue::Null))
    }
    pub fn capabilities(&self) -> Result<ContextCapabilities> {
        let color = self.extension_supported("EXT_color_buffer_float")?;
        Ok(ContextCapabilities {
            webgl2: true,
            floating_point_texture: true,
            half_floating_point_texture: true,
            texture_float_linear: self.extension_supported("OES_texture_float_linear")?,
            texture_half_float_linear: true,
            color_buffer_float: color,
            color_buffer_half_float: color,
            depth_texture: true,
            element_index_uint: true,
            instanced_arrays: true,
            vertex_array_object: true,
            draw_buffers: true,
            s3tc: self.extension_supported("WEBGL_compressed_texture_s3tc")?,
            pvrtc: self.extension_supported("WEBGL_compressed_texture_pvrtc")?,
            astc: self.extension_supported("WEBGL_compressed_texture_astc")?,
            etc: true,
            etc1: self.extension_supported("WEBGL_compressed_texture_etc1")?,
            bc7: self.extension_supported("EXT_texture_compression_bptc")?,
            anisotropic: self.extension_supported("EXT_texture_filter_anisotropic")?,
        })
    }
    pub fn shader_capabilities(&self) -> Result<ShaderCapabilities> {
        Ok(ShaderCapabilities {
            floating_point_texture: true,
            texture_float_linear: self.extension_supported("OES_texture_float_linear")?,
        })
    }
    pub fn limits(&self) -> Result<ContextLimits> {
        let number = |p: u32| -> Result<u32> {
            match self.call("getParameter", &[p.into()])? {
                GlValue::Number(v) => Ok(v as u32),
                _ => Err(crate::core::RuntimeError::new("Invalid context limit from host")),
            }
        };
        let pair = |p: u32| -> Result<[f64; 2]> {
            match self.call("getParameter", &[p.into()])? {
                GlValue::Numbers(v) if v.len() == 2 => Ok([v[0], v[1]]),
                _ => Err(crate::core::RuntimeError::new("Invalid vector context limit from host")),
            }
        };
        let precision = |datatype| -> Result<bool> {
            match self.call("getShaderPrecisionFormat", &[gl::FRAGMENT_SHADER.into(), datatype])? {
                GlValue::Record(v) => {
                    Ok(v["precision"].as_f64().unwrap_or(0.) > 0. || v["rangeMax"].as_f64().unwrap_or(0.) > 0.)
                }
                _ => Ok(false),
            }
        };
        let line = pair(gl::ALIASED_LINE_WIDTH_RANGE)?;
        let point = pair(gl::ALIASED_POINT_SIZE_RANGE)?;
        let viewport = pair(gl::MAX_VIEWPORT_DIMS)?;
        let anisotropy =
            if self.extension_supported("EXT_texture_filter_anisotropic")? { number(0x84ff)? as f64 } else { 1. };
        Ok(ContextLimits {
            maximum_combined_texture_image_units: number(gl::MAX_COMBINED_TEXTURE_IMAGE_UNITS)?,
            maximum_cube_map_size: number(gl::MAX_CUBE_MAP_TEXTURE_SIZE)?,
            maximum_fragment_uniform_vectors: number(gl::MAX_FRAGMENT_UNIFORM_VECTORS)?,
            maximum_texture_image_units: number(gl::MAX_TEXTURE_IMAGE_UNITS)?,
            maximum_renderbuffer_size: number(gl::MAX_RENDERBUFFER_SIZE)?,
            maximum_texture_size: number(gl::MAX_TEXTURE_SIZE)?,
            maximum_3d_texture_size: number(gl::MAX_3D_TEXTURE_SIZE)?,
            maximum_varying_vectors: number(gl::MAX_VARYING_VECTORS)?,
            maximum_vertex_attributes: number(gl::MAX_VERTEX_ATTRIBS)?,
            maximum_vertex_texture_image_units: number(gl::MAX_VERTEX_TEXTURE_IMAGE_UNITS)?,
            maximum_vertex_uniform_vectors: number(gl::MAX_VERTEX_UNIFORM_VECTORS)?,
            minimum_aliased_line_width: line[0],
            maximum_aliased_line_width: line[1],
            minimum_aliased_point_size: point[0],
            maximum_aliased_point_size: point[1],
            maximum_viewport_width: viewport[0] as u32,
            maximum_viewport_height: viewport[1] as u32,
            maximum_texture_filter_anisotropy: anisotropy,
            maximum_draw_buffers: number(gl::MAX_DRAW_BUFFERS)?,
            maximum_color_attachments: number(gl::MAX_COLOR_ATTACHMENTS)?,
            maximum_samples: number(gl::MAX_SAMPLES)?,
            highp_float_supported: precision(gl::HIGH_FLOAT.into())?,
            highp_int_supported: precision(gl::HIGH_INT.into())?,
        })
    }
    pub fn begin_frame(&self) {}
    pub fn end_frame(&self) -> Result<()> {
        self.call("useProgram", &[GlValue::Null])?;
        self.bind_framebuffer(None)?;
        for unit in 0..self.frame_texture_units.get() {
            self.call("activeTexture", &[(gl::TEXTURE0 + unit).into()])?;
            self.call("bindTexture", &[gl::TEXTURE_2D.into(), GlValue::Null])?;
            self.call("bindTexture", &[gl::TEXTURE_CUBE_MAP.into(), GlValue::Null])?;
        }
        self.frame_texture_units.set(0);
        Ok(())
    }
}
