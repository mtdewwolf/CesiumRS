//! Cesium Texture3D uploads, mip chains, subvolumes and sampling.
use super::{context::*, texture_support as support, *};
use crate::core::{webgl_constants as gl, PixelFormat};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};
#[derive(Clone, Copy, Debug)]
pub struct TextureVolume {
    pub x: u32,
    pub y: u32,
    pub z: u32,
    pub width: u32,
    pub height: u32,
    pub depth: u32,
}
pub struct Texture3D<G: Gl> {
    pub(crate) resource: Rc<Resource<G>>,
    options: TextureOptions,
    depth: u32,
    sampler: RefCell<Sampler>,
    allocated_levels: Cell<u32>,
    id: String,
}
impl<G: Gl> Texture3D<G> {
    pub fn new(context: &Context<G>, options: TextureOptions, depth: u32, bytes: Option<&[u8]>) -> Result<Self> {
        Self::with_mipmaps(context, options, depth, bytes, &[])
    }
    pub fn with_mipmaps(
        context: &Context<G>,
        options: TextureOptions,
        depth: u32,
        bytes: Option<&[u8]>,
        mips: &[Vec<u8>],
    ) -> Result<Self> {
        ensure(context.webgl2(), "Texture3D requires WebGL2")?;
        support::validate_format(context, options.pixel_format, options.pixel_datatype)?;
        ensure(options.width > 0 && options.height > 0 && depth > 0, "3D texture dimensions must be positive")?;
        ensure(
            support::color_format(options.pixel_format, options.pixel_datatype)
                || (bytes.is_none()
                    && match options.pixel_format {
                        crate::core::PixelFormat::DepthComponent => {
                            matches!(options.pixel_datatype, PixelDatatype::UnsignedShort | PixelDatatype::UnsignedInt)
                        }
                        crate::core::PixelFormat::DepthStencil => {
                            options.pixel_datatype == PixelDatatype::UnsignedInt24_8
                        }
                        _ => false,
                    }),
            "Unsupported 3D texture format or source",
        )?;
        let length = support::pixels_size(
            options.pixel_format,
            options.pixel_datatype,
            &[options.width, options.height, depth],
        )?;
        if let GlValue::Number(max) = context.call("getParameter", &[gl::MAX_3D_TEXTURE_SIZE.into()])? {
            ensure(
                [options.width, options.height, depth].iter().all(|&d| d as f64 <= max),
                "3D texture exceeds context limits",
            )?;
        }
        if let Some(bytes) = bytes {
            ensure(bytes.len() == length, "Invalid 3D texture source length")?;
        }
        ensure(bytes.is_some() || mips.is_empty(), "Mip levels require a base image")?;
        let mut dimensions = [options.width, options.height, depth];
        for mip in mips {
            dimensions = dimensions.map(|n| (n / 2).max(1));
            ensure(
                mip.len() >= support::pixels_size(options.pixel_format, options.pixel_datatype, &dimensions)?,
                "Invalid 3D mip size",
            )?;
        }
        let max_levels = 32 - [options.width, options.height, depth].into_iter().max().unwrap().leading_zeros();
        ensure(mips.len() < max_levels as usize, "Too many 3D mip levels")?;
        let resource = Rc::new(Resource::create(context, "createTexture", &[], "deleteTexture")?);
        let id = options.id.clone().unwrap_or_else(crate::core::create_guid::create_guid);
        let sampler = options.sampler;
        let texture = Self {
            resource,
            options,
            depth,
            sampler: RefCell::new(sampler),
            allocated_levels: Cell::new(if bytes.is_some() { max_levels } else { 1 }),
            id,
        };
        texture.bind(0)?;
        let result = (|| {
            support::pixel_store(
                context,
                &texture.options,
                texture.width(),
                false,
                texture.options.skip_color_space_conversion,
            )?;
            let format = texture.pixel_format();
            let datatype = texture.pixel_datatype();
            if let Some(base) = bytes {
                // Allocate the complete immutable chain so generate_mipmap can
                // later fill it; a one-level immutable allocation cannot grow.
                context.call(
                    "texStorage3D",
                    &[
                        gl::TEXTURE_3D.into(),
                        max_levels.into(),
                        (match datatype {
                            PixelDatatype::UnsignedShort4_4_4_4 => gl::RGBA4,
                            PixelDatatype::UnsignedShort5_5_5_1 => gl::RGB5_A1,
                            PixelDatatype::UnsignedShort5_6_5 => gl::RGB565,
                            _ => format.to_internal_format(datatype, true),
                        })
                        .into(),
                        texture.width().into(),
                        texture.height().into(),
                        depth.into(),
                    ],
                )?;
                texture.upload(0, [0; 3], texture.dimensions(), base)?;
                let mut dims = texture.dimensions();
                for (i, mip) in mips.iter().enumerate() {
                    dims = dims.map(|n| (n / 2).max(1));
                    texture.upload(i as u32 + 1, [0; 3], dims, mip)?;
                }
            } else {
                context.call(
                    "texImage3D",
                    &[
                        gl::TEXTURE_3D.into(),
                        0.into(),
                        (match datatype {
                            PixelDatatype::UnsignedShort4_4_4_4 => gl::RGBA4,
                            PixelDatatype::UnsignedShort5_5_5_1 => gl::RGB5_A1,
                            PixelDatatype::UnsignedShort5_6_5 => gl::RGB565,
                            _ => format.to_internal_format(datatype, true),
                        })
                        .into(),
                        texture.width().into(),
                        texture.height().into(),
                        depth.into(),
                        0.into(),
                        (format as u32).into(),
                        datatype.to_webgl_constant(true).into(),
                        GlValue::Null,
                    ],
                )?;
            }
            support::sampler(context, gl::TEXTURE_3D, format, datatype, sampler)
        })();
        context.call("bindTexture", &[gl::TEXTURE_3D.into(), GlValue::Null])?;
        result?;
        Ok(texture)
    }
    fn upload(&self, level: u32, offset: [u32; 3], dims: [u32; 3], bytes: &[u8]) -> Result<()> {
        self.resource
            .context
            .call(
                "texSubImage3D",
                &[
                    gl::TEXTURE_3D.into(),
                    level.into(),
                    offset[0].into(),
                    offset[1].into(),
                    offset[2].into(),
                    dims[0].into(),
                    dims[1].into(),
                    dims[2].into(),
                    (self.pixel_format() as u32).into(),
                    self.pixel_datatype().to_webgl_constant(true).into(),
                    GlValue::Pixels { bytes: bytes.to_vec(), datatype: self.pixel_datatype() },
                ],
            )
            .map(|_| ())
    }
    pub fn copy_from(&self, region: TextureVolume, bytes: &[u8]) -> Result<()> {
        ensure(!self.pixel_format().is_depth_format(), "Cannot upload depth volume pixels")?;
        range(region.x as usize, region.width as usize, self.width() as usize)?;
        range(region.y as usize, region.height as usize, self.height() as usize)?;
        range(region.z as usize, region.depth as usize, self.depth as usize)?;
        ensure(
            bytes.len()
                == support::pixels_size(
                    self.pixel_format(),
                    self.pixel_datatype(),
                    &[region.width, region.height, region.depth],
                )?,
            "Invalid volume source length",
        )?;
        self.bind(0)?;
        let c = &self.resource.context;
        let result = (|| {
            support::pixel_store(c, &self.options, region.width, false, false)?;
            self.upload(0, [region.x, region.y, region.z], [region.width, region.height, region.depth], bytes)
        })();
        c.call("bindTexture", &[gl::TEXTURE_3D.into(), GlValue::Null])?;
        result
    }
    pub fn bind(&self, unit: u32) -> Result<()> {
        let c = &self.resource.context;
        if let GlValue::Number(max) = c.call("getParameter", &[gl::MAX_COMBINED_TEXTURE_IMAGE_UNITS.into()])? {
            ensure((unit as f64) < max, "Texture unit out of range")?;
        }
        c.call("activeTexture", &[(gl::TEXTURE0 + unit).into()])?;
        c.call("bindTexture", &[gl::TEXTURE_3D.into(), self.resource.value()?]).map(|_| ())
    }
    pub fn set_sampler(&self, value: Sampler) -> Result<()> {
        value.validate()?;
        self.bind(0)?;
        let c = &self.resource.context;
        let r = support::sampler(c, gl::TEXTURE_3D, self.pixel_format(), self.pixel_datatype(), value);
        c.call("bindTexture", &[gl::TEXTURE_3D.into(), GlValue::Null])?;
        r?;
        *self.sampler.borrow_mut() = value;
        Ok(())
    }
    pub fn generate_mipmap(&self, hint: MipmapHint) -> Result<()> {
        ensure(!self.pixel_format().is_depth_format(), "Cannot generate depth mipmaps")?;
        self.bind(0)?;
        let c = &self.resource.context;
        let r = (|| {
            c.call("hint", &[gl::GENERATE_MIPMAP_HINT.into(), (hint as u32).into()])?;
            c.call("generateMipmap", &[gl::TEXTURE_3D.into()])?;
            Ok(())
        })();
        c.call("bindTexture", &[gl::TEXTURE_3D.into(), GlValue::Null])?;
        r?;
        self.allocated_levels.set(32 - self.dimensions().into_iter().max().unwrap().leading_zeros());
        Ok(())
    }
    pub fn id(&self) -> &str {
        &self.id
    }
    pub fn width(&self) -> u32 {
        self.options.width
    }
    pub fn height(&self) -> u32 {
        self.options.height
    }
    pub fn depth(&self) -> u32 {
        self.depth
    }
    pub fn dimensions(&self) -> [u32; 3] {
        [self.width(), self.height(), self.depth]
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
    pub fn pre_multiply_alpha(&self) -> bool {
        self.options.pre_multiply_alpha || matches!(self.pixel_format(), PixelFormat::Rgb | PixelFormat::Luminance)
    }
    pub fn sampler(&self) -> Sampler {
        *self.sampler.borrow()
    }
    pub fn size_in_bytes(&self) -> usize {
        (0..self.allocated_levels.get())
            .map(|level| {
                let dimensions = self.dimensions().map(|n| (n >> level).max(1));
                support::pixels_size(self.pixel_format(), self.pixel_datatype(), &dimensions).unwrap()
            })
            .sum()
    }
    pub fn destroy(&self) -> Result<()> {
        self.resource.destroy()
    }
    pub fn is_destroyed(&self) -> bool {
        self.resource.is_destroyed()
    }
}
