//! CesiumJS Texture: typed, compressed, DOM and framebuffer sources; Sampler.
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
    pub pre_multiply_alpha: bool,
    pub skip_color_space_conversion: bool,
    pub id: Option<String>,
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
            pre_multiply_alpha: false,
            skip_color_space_conversion: false,
            id: None,
        }
    }
}
pub struct Texture<G: Gl> {
    pub(crate) resource: Rc<Resource<G>>,
    options: TextureOptions,
    sampler: RefCell<Sampler>,
    has_mipmap: Cell<bool>,
    id: String,
}
impl<G: Gl> Texture<G> {
    pub fn new(context: &Context<G>, options: TextureOptions, pixels: Option<&[u8]>) -> Result<Self> {
        ensure(options.width > 0 && options.height > 0, "Texture dimensions must be positive")?;
        size::<G::Object>(options.width as usize)?;
        size::<G::Object>(options.height as usize)?;
        options.sampler.validate()?;
        // Typed and blank allocation. DOM, compressed and framebuffer source
        // constructors below share the same texture metadata and ownership.
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
            F::RedInteger | F::RgInteger | F::RgbInteger | F::RgbaInteger => options.pixel_datatype == D::UnsignedInt,
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
        let id = options.id.clone().unwrap_or_else(crate::core::create_guid::create_guid);
        let result = Self { resource, options, sampler: RefCell::new(sampler), has_mipmap: Cell::new(false), id };
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
    pub fn id(&self) -> &str {
        &self.id
    }
    pub fn dimensions(&self) -> [u32; 2] {
        [self.width(), self.height()]
    }
    pub fn pre_multiply_alpha(&self) -> bool {
        self.options.pre_multiply_alpha || matches!(self.pixel_format(), PixelFormat::Rgb | PixelFormat::Luminance)
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
        let size = if self.pixel_format().is_compressed_format() {
            self.pixel_format().compressed_texture_size_in_bytes(self.width() as f64, self.height() as f64) as usize
        } else {
            byte_length(&self.options, self.width(), self.height()).unwrap()
        };
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
                    "texParameteri",
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
        ensure(
            !self.options.pixel_format.is_depth_format() && !self.options.pixel_format.is_compressed_format(),
            "Cannot upload subimages to depth/compressed textures",
        )?;
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
        ensure(
            !self.options.pixel_format.is_depth_format() && !self.options.pixel_format.is_compressed_format(),
            "Cannot mipmap depth/compressed textures",
        )?;
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

pub struct FramebufferCopy<'a, G: Gl> {
    pub framebuffer: Option<&'a super::Framebuffer<G>>,
    pub source: TextureRegion,
    pub destination: [u32; 2],
}
impl<G: Gl> Texture<G> {
    /// Additional mip levels are uploaded as supplied, without row reversal,
    /// matching Cesium's typed and compressed source paths.
    pub fn with_mipmaps(
        context: &Context<G>,
        options: TextureOptions,
        base: &[u8],
        levels: &[Vec<u8>],
    ) -> Result<Self> {
        let texture = Self::new(context, options, Some(base))?;
        texture.upload_mip_levels(levels, false)?;
        Ok(texture)
    }
    pub fn compressed(context: &Context<G>, options: TextureOptions, base: &[u8], levels: &[Vec<u8>]) -> Result<Self> {
        let f = options.pixel_format;
        ensure(
            f.is_compressed_format() && options.width > 0 && options.height > 0,
            "Compressed texture requires a compressed format and positive dimensions",
        )?;
        let extension = if f.is_dxt_format() {
            Some("WEBGL_compressed_texture_s3tc")
        } else if f.is_pvrtc_format() {
            Some("WEBGL_compressed_texture_pvrtc")
        } else if f.is_astc_format() {
            Some("WEBGL_compressed_texture_astc")
        } else if f.is_etc1_format() {
            Some("WEBGL_compressed_texture_etc1")
        } else if f.is_bc7_format() {
            Some("EXT_texture_compression_bptc")
        } else {
            None
        };
        if let Some(extension) = extension {
            ensure(
                !matches!(context.call("getExtension", &[extension.into()])?, GlValue::Null),
                "Compressed texture format is unsupported by this host",
            )?;
        }
        let expected = f.compressed_texture_size_in_bytes(options.width as f64, options.height as f64) as usize;
        ensure(base.len() == expected, "Invalid compressed texture byte length")?;
        size::<G::Object>(expected)?;
        options.sampler.validate()?;
        let id = options.id.clone().unwrap_or_else(crate::core::create_guid::create_guid);
        let resource = Rc::new(Resource::create(context, "createTexture", &[], "deleteTexture")?);
        let sampler = options.sampler;
        let texture = Self { resource, options, sampler: RefCell::new(sampler), has_mipmap: Cell::new(false), id };
        texture.bind(0)?;
        let upload = (|| {
            context.call("pixelStorei", &[gl::UNPACK_ALIGNMENT.into(), 4.into()])?;
            context.call("pixelStorei", &[gl::UNPACK_FLIP_Y_WEBGL.into(), false.into()])?;
            context.call("pixelStorei", &[gl::UNPACK_PREMULTIPLY_ALPHA_WEBGL.into(), false.into()])?;
            context.call(
                "compressedTexImage2D",
                &[
                    gl::TEXTURE_2D.into(),
                    0.into(),
                    (f as u32).into(),
                    texture.width().into(),
                    texture.height().into(),
                    0.into(),
                    GlValue::Bytes(base.to_vec()),
                ],
            )
        })();
        context.call("bindTexture", &[gl::TEXTURE_2D.into(), GlValue::Null])?;
        upload?;
        texture.upload_mip_levels(levels, true)?;
        texture.set_sampler(sampler)?;
        Ok(texture)
    }
    fn upload_mip_levels(&self, levels: &[Vec<u8>], compressed: bool) -> Result<()> {
        let c = &self.resource.context;
        let (mut width, mut height) = (self.width(), self.height());
        // Validate all levels before mutating GPU storage.
        for bytes in levels {
            width = (width / 2).max(1);
            height = (height / 2).max(1);
            let expected = if compressed {
                self.pixel_format().compressed_texture_size_in_bytes(width as f64, height as f64) as usize
            } else {
                byte_length(&self.options, width, height)?
            };
            ensure(
                if compressed {
                    bytes.len() == expected
                } else {
                    bytes.len() >= expected && bytes.len().is_multiple_of(self.pixel_datatype().size_in_bytes())
                },
                "Invalid supplied mip-level byte length",
            )?;
        }
        self.bind(0)?;
        width = self.width();
        height = self.height();
        let result = (|| {
            // Cesium retains the base image's unpack alignment for every
            // supplied mip level. RGB levels may therefore include row padding.
            if !compressed {
                self.pixel_store(self.width())?;
            }
            for (level, bytes) in levels.iter().enumerate() {
                width = (width / 2).max(1);
                height = (height / 2).max(1);
                if compressed {
                    c.call(
                        "compressedTexImage2D",
                        &[
                            gl::TEXTURE_2D.into(),
                            size(level + 1)?,
                            (self.pixel_format() as u32).into(),
                            width.into(),
                            height.into(),
                            0.into(),
                            GlValue::Bytes(bytes.clone()),
                        ],
                    )?;
                } else {
                    c.call(
                        "texImage2D",
                        &[
                            gl::TEXTURE_2D.into(),
                            size(level + 1)?,
                            self.pixel_format().to_internal_format(self.pixel_datatype(), true).into(),
                            width.into(),
                            height.into(),
                            0.into(),
                            (self.pixel_format() as u32).into(),
                            self.pixel_datatype().to_webgl_constant(true).into(),
                            GlValue::Pixels { bytes: bytes.clone(), datatype: self.pixel_datatype() },
                        ],
                    )?;
                }
            }
            Ok(())
        })();
        c.call("bindTexture", &[gl::TEXTURE_2D.into(), GlValue::Null])?;
        result
    }
    /// The image is an opaque host image/canvas/video/ImageBitmap object. A host
    /// supplies intrinsic dimensions in options; no image decoding occurs here.
    pub fn from_image(context: &Context<G>, options: TextureOptions, image: G::Object) -> Result<Self> {
        ensure(
            !options.pixel_format.is_depth_format() && !options.pixel_format.is_compressed_format(),
            "Image source requires a color texture",
        )?;
        let t = Self::new(context, options, None)?;
        t.bind(0)?;
        let result = (|| {
            t.image_pixel_store()?;
            context.call(
                "texImage2D",
                &[
                    gl::TEXTURE_2D.into(),
                    0.into(),
                    t.pixel_format().to_internal_format(t.pixel_datatype(), true).into(),
                    (t.pixel_format() as u32).into(),
                    t.pixel_datatype().to_webgl_constant(true).into(),
                    GlValue::Object(image),
                ],
            )
        })();
        context.call("bindTexture", &[gl::TEXTURE_2D.into(), GlValue::Null])?;
        result?;
        Ok(t)
    }
    fn image_pixel_store(&self) -> Result<()> {
        let c = &self.resource.context;
        c.call("pixelStorei", &[gl::UNPACK_ALIGNMENT.into(), 4.into()])?;
        c.call("pixelStorei", &[gl::UNPACK_PREMULTIPLY_ALPHA_WEBGL.into(), self.pre_multiply_alpha().into()])?;
        c.call("pixelStorei", &[gl::UNPACK_FLIP_Y_WEBGL.into(), self.flip_y().into()])?;
        c.call(
            "pixelStorei",
            &[
                gl::UNPACK_COLORSPACE_CONVERSION_WEBGL.into(),
                if self.options.skip_color_space_conversion { gl::NONE } else { gl::BROWSER_DEFAULT_WEBGL }.into(),
            ],
        )?;
        c.call("bindBuffer", &[gl::PIXEL_UNPACK_BUFFER.into(), GlValue::Null])?;
        Ok(())
    }
    pub fn copy_from_image(&self, image: G::Object, region: TextureRegion) -> Result<()> {
        ensure(
            !self.pixel_format().is_depth_format() && !self.pixel_format().is_compressed_format(),
            "Image source requires a color texture",
        )?;
        range(region.x as usize, region.width as usize, self.width() as usize)?;
        range(region.y as usize, region.height as usize, self.height() as usize)?;
        let c = &self.resource.context;
        self.bind(0)?;
        let result = (|| {
            self.image_pixel_store()?;
            c.call(
                "texSubImage2D",
                &[
                    gl::TEXTURE_2D.into(),
                    0.into(),
                    region.x.into(),
                    region.y.into(),
                    (self.pixel_format() as u32).into(),
                    self.pixel_datatype().to_webgl_constant(true).into(),
                    GlValue::Object(image),
                ],
            )
        })();
        c.call("bindTexture", &[gl::TEXTURE_2D.into(), GlValue::Null])?;
        result.map(|_| ())
    }
    pub fn from_framebuffer(
        context: &Context<G>,
        options: TextureOptions,
        source: FramebufferCopy<'_, G>,
    ) -> Result<Self> {
        ensure(
            source.destination == [0, 0]
                && source.source.width == options.width
                && source.source.height == options.height,
            "Framebuffer source dimensions must match the new texture",
        )?;
        let t = Self::new(context, options, None)?;
        t.copy_from_framebuffer(source)?;
        Ok(t)
    }
    pub fn copy_from_framebuffer(&self, copy: FramebufferCopy<'_, G>) -> Result<()> {
        ensure(
            !self.pixel_format().is_depth_format()
                && !self.pixel_format().is_compressed_format()
                && !matches!(self.pixel_datatype(), PixelDatatype::Float | PixelDatatype::HalfFloat),
            "Cannot copy framebuffer into depth, compressed or float textures",
        )?;
        let r = copy.source;
        let [x, y] = copy.destination;
        range(x as usize, r.width as usize, self.width() as usize)?;
        range(y as usize, r.height as usize, self.height() as usize)?;
        let c = &self.resource.context;
        let dimensions =
            copy.framebuffer.and_then(super::Framebuffer::dimensions).unwrap_or(c.host().drawing_buffer_size()?);
        range(r.x as usize, r.width as usize, dimensions[0] as usize)?;
        range(r.y as usize, r.height as usize, dimensions[1] as usize)?;
        c.bind_framebuffer(copy.framebuffer)?;
        self.bind(0)?;
        let result = c.call(
            "copyTexSubImage2D",
            &[
                gl::TEXTURE_2D.into(),
                0.into(),
                x.into(),
                y.into(),
                r.x.into(),
                r.y.into(),
                r.width.into(),
                r.height.into(),
            ],
        );
        c.call("bindTexture", &[gl::TEXTURE_2D.into(), GlValue::Null])?;
        result.map(|_| ())
    }
}
