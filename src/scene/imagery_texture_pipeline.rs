//! Renderer-backed imagery uploads and deferred Web Mercator reprojection.
//! Each pipeline belongs to one layer. Drain its commands into the renderer's
//! compute pass; dropping an unexecuted command restores the tile for retry.
use super::{FrameResource, Imagery, ImageryImage};
use crate::core::{js_math::JsMath, IndexDatatype, PixelDatatype, PixelFormat, Rectangle, RuntimeError};
use crate::renderer::*;
use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
    rc::Rc,
};

type Result<T> = std::result::Result<T, RuntimeError>;

/// Decoded RGBA8 bytes for native image hosts. Rows have the same orientation as a
/// typed-array Cesium Texture source (the upload applies flipY).
#[derive(Clone, Debug)]
pub struct ImageryPixels {
    pub width: u32,
    pub height: u32,
    pub bytes: Vec<u8>,
}

#[derive(Clone, Copy, Debug)]
pub struct ImageryTextureSettings {
    pub minification_filter: TextureMinificationFilter,
    pub magnification_filter: TextureMagnificationFilter,
    pub maximum_anisotropy: Option<f64>,
}
impl ImageryTextureSettings {
    fn sampler(self) -> Result<Sampler> {
        if !matches!(self.minification_filter, TextureMinificationFilter::Nearest | TextureMinificationFilter::Linear) {
            return Err(RuntimeError::new("ImageryLayer minification filter must be NEAREST or LINEAR"));
        }
        Ok(Sampler {
            minification_filter: self.minification_filter,
            magnification_filter: self.magnification_filter,
            ..Default::default()
        })
    }
}

/// Type-erased bridge used by ImageryLayer's host-independent state machine.
/// The concrete implementation retains actual renderer textures and commands.
pub trait ImageryTextureHost {
    fn as_any(&self) -> &dyn std::any::Any;
    fn upload(&self, image: &ImageryImage, alpha: bool, settings: ImageryTextureSettings) -> Result<FrameResource>;
    fn reproject(
        &self,
        imagery: &Rc<RefCell<Imagery>>,
        geographic: bool,
        settings: ImageryTextureSettings,
    ) -> Result<()>;
    fn cancel_reprojections(&self);
}

/// Adapter for DOM image/canvas hosts. Construct a Texture with
/// `Texture::from_image` using the supplied options and actual image dimensions.
pub type ImageryImageUpload<G> = Rc<dyn Fn(&Context<G>, &FrameResource, TextureOptions) -> Result<Texture<G>>>;

pub struct ImageryTexturePipeline<G: Gl> {
    context: Context<G>,
    image_upload: Option<ImageryImageUpload<G>>,
    commands: RefCell<Vec<ComputeCommand<G>>>,
}
impl<G: Gl + 'static> ImageryTexturePipeline<G> {
    pub fn new(context: &Context<G>) -> Self {
        Self { context: context.clone(), image_upload: None, commands: RefCell::new(Vec::new()) }
    }
    pub fn with_image_upload(context: &Context<G>, upload: ImageryImageUpload<G>) -> Self {
        Self { image_upload: Some(upload), ..Self::new(context) }
    }
    /// Moves queued commands into the current frame's compute pass exactly once.
    pub fn queue_reprojection_commands(&self, commands: &mut Vec<ComputeCommand<G>>) {
        commands.append(&mut self.commands.borrow_mut());
    }
    pub fn pending_reprojections(&self) -> usize {
        self.commands.borrow().len()
    }
}

fn finalize<G: Gl>(context: &Context<G>, texture: &Texture<G>, settings: ImageryTextureSettings) -> Result<()> {
    let mut sampler = settings.sampler()?;
    if sampler.minification_filter == TextureMinificationFilter::Linear
        && sampler.magnification_filter == TextureMagnificationFilter::Linear
        && !texture.pixel_format().is_compressed_format()
        && texture.width().is_power_of_two()
        && texture.height().is_power_of_two()
    {
        let supported = if context.extension_supported("EXT_texture_filter_anisotropic")? {
            match context.host().invoke("getParameter", &[GlValue::Number(0x84ff as f64)])? {
                GlValue::Number(value) => value,
                _ => return Err(RuntimeError::new("Invalid anisotropy limit from host")),
            }
        } else {
            1.
        };
        sampler.maximum_anisotropy = settings.maximum_anisotropy.unwrap_or(supported).min(supported);
        sampler.minification_filter = TextureMinificationFilter::LinearMipmapLinear;
        texture.generate_mipmap(MipmapHint::Nicest)?;
    }
    texture.set_sampler(sampler)
}

