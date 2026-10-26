//! CesiumJS `Scene/Model/TextureUniform`: the description of an image- or
//! typed-array-backed `sampler2D` value used by [`super::CustomShader`] and
//! [`super::TextureManager`].

use crate::{
    core::{PixelDatatype, PixelFormat, Resource, RuntimeError},
    renderer::{Sampler, TextureMagnificationFilter, TextureMinificationFilter, TextureWrap},
};
use std::rc::Rc;

/// Options accepted by [`TextureUniform::new`].
#[derive(Clone, Debug, Default)]
pub struct TextureUniformOptions {
    /// Row-major texture contents. Exactly one of `typed_array` and `url` is
    /// required, matching the upstream developer-error contract.
    pub typed_array: Option<Vec<u8>>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub url: Option<String>,
    /// Defaults to `true`; selects `REPEAT` (not clamped) wrapping.
    pub repeat: Option<bool>,
    pub pixel_format: Option<PixelFormat>,
    pub pixel_datatype: Option<PixelDatatype>,
    pub minification_filter: Option<TextureMinificationFilter>,
    pub magnification_filter: Option<TextureMagnificationFilter>,
    pub maximum_anisotropy: Option<f64>,
}

/// A sampler uniform value. The URL-backed form keeps the parsed [`Resource`]
/// so a host can fetch it with the same credentials and proxy rules as the
/// rest of the Scene ports.
#[derive(Clone)]
pub struct TextureUniform {
    pub typed_array: Option<Vec<u8>>,
    pub width: Option<u32>,
    pub height: Option<u32>,
    pub pixel_format: PixelFormat,
    pub pixel_datatype: PixelDatatype,
    pub resource: Option<Rc<Resource>>,
    pub sampler: Sampler,
}

impl std::fmt::Debug for TextureUniform {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TextureUniform")
            .field("typed_array_len", &self.typed_array.as_ref().map(Vec::len))
            .field("width", &self.width)
            .field("height", &self.height)
            .field("pixel_format", &self.pixel_format)
            .field("pixel_datatype", &self.pixel_datatype)
            .field("url", &self.resource.as_ref().map(|r| r.url()))
            .field("sampler", &self.sampler)
            .finish()
    }
}

impl TextureUniform {
    pub fn new(options: TextureUniformOptions) -> Result<Self, RuntimeError> {
        let has_typed_array = options.typed_array.is_some();
        let has_url = options.url.is_some();
        if has_typed_array == has_url {
            return Err(RuntimeError::new("exactly one of options.typedArray, options.url must be defined"));
        }
        if has_typed_array && (options.width.is_none() || options.height.is_none()) {
            return Err(RuntimeError::new(
                "options.width and options.height are required when options.typedArray is defined",
            ));
        }
        let repeat = options.repeat.unwrap_or(true);
        let wrap = if repeat { TextureWrap::Repeat } else { TextureWrap::ClampToEdge };
        let sampler = Sampler {
            wrap_r: TextureWrap::ClampToEdge,
            wrap_s: wrap,
            wrap_t: wrap,
            minification_filter: options.minification_filter.unwrap_or(TextureMinificationFilter::Linear),
            magnification_filter: options.magnification_filter.unwrap_or(TextureMagnificationFilter::Linear),
            maximum_anisotropy: options.maximum_anisotropy.unwrap_or(1.),
        };
        let resource = options
            .url
            .map(|url| Resource::new(url).map(Rc::new).ok_or_else(|| RuntimeError::new("Invalid texture uniform URL")))
            .transpose()?;
        Ok(Self {
            typed_array: options.typed_array,
            width: options.width,
            height: options.height,
            pixel_format: options.pixel_format.unwrap_or(PixelFormat::Rgba),
            pixel_datatype: options.pixel_datatype.unwrap_or(PixelDatatype::UnsignedByte),
            resource,
            sampler,
        })
    }

    /// A typed-array texture, the form used by fixtures and CPU-generated data.
    pub fn from_typed_array(
        pixels: Vec<u8>,
        width: u32,
        height: u32,
        pixel_format: PixelFormat,
        pixel_datatype: PixelDatatype,
        sampler: Sampler,
    ) -> Self {
        Self {
            typed_array: Some(pixels),
            width: Some(width),
            height: Some(height),
            pixel_format,
            pixel_datatype,
            resource: None,
            sampler,
        }
    }

    /// A URL-backed texture.
    pub fn from_url(url: &str, sampler: Sampler) -> Result<Self, RuntimeError> {
        Ok(Self {
            typed_array: None,
            width: None,
            height: None,
            pixel_format: PixelFormat::Rgba,
            pixel_datatype: PixelDatatype::UnsignedByte,
            resource: Some(Rc::new(
                Resource::new(url).ok_or_else(|| RuntimeError::new("Invalid texture uniform URL"))?,
            )),
            sampler,
        })
    }

    /// The sampler repeated in both directions, the default for custom shaders.
    pub fn default_sampler() -> Sampler {
        Sampler {
            wrap_r: TextureWrap::ClampToEdge,
            wrap_s: TextureWrap::Repeat,
            wrap_t: TextureWrap::Repeat,
            ..Sampler::default()
        }
    }
}
