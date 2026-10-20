//! CesiumJS Texture's uncompressed 2D typed-pixel path and Sampler.
//! Pixel bytes use little-endian elements; row reversal is performed in Rust,
//! because WebGL ignores UNPACK_FLIP_Y_WEBGL for typed-array uploads.
use super::{context::*, MipmapHint, TextureMagnificationFilter as Mag, TextureMinificationFilter as Min, TextureWrap};
use crate::core::{webgl_constants as gl, PixelDatatype, PixelFormat};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Sampler {
    pub wrap_r: TextureWrap,
    pub wrap_s: TextureWrap,
    pub wrap_t: TextureWrap,
    pub minification_filter: Min,
    pub magnification_filter: Mag,
    pub maximum_anisotropy: f64,
}
impl Default for Sampler {
    fn default() -> Self {
        Self {
            wrap_r: TextureWrap::ClampToEdge,
            wrap_s: TextureWrap::ClampToEdge,
            wrap_t: TextureWrap::ClampToEdge,
            minification_filter: Min::Linear,
            magnification_filter: Mag::Linear,
            maximum_anisotropy: 1.,
        }
    }
}
impl Sampler {
    pub const NEAREST: Self = Self {
        wrap_r: TextureWrap::ClampToEdge,
        wrap_s: TextureWrap::ClampToEdge,
        wrap_t: TextureWrap::ClampToEdge,
        minification_filter: Min::Nearest,
        magnification_filter: Mag::Nearest,
        maximum_anisotropy: 1.,
    };
    pub fn validate(&self) -> Result<()> {
        ensure(
            self.maximum_anisotropy.is_finite() && self.maximum_anisotropy >= 1.,
            "Sampler anisotropy must be at least one",
        )
    }
}
#[derive(Clone, Debug)]
pub struct TextureOptions {
    pub width: u32,
    pub height: u32,
    pub pixel_format: PixelFormat,
    pub pixel_datatype: PixelDatatype,
    pub flip_y: bool,
    pub sampler: Sampler,
}
impl TextureOptions {
    pub fn rgba(width: u32, height: u32) -> Self {
        Self {
            width,
            height,
            pixel_format: PixelFormat::Rgba,
            pixel_datatype: PixelDatatype::UnsignedByte,
            flip_y: true,
            sampler: Sampler::default(),
        }
    }
}
pub struct Texture<G: Gl> {
    pub(crate) resource: Rc<Resource<G>>,
    options: TextureOptions,
    sampler: RefCell<Sampler>,
    has_mipmap: Cell<bool>,
}
impl<G: Gl> Texture<G> {
    pub fn new(context: &Context<G>, options: TextureOptions, pixels: Option<&[u8]>) -> Result<Self> {
        ensure(options.width > 0 && options.height > 0, "Texture dimensions must be positive")?;
        size::<G::Object>(options.width as usize)?;
        size::<G::Object>(options.height as usize)?;
        options.sampler.validate()?;
        // These are exactly the format/type combinations supported by this
        // typed-pixel foundation. Legacy DOM and compressed uploads come later.
        use PixelDatatype as D;
        use PixelFormat as F;
        let supported = match options.pixel_format {
            F::Rgba => matches!(
                options.pixel_datatype,
                D::UnsignedByte | D::Float | D::HalfFloat | D::UnsignedShort4_4_4_4 | D::UnsignedShort5_5_5_1
            ),
            F::Rgb => {
                matches!(options.pixel_datatype, D::UnsignedByte | D::Float | D::HalfFloat | D::UnsignedShort5_6_5)
            }
            F::Red | F::Rg => matches!(options.pixel_datatype, D::UnsignedByte | D::Float | D::HalfFloat),
            F::DepthComponent => {
                matches!(options.pixel_datatype, D::UnsignedShort | D::UnsignedInt) && pixels.is_none()
            }
            F::DepthStencil => options.pixel_datatype == D::UnsignedInt24_8 && pixels.is_none(),
            _ => false,
        };
        ensure(
            supported,
            "Unsupported texture format/type or source; use uncompressed WebGL2 color or blank depth textures",
        )?;
        let bytes = byte_length(&options, options.width, options.height)?;
        if pixels.is_some() && options.flip_y && options.height > 1 && options.pixel_datatype.is_packed() {
            let expanded = bytes
                .checked_mul(options.pixel_format.components_length())
                .ok_or_else(|| crate::core::RuntimeError::new("Packed texture flip buffer overflow"))?;
            size::<G::Object>(expanded)?;
        }
        if let Some(p) = pixels {
            ensure(p.len() == bytes, "Texture source byte length does not match its dimensions")?;
        }
        if let GlValue::Number(max) = context.call("getParameter", &[gl::MAX_TEXTURE_SIZE.into()])? {
            ensure(
                options.width as f64 <= max && options.height as f64 <= max,
                "Texture dimensions exceed MAX_TEXTURE_SIZE",
            )?;
        }
        let sampler = options.sampler;
        let resource = Rc::new(Resource::create(context, "createTexture", &[], "deleteTexture")?);
        let result = Self { resource, options, sampler: RefCell::new(sampler), has_mipmap: Cell::new(false) };
        result.bind(0)?;
        let upload = (|| {
            result.pixel_store(result.options.width)?;
            context.call(
                "texImage2D",
                &[
                    gl::TEXTURE_2D.into(),
                    0.into(),
                    result.options.pixel_format.to_internal_format(result.options.pixel_datatype, true).into(),
                    result.options.width.into(),
                    result.options.height.into(),
                    0.into(),
                    (result.options.pixel_format as u32).into(),
                    result.options.pixel_datatype.to_webgl_constant(true).into(),
                    pixels
                        .map(|p| result.pixels(p, result.options.width, result.options.height))
                        .unwrap_or(GlValue::Null),
                ],
            )?;
            Ok(())
        })();
        context.call("bindTexture", &[gl::TEXTURE_2D.into(), GlValue::Null])?;
        upload?;
        result.set_sampler(sampler)?;
        Ok(result)
    }
    fn pixel_store(&self, width: u32) -> Result<()> {
        let c = &self.resource.context;
        c.call(
            "pixelStorei",
            &[
                gl::UNPACK_ALIGNMENT.into(),
                self.options.pixel_format.alignment_in_bytes(self.options.pixel_datatype, width as f64).into(),
            ],
        )?;
        c.call("pixelStorei", &[gl::UNPACK_PREMULTIPLY_ALPHA_WEBGL.into(), false.into()])?;
        c.call("pixelStorei", &[gl::UNPACK_FLIP_Y_WEBGL.into(), false.into()])?;
        // An externally bound PBO changes the overload/meaning of texture data.
        c.call("bindBuffer", &[gl::PIXEL_UNPACK_BUFFER.into(), GlValue::Null])?;
        Ok(())
    }
    fn pixels(&self, bytes: &[u8], width: u32, height: u32) -> GlValue<G::Object> {
        let mut bytes = bytes.to_vec();
        if self.options.flip_y && height > 1 {
            // PixelFormat.flipY counts *components* even for packed ushort
            // pixels. Preserve that Cesium quirk: absent input elements become
            // zero and the temporary upload is larger than the packed image.
            let row = width as usize
                * self.options.pixel_format.components_length()
                * self.options.pixel_datatype.size_in_bytes();
            bytes.resize(row * height as usize, 0);
            for y in 0..height as usize / 2 {
                let (left, right) = bytes.split_at_mut((height as usize - y - 1) * row);
                left[y * row..(y + 1) * row].swap_with_slice(&mut right[..row]);
            }
        }
        GlValue::Pixels { bytes, datatype: self.options.pixel_datatype }
    }
    pub fn width(&self) -> u32 {
        self.options.width
    }
    pub fn height(&self) -> u32 {
        self.options.height
    }
    pub fn pixel_format(&self) -> PixelFormat {
        self.options.pixel_format
    }
    pub fn pixel_datatype(&self) -> PixelDatatype {
        self.options.pixel_datatype
    }
    pub fn flip_y(&self) -> bool {
        self.options.flip_y
    }
    pub fn size_in_bytes(&self) -> usize {
        let size = byte_length(&self.options, self.width(), self.height()).unwrap();
        if self.has_mipmap.get() {
            (size as u64 * 4 / 3) as usize
        } else {
            size
        }
    }
    pub fn sampler(&self) -> Sampler {
        *self.sampler.borrow()
    }
    pub fn set_sampler(&self, sampler: Sampler) -> Result<()> {
        sampler.validate()?;
        let c = &self.resource.context;
        let mut min = sampler.minification_filter as u32;
        let mut mag = sampler.magnification_filter as u32;
        if self.options.pixel_datatype == PixelDatatype::Float
            && matches!(c.call("getExtension", &["OES_texture_float_linear".into()])?, GlValue::Null)
        {
            min = if matches!(sampler.minification_filter, Min::Nearest | Min::Linear) {
                gl::NEAREST
            } else {
                gl::NEAREST_MIPMAP_NEAREST
            };
            mag = gl::NEAREST;
        }
        // Half-float linear filtering is core in WebGL2.
        if self.options.pixel_format.is_depth_format() {
            min = gl::NEAREST;
            mag = gl::NEAREST;
        }
        self.bind(0)?;
        let result = (|| {
            for (pname, value) in [
                (gl::TEXTURE_MIN_FILTER, min),
                (gl::TEXTURE_MAG_FILTER, mag),
                (gl::TEXTURE_WRAP_S, sampler.wrap_s as u32),
                (gl::TEXTURE_WRAP_T, sampler.wrap_t as u32),
            ] {
                c.call("texParameteri", &[gl::TEXTURE_2D.into(), pname.into(), value.into()])?;
            }
            if !matches!(c.call("getExtension", &["EXT_texture_filter_anisotropic".into()])?, GlValue::Null) {
                c.call(
                    "texParameterf",
                    &[gl::TEXTURE_2D.into(), 0x84FE_u32.into(), sampler.maximum_anisotropy.into()],
                )?;
            }
            Ok(())
        })();
        c.call("bindTexture", &[gl::TEXTURE_2D.into(), GlValue::Null])?;
        result?;
        *self.sampler.borrow_mut() = sampler;
        Ok(())
    }
    pub fn bind(&self, unit: u32) -> Result<()> {
        let value = self.resource.value()?;
        let c = &self.resource.context;
        if let GlValue::Number(max) = c.call("getParameter", &[gl::MAX_COMBINED_TEXTURE_IMAGE_UNITS.into()])? {
            ensure((unit as f64) < max, "Texture unit out of range")?;
        }
        c.call("activeTexture", &[(gl::TEXTURE0 + unit).into()])?;
        c.call("bindTexture", &[gl::TEXTURE_2D.into(), value])?;
        Ok(())
    }
    pub fn copy_from(&self, pixels: &[u8], region: TextureRegion) -> Result<()> {
        ensure(!self.options.pixel_format.is_depth_format(), "Cannot upload pixels to depth textures")?;
        range(region.x as usize, region.width as usize, self.width() as usize)?;
        range(region.y as usize, region.height as usize, self.height() as usize)?;
        ensure(
            region.width > 0
                && region.height > 0
                && pixels.len() == byte_length(&self.options, region.width, region.height)?,
            "Invalid texture subimage byte length or dimensions",
        )?;
        let c = &self.resource.context;
        self.bind(0)?;
        let result = (|| {
            self.pixel_store(region.width)?;
            c.call(
                "texSubImage2D",
                &[
                    gl::TEXTURE_2D.into(),
                    0.into(),
                    region.x.into(),
                    region.y.into(),
                    region.width.into(),
                    region.height.into(),
                    (self.options.pixel_format as u32).into(),
                    self.options.pixel_datatype.to_webgl_constant(true).into(),
                    self.pixels(pixels, region.width, region.height),
                ],
            )
        })();
        c.call("bindTexture", &[gl::TEXTURE_2D.into(), GlValue::Null])?;
        result.map(|_| ())
    }
    pub fn generate_mipmap(&self, hint: MipmapHint) -> Result<()> {
        ensure(!self.options.pixel_format.is_depth_format(), "Cannot mipmap a depth texture")?;
        self.bind(0)?;
        let c = &self.resource.context;
        let result = (|| {
            c.call("hint", &[gl::GENERATE_MIPMAP_HINT.into(), (hint as u32).into()])?;
            c.call("generateMipmap", &[gl::TEXTURE_2D.into()])
        })();
        c.call("bindTexture", &[gl::TEXTURE_2D.into(), GlValue::Null])?;
        result?;
        self.has_mipmap.set(true);
        Ok(())
    }
    pub fn is_destroyed(&self) -> bool {
        self.resource.is_destroyed()
    }
    pub fn destroy(&self) -> Result<()> {
        self.resource.destroy()
    }
}
fn byte_length(options: &TextureOptions, width: u32, height: u32) -> Result<usize> {
    let components = if options.pixel_datatype.is_packed() { 1 } else { options.pixel_format.components_length() };
    let length = (width as usize)
        .checked_mul(height as usize)
        .and_then(|n| n.checked_mul(components))
        .and_then(|n| n.checked_mul(options.pixel_datatype.size_in_bytes()));
    let length = length.ok_or_else(|| crate::core::RuntimeError::new("Texture dimensions overflow byte length"))?;
    size::<()>(length)?;
    Ok(length)
}
#[derive(Clone, Copy, Debug)]
pub struct TextureRegion {
    pub x: u32,
    pub y: u32,
    pub width: u32,
    pub height: u32,
}