/// Cesium's duplicated T coordinate for the two vertices in each of 64 rows.
/// Float32 conversion happens after the V8-compatible sin/log calculation.
pub fn imagery_reprojection_coordinates(rectangle: &Rectangle) -> Vec<f32> {
    let mercator = |latitude: f64| {
        let sin = latitude.js_sin();
        0.5 * ((1. + sin) / (1. - sin)).js_ln()
    };
    let south = mercator(rectangle.south);
    let scale = 1. / (mercator(rectangle.north) - south);
    (0..64)
        .flat_map(|j| {
            let fraction = j as f64 / 63.;
            let latitude = (1. - fraction) * rectangle.south + fraction * rectangle.north;
            let t = ((mercator(latitude) - south) * scale) as f32;
            [t, t]
        })
        .collect()
}

struct ReprojectionLease {
    imagery: Rc<RefCell<Imagery>>,
    active: Cell<bool>,
}
impl ReprojectionLease {
    fn finish(&self, texture: Option<FrameResource>) {
        if !self.active.replace(false) {
            return;
        }
        let mut imagery = self.imagery.borrow_mut();
        if let Some(texture) = texture {
            imagery.texture = Some(texture);
            imagery.state = super::ImageryState::Ready;
        } else {
            imagery.state = super::ImageryState::TextureLoaded;
        }
        imagery.release_reference();
    }
}
impl Drop for ReprojectionLease {
    fn drop(&mut self) {
        self.finish(None);
    }
}

