//! CesiumJS `SpecularEnvironmentCubeMap`: a KTX2 cube map with packed
//! convoluted mipmaps for physically based specular image-based lighting.
//!
//! Decoding/transport belongs to the host. [`SpecularEnvironmentCubeMap`]
//! validates support, installs decoded KTX2 levels, and exposes the ready
//! state that `ImageBasedLighting` consumes.

use super::frame_state::FrameState;
use crate::core::{
    ktx2_transcoder::{CubeFace, Ktx2Texture},
    Event, PixelDatatype, PixelFormat, RuntimeError,
};
use crate::renderer::{
    Context, ContextCapabilities, CubeMap, Gl, Sampler, TextureMinificationFilter, TextureOptions, TextureSource,
};
use std::rc::Rc;

/// A cube map used as a specular environment map.
pub struct SpecularEnvironmentCubeMap<G: Gl> {
    url: String,
    cube_map_buffers: Option<Ktx2Texture>,
    texture: Option<Rc<CubeMap<G>>>,
    maximum_mipmap_level: Option<u32>,
    loading: bool,
    ready: bool,
    error_event: Event<RuntimeError>,
    destroyed: bool,
}

impl<G: Gl + 'static> SpecularEnvironmentCubeMap<G> {
    pub fn new(url: impl Into<String>) -> Self {
        Self {
            url: url.into(),
            cube_map_buffers: None,
            texture: None,
            maximum_mipmap_level: None,
            loading: false,
            ready: false,
            error_event: Event::new(),
            destroyed: false,
        }
    }

    /// The KTX2 URL.
    pub fn url(&self) -> &str {
        &self.url
    }

    /// Raised when an asynchronous load fails.
    pub fn error_event(&mut self) -> &mut Event<RuntimeError> {
        &mut self.error_event
    }

    /// The loaded cube map.
    pub fn texture(&self) -> Option<&Rc<CubeMap<G>>> {
        self.texture.as_ref()
    }

    /// The highest mip level with valid environment-map data.
    pub fn maximum_mipmap_level(&self) -> Option<u32> {
        self.maximum_mipmap_level
    }

    pub fn ready(&self) -> bool {
        self.ready
    }

    pub fn is_loading(&self) -> bool {
        self.loading
    }

    /// `SpecularEnvironmentCubeMap.isSupported`.
    pub fn is_supported(context: &ContextCapabilities) -> bool {
        let float_buffers_and_textures = (context.color_buffer_half_float && context.half_floating_point_texture)
            || (context.floating_point_texture && context.color_buffer_float);
        float_buffers_and_textures && context.supports_texture_lod
    }

    /// Records that a KTX2 request started. A second `update` while loading
    /// returns without starting another request.
    pub fn begin_loading(&mut self) -> bool {
        if self.loading || self.texture.is_some() {
            return false;
        }
        self.loading = true;
        true
    }

    /// Reports an asynchronous load failure through `errorEvent`.
    pub fn report_load_error(&mut self, error: RuntimeError) {
        self.loading = false;
        if !self.destroyed {
            self.error_event.raise_event(&error);
        }
    }

    /// Stores decoded buffers; the next `update` uploads them.
    pub fn set_cube_map_buffers(&mut self, buffers: Ktx2Texture) {
        self.cube_map_buffers = Some(buffers);
        self.loading = false;
    }

    /// `SpecularEnvironmentCubeMap.prototype.update`.
    pub fn update(&mut self, frame: &FrameState) -> Result<(), RuntimeError> {
        if self.destroyed {
            return Err(RuntimeError::new("This object was destroyed, i.e., destroy() was called."));
        }
        let Some(context) = frame.context.as_ref().and_then(|resource| resource.get::<Context<G>>()).cloned() else {
            return Ok(());
        };
        if !Self::is_supported(&context.capabilities()?) {
            return Ok(());
        }
        if self.texture.is_some() {
            self.cube_map_buffers = None;
            return Ok(());
        }
        let Some(buffers) = self.cube_map_buffers.take() else {
            return Ok(());
        };
        self.upload(&context, buffers)
    }

    /// Installs decoded KTX2 levels immediately.
    pub fn upload(&mut self, context: &Context<G>, buffers: Ktx2Texture) -> Result<(), RuntimeError> {
        let levels = match buffers {
            Ktx2Texture::Cubemap(levels) => levels,
            _ => return Err(RuntimeError::new("Specular environment maps must be KTX2 cube maps.")),
        };
        let first = levels
            .first()
            .and_then(|level| level.first())
            .ok_or_else(|| RuntimeError::new("KTX2 cube map has no faces."))?;
        let buffer = &first.1;
        let pixel_datatype = buffer.pixel_datatype.and_then(PixelDatatype::from_value).unwrap_or(
            if context.capabilities()?.half_floating_point_texture {
                PixelDatatype::HalfFloat
            } else {
                PixelDatatype::Float
            },
        );
        let pixel_format = PixelFormat::Rgba;
        let mip_levels = levels.len() as u32;
        self.maximum_mipmap_level = Some(mip_levels.saturating_sub(1));

        let face_size = buffer.width;
        let expected_mip_levels = face_size.ilog2() + 1;
        let mut levels = levels;
        if mip_levels != expected_mip_levels {
            // Fill remaining levels with null faces so the cube map has a full
            // chain; Cesium treats the last supplied level as roughness 1.0.
            let empty: Vec<(CubeFace, crate::core::CompressedTextureBuffer)> = vec![];
            for _ in mip_levels..expected_mip_levels {
                levels.push(empty.clone());
            }
        }

        let to_source = |buffer: &crate::core::CompressedTextureBuffer| TextureSource::Pixels {
            width: buffer.width,
            height: buffer.height,
            bytes: buffer.buffer_view().to_vec(),
        };
        let first_level: Vec<(CubeFace, TextureSource<G>)> =
            levels[0].iter().map(|(face, buffer)| (*face, to_source(buffer))).collect();
        let mut ordered: [Option<TextureSource<G>>; 6] = [None, None, None, None, None, None];
        for (face, source) in first_level {
            ordered[face as usize] = Some(source);
        }
        let [positive_x, negative_x, positive_y, negative_y, positive_z, negative_z] = ordered;
        let sources = [
            positive_x.ok_or_else(|| RuntimeError::new("KTX2 cube map is missing a face."))?,
            negative_x.ok_or_else(|| RuntimeError::new("KTX2 cube map is missing a face."))?,
            positive_y.ok_or_else(|| RuntimeError::new("KTX2 cube map is missing a face."))?,
            negative_y.ok_or_else(|| RuntimeError::new("KTX2 cube map is missing a face."))?,
            positive_z.ok_or_else(|| RuntimeError::new("KTX2 cube map is missing a face."))?,
            negative_z.ok_or_else(|| RuntimeError::new("KTX2 cube map is missing a face."))?,
        ];

        let mut options = TextureOptions::rgba(face_size, face_size);
        options.flip_y = false;
        options.pixel_format = pixel_format;
        options.pixel_datatype = pixel_datatype;
        options.sampler =
            Sampler { minification_filter: TextureMinificationFilter::LinearMipmapLinear, ..Default::default() };
        let cube_map = CubeMap::new(context, options, Some(sources))?;
        if levels.len() > 1 {
            let mip_sources: Vec<[TextureSource<G>; 6]> = levels
                .iter()
                .skip(1)
                .map(|level| {
                    let mut ordered: [Option<TextureSource<G>>; 6] = [None, None, None, None, None, None];
                    for (face, buffer) in level {
                        ordered[*face as usize] = Some(to_source(buffer));
                    }
                    Ok([
                        ordered[0].take().ok_or_else(|| RuntimeError::new("KTX2 mip level is missing a face."))?,
                        ordered[1].take().ok_or_else(|| RuntimeError::new("KTX2 mip level is missing a face."))?,
                        ordered[2].take().ok_or_else(|| RuntimeError::new("KTX2 mip level is missing a face."))?,
                        ordered[3].take().ok_or_else(|| RuntimeError::new("KTX2 mip level is missing a face."))?,
                        ordered[4].take().ok_or_else(|| RuntimeError::new("KTX2 mip level is missing a face."))?,
                        ordered[5].take().ok_or_else(|| RuntimeError::new("KTX2 mip level is missing a face."))?,
                    ])
                })
                .collect::<Result<Vec<_>, RuntimeError>>()?;
            cube_map.load_mipmaps(&mip_sources, false)?;
        }
        self.texture = Some(Rc::new(cube_map));
        self.ready = true;
        Ok(())
    }

    pub fn is_destroyed(&self) -> bool {
        self.destroyed
    }

    pub fn destroy(&mut self) -> Result<(), RuntimeError> {
        self.cube_map_buffers = None;
        if let Some(texture) = self.texture.take() {
            texture.destroy()?;
        }
        self.destroyed = true;
        Ok(())
    }
}