impl<G: Gl + 'static> ImageryTextureHost for ImageryTexturePipeline<G> {
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
    fn upload(&self, image: &ImageryImage, alpha: bool, settings: ImageryTextureSettings) -> Result<FrameResource> {
        let sampler = settings.sampler()?;
        let texture = match image {
            ImageryImage::Image(image) => {
                let mut options = TextureOptions::rgba(1, 1);
                options.pixel_format = if alpha { PixelFormat::Rgba } else { PixelFormat::Rgb };
                options.sampler = sampler;
                if let Some(pixels) = image.get::<ImageryPixels>() {
                    options.width = pixels.width;
                    options.height = pixels.height;
                    let expected =
                        (pixels.width as usize).checked_mul(pixels.height as usize).and_then(|n| n.checked_mul(4));
                    if expected != Some(pixels.bytes.len()) {
                        return Err(RuntimeError::new("Imagery RGBA byte length does not match dimensions"));
                    }
                    if alpha {
                        Texture::new(&self.context, options, Some(&pixels.bytes))?
                    } else {
                        let rgb: Vec<u8> = pixels.bytes.chunks_exact(4).flat_map(|p| p[..3].iter().copied()).collect();
                        Texture::new(&self.context, options, Some(&rgb))?
                    }
                } else if let Some(upload) = &self.image_upload {
                    upload(&self.context, image, options)?
                } else {
                    return Err(RuntimeError::new("Imagery host image requires an image upload adapter"));
                }
            }
            ImageryImage::Compressed(_) | ImageryImage::CompressedMipmaps(_) => {
                let levels = match image {
                    ImageryImage::Compressed(image) => std::slice::from_ref(image),
                    ImageryImage::CompressedMipmaps(levels) => levels.as_slice(),
                    _ => unreachable!(),
                };
                let base = levels.first().ok_or_else(|| RuntimeError::new("Empty imagery mip chain"))?;
                let mut options = TextureOptions::rgba(base.width, base.height);
                options.pixel_format = PixelFormat::from_value(base.internal_format)
                    .ok_or_else(|| RuntimeError::new("Unknown imagery pixel format"))?;
                options.pixel_datatype = base
                    .pixel_datatype
                    .map(PixelDatatype::from_value)
                    .unwrap_or(Some(PixelDatatype::UnsignedByte))
                    .ok_or_else(|| RuntimeError::new("Unknown imagery pixel datatype"))?;
                options.sampler = sampler;
                let mips: Vec<_> = levels.iter().skip(1).map(|m| m.buffer_view().to_vec()).collect();
                if options.pixel_format.is_compressed_format() {
                    Texture::compressed(&self.context, options, base.buffer_view(), &mips)?
                } else {
                    Texture::with_mipmaps(&self.context, options, base.buffer_view(), &mips)?
                }
            }
        };
        Ok(FrameResource::new(Rc::new(texture)))
    }

    fn reproject(
        &self,
        imagery: &Rc<RefCell<Imagery>>,
        geographic: bool,
        settings: ImageryTextureSettings,
    ) -> Result<()> {
        let (source, rectangle, mercator) = {
            let i = imagery.borrow();
            (
                i.texture_web_mercator.as_ref().or(i.texture.as_ref()).cloned(),
                i.rectangle,
                i.texture_web_mercator.is_some(),
            )
        };
        let source = source.ok_or_else(|| RuntimeError::new("Imagery has no texture"))?;
        let texture = source
            .get::<Rc<Texture<G>>>()
            .ok_or_else(|| RuntimeError::new("Imagery texture belongs to another renderer host"))?
            .clone();
        if !geographic || !mercator || rectangle.width() / texture.width() as f64 <= 1e-5 {
            finalize(&self.context, &texture, settings)?;
            let mut i = imagery.borrow_mut();
            if geographic {
                i.texture = Some(source);
            }
            i.state = super::ImageryState::Ready;
            return Ok(());
        }
        texture.set_sampler(Sampler::default())?;
        let mut options = TextureOptions::rgba(texture.width(), texture.height());
        options.pixel_format = texture.pixel_format();
        options.pixel_datatype = texture.pixel_datatype();
        options.pre_multiply_alpha = texture.pre_multiply_alpha();
        let output = Rc::new(Texture::new(&self.context, options, None)?);
        if texture.width().is_power_of_two() && texture.height().is_power_of_two() {
            output.generate_mipmap(MipmapHint::Nicest)?;
        }
        let positions: Vec<f32> =
            (0..64).flat_map(|j| [0., (j as f64 / 63.) as f32, 1., (j as f64 / 63.) as f32]).collect();
        let bytes: Vec<_> = positions.iter().flat_map(|v| v.to_le_bytes()).collect();
        let position =
            Buffer::new(&self.context, BufferKind::Vertex, BufferUsage::StaticDraw, BufferData::Bytes(&bytes))?;
        let bytes: Vec<_> = imagery_reprojection_coordinates(&rectangle).iter().flat_map(|v| v.to_le_bytes()).collect();
        let t = Buffer::new(&self.context, BufferKind::Vertex, BufferUsage::StaticDraw, BufferData::Bytes(&bytes))?;
        let indices: Vec<u8> = (0..63u16)
            .flat_map(|j| {
                let a = 2 * j;
                [a, a + 2, a + 1, a + 1, a + 2, a + 3]
            })
            .flat_map(|v| v.to_le_bytes())
            .collect();
        let indices = Buffer::new(
            &self.context,
            BufferKind::Index(IndexDatatype::UnsignedShort),
            BufferUsage::StaticDraw,
            BufferData::Bytes(&indices),
        )?;
        let va = Rc::new(VertexArray::new(
            &self.context,
            vec![VertexAttribute::buffer(0, position, 2), VertexAttribute::buffer(1, t, 1)],
            Some(indices),
        )?);
        let program = Rc::new(ShaderProgram::from_sources(&self.context, &ShaderProgramOptions {
            // Equivalent to Cesium's viewport orthographic transform for [0,1] positions.
            vertex_shader_source: ShaderSource { sources: vec!["in vec2 position; in float webMercatorT; out vec2 v_textureCoordinates; void main() { v_textureCoordinates = vec2(position.x, webMercatorT); gl_Position = vec4(position * 2.0 - 1.0, 0.0, 1.0); }".into()], ..Default::default() },
            fragment_shader_source: ShaderSource { sources: vec![include_str!("../shaders/root/glsl/ReprojectWebMercatorFS.glsl").into()], ..Default::default() },
            attribute_locations: BTreeMap::from([("position".into(),0),("webMercatorT".into(),1)]),
        })?);
        imagery.borrow_mut().add_reference();
        imagery.borrow_mut().state = super::ImageryState::Transitioning;
        let lease = Rc::new(ReprojectionLease { imagery: imagery.clone(), active: Cell::new(true) });
        let post_lease = lease.clone();
        let context = self.context.clone();
        self.commands.borrow_mut().push(ComputeCommand {
            vertex_array: Some(va),
            shader_program: Some(program),
            output_texture: Some(output),
            textures: vec![("u_texture".into(), texture)],
            post_execute: Some(Box::new(move |texture| {
                if !post_lease.active.get() {
                    return Ok(());
                }
                finalize(&context, texture, settings)?;
                post_lease.finish(Some(FrameResource::new(texture.clone())));
                Ok(())
            })),
            canceled: Some(Box::new(move || {
                lease.finish(None);
                Ok(())
            })),
            ..Default::default()
        });
        Ok(())
    }
    fn cancel_reprojections(&self) {
        self.commands.borrow_mut().clear();
    }
}
