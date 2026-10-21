//! CesiumJS Framebuffer: texture/renderbuffer attachments and explicit ownership.
use super::{context::*, CubeMapFace, Renderbuffer, Texture};
use crate::core::{webgl_constants as gl, PixelDatatype, PixelFormat, RuntimeError};
use std::rc::Rc;
pub enum Attachment<G: Gl> {
    Texture(Rc<Texture<G>>),
    CubeFace(CubeMapFace<G>),
    Renderbuffer(Rc<Renderbuffer<G>>),
}
impl<G: Gl> Clone for Attachment<G> {
    fn clone(&self) -> Self {
        match self {
            Self::Texture(v) => Self::Texture(v.clone()),
            Self::CubeFace(v) => Self::CubeFace(v.clone()),
            Self::Renderbuffer(v) => Self::Renderbuffer(v.clone()),
        }
    }
}
impl<G: Gl> Attachment<G> {
    fn resource(&self) -> &Resource<G> {
        match self {
            Self::Texture(v) => &v.resource,
            Self::CubeFace(v) => &v.resource,
            Self::Renderbuffer(v) => &v.resource,
        }
    }
    pub fn width(&self) -> u32 {
        match self {
            Self::Texture(v) => v.width(),
            Self::CubeFace(v) => v.width(),
            Self::Renderbuffer(v) => v.width(),
        }
    }
    pub fn height(&self) -> u32 {
        match self {
            Self::Texture(v) => v.height(),
            Self::CubeFace(v) => v.height(),
            Self::Renderbuffer(v) => v.height(),
        }
    }
    fn attach(&self, context: &Context<G>, attachment: u32) -> Result<()> {
        context.same(&self.resource().context)?;
        match self {
            Self::CubeFace(t) => {
                context.call(
                    "framebufferTexture2D",
                    &[
                        gl::FRAMEBUFFER.into(),
                        attachment.into(),
                        (t.face() as u32).into(),
                        t.resource.value()?,
                        0.into(),
                    ],
                )?;
            }
            Self::Texture(t) => {
                context.call(
                    "framebufferTexture2D",
                    &[gl::FRAMEBUFFER.into(), attachment.into(), gl::TEXTURE_2D.into(), t.resource.value()?, 0.into()],
                )?;
            }
            Self::Renderbuffer(r) => {
                context.call(
                    "framebufferRenderbuffer",
                    &[gl::FRAMEBUFFER.into(), attachment.into(), gl::RENDERBUFFER.into(), r.resource.value()?],
                )?;
            }
        }
        Ok(())
    }
    pub fn pixel_format_datatype(&self) -> Option<(PixelFormat, PixelDatatype)> {
        match self {
            Self::Texture(t) => Some((t.pixel_format(), t.pixel_datatype())),
            Self::CubeFace(t) => Some((t.pixel_format(), t.pixel_datatype())),
            Self::Renderbuffer(_) => None,
        }
    }
}
pub struct FramebufferOptions<G: Gl> {
    pub colors: Vec<Attachment<G>>,
    pub depth: Option<Attachment<G>>,
    pub stencil: Option<Rc<Renderbuffer<G>>>,
    pub depth_stencil: Option<Attachment<G>>,
    pub destroy_attachments: bool,
}
impl<G: Gl> Default for FramebufferOptions<G> {
    fn default() -> Self {
        Self { colors: vec![], depth: None, stencil: None, depth_stencil: None, destroy_attachments: true }
    }
}
pub struct Framebuffer<G: Gl> {
    pub(crate) resource: Resource<G>,
    options: FramebufferOptions<G>,
    destroy_attachments: std::cell::Cell<bool>,
}
impl<G: Gl> Framebuffer<G> {
    pub fn new(context: &Context<G>, options: FramebufferOptions<G>) -> Result<Self> {
        ensure(
            !(options.depth.is_some() && options.depth_stencil.is_some()),
            "Cannot attach both depth and depth-stencil",
        )?;
        ensure(
            options.stencil.is_none() || options.depth.is_none() && options.depth_stencil.is_none(),
            "Cannot combine stencil with depth/depth-stencil",
        )?;
        ensure(
            options.colors.iter().all(|a| !matches!(a, Attachment::Renderbuffer(_)))
                || options.colors.iter().all(|a| matches!(a, Attachment::Renderbuffer(_))),
            "Cannot mix color textures and renderbuffers",
        )?;
        let mrt = context.webgl2() || context.extension_supported("WEBGL_draw_buffers")?;
        ensure(mrt || options.colors.len() <= 1, "Multiple render targets unsupported")?;
        if let GlValue::Number(max) =
            if mrt { context.call("getParameter", &[gl::MAX_COLOR_ATTACHMENTS.into()])? } else { 1u32.into() }
        {
            ensure(options.colors.len() as f64 <= max, "Too many color attachments")?;
        }
        for a in options.colors.iter().chain(options.depth.iter()).chain(options.depth_stencil.iter()) {
            context.same(&a.resource().context)?;
            a.resource().value()?;
        }
        for a in &options.colors {
            if let Some((format, datatype)) = a.pixel_format_datatype() {
                ensure(
                    format.is_color_format()
                        || matches!(
                            format,
                            PixelFormat::Rg
                                | PixelFormat::RedInteger
                                | PixelFormat::RgInteger
                                | PixelFormat::RgbInteger
                                | PixelFormat::RgbaInteger
                        ),
                    "Color texture requires a color pixel format",
                )?;
                if matches!(datatype, PixelDatatype::Float | PixelDatatype::HalfFloat) {
                    let caps = context.capabilities()?;
                    ensure(
                        if datatype == PixelDatatype::Float {
                            caps.color_buffer_float
                        } else {
                            caps.color_buffer_half_float
                        },
                        "Floating point color attachments unsupported",
                    )?;
                }
            }
        }
        if let Some(Attachment::Texture(t)) = &options.depth {
            ensure(t.pixel_format() == PixelFormat::DepthComponent, "Depth texture must use DEPTH_COMPONENT")?;
        }
        if let Some(Attachment::Texture(t)) = &options.depth_stencil {
            ensure(t.pixel_format() == PixelFormat::DepthStencil, "Depth-stencil texture must use DEPTH_STENCIL")?;
        }
        if let Some(r) = &options.stencil {
            context.same(&r.resource.context)?;
            r.resource.value()?;
        }
        let resource = Resource::create(context, "createFramebuffer", &[], "deleteFramebuffer")?;
        context.call("bindFramebuffer", &[gl::FRAMEBUFFER.into(), resource.value()?])?;
        let setup = (|| {
            for (i, a) in options.colors.iter().enumerate() {
                a.attach(context, gl::COLOR_ATTACHMENT0 + i as u32)?;
            }
            if let Some(a) = &options.depth {
                a.attach(context, gl::DEPTH_ATTACHMENT)?;
            }
            if let Some(r) = &options.stencil {
                Attachment::Renderbuffer(r.clone()).attach(context, gl::STENCIL_ATTACHMENT)?;
            }
            if let Some(a) = &options.depth_stencil {
                a.attach(context, gl::DEPTH_STENCIL_ATTACHMENT)?;
            }
            Ok(())
        })();
        context.call("bindFramebuffer", &[gl::FRAMEBUFFER.into(), GlValue::Null])?;
        setup?;
        let destroy_attachments = std::cell::Cell::new(options.destroy_attachments);
        Ok(Self { resource, options, destroy_attachments })
    }
    /// Snapshot of the attachment set and current ownership policy.
    pub fn options(&self) -> FramebufferOptions<G> {
        FramebufferOptions {
            colors: self.options.colors.clone(),
            depth: self.options.depth.clone(),
            stencil: self.options.stencil.clone(),
            depth_stencil: self.options.depth_stencil.clone(),
            destroy_attachments: self.destroy_attachments.get(),
        }
    }
    pub fn number_of_color_attachments(&self) -> usize {
        self.options.colors.len()
    }
    pub fn color_texture(&self, index: usize) -> Option<&Rc<Texture<G>>> {
        match self.options.colors.get(index)? {
            Attachment::Texture(t) => Some(t),
            _ => None,
        }
    }
    pub fn color_attachment(&self, index: usize) -> Option<&Attachment<G>> {
        self.options.colors.get(index)
    }
    pub fn color_renderbuffer(&self, index: usize) -> Option<&Rc<Renderbuffer<G>>> {
        match self.options.colors.get(index)? {
            Attachment::Renderbuffer(r) => Some(r),
            _ => None,
        }
    }
    pub fn has_depth_attachment(&self) -> bool {
        self.options.depth.is_some() || self.options.depth_stencil.is_some()
    }
    pub fn dimensions(&self) -> Option<[u32; 2]> {
        self.options
            .colors
            .first()
            .or(self.options.depth.as_ref())
            .or(self.options.depth_stencil.as_ref())
            .map(|a| [a.width(), a.height()])
            .or_else(|| self.options.stencil.as_ref().map(|r| [r.width(), r.height()]))
    }
    pub fn active_color_attachments(&self) -> Vec<i32> {
        (0..self.options.colors.len()).map(|i| (gl::COLOR_ATTACHMENT0 + i as u32) as i32).collect()
    }
    fn validate_resources(&self) -> Result<()> {
        self.resource.value()?;
        for a in self.options.colors.iter().chain(self.options.depth.iter()).chain(self.options.depth_stencil.iter()) {
            a.resource().value()?;
        }
        if let Some(r) = &self.options.stencil {
            r.resource.value()?;
        }
        Ok(())
    }
    pub fn bind(&self) -> Result<()> {
        self.validate_resources()?;
        self.resource.context.call("bindFramebuffer", &[gl::FRAMEBUFFER.into(), self.resource.value()?]).map(|_| ())
    }
    pub fn bind_draw(&self) -> Result<()> {
        self.validate_resources()?;
        self.resource
            .context
            .call(
                "bindFramebuffer",
                &[
                    (if self.resource.context.webgl2() { gl::DRAW_FRAMEBUFFER } else { gl::FRAMEBUFFER }).into(),
                    self.resource.value()?,
                ],
            )
            .map(|_| ())
    }
    pub fn bind_read(&self) -> Result<()> {
        self.validate_resources()?;
        self.resource
            .context
            .call(
                "bindFramebuffer",
                &[
                    (if self.resource.context.webgl2() { gl::READ_FRAMEBUFFER } else { gl::FRAMEBUFFER }).into(),
                    self.resource.value()?,
                ],
            )
            .map(|_| ())
    }
    pub fn status(&self) -> Result<u32> {
        self.bind()?;
        let c = &self.resource.context;
        let status = c.call("checkFramebufferStatus", &[gl::FRAMEBUFFER.into()]);
        c.call("bindFramebuffer", &[gl::FRAMEBUFFER.into(), GlValue::Null])?;
        match status? {
            GlValue::Number(n) => Ok(n as u32),
            _ => Err(RuntimeError::new("Invalid framebuffer status from host")),
        }
    }
    pub fn destroy_attachments(&self) -> bool {
        self.destroy_attachments.get()
    }
    pub fn set_destroy_attachments(&self, destroy: bool) {
        self.destroy_attachments.set(destroy);
    }
    pub fn depth_texture(&self) -> Option<&Rc<Texture<G>>> {
        match self.options.depth.as_ref()? {
            Attachment::Texture(t) => Some(t),
            _ => None,
        }
    }
    pub fn depth_renderbuffer(&self) -> Option<&Rc<Renderbuffer<G>>> {
        match self.options.depth.as_ref()? {
            Attachment::Renderbuffer(r) => Some(r),
            _ => None,
        }
    }
    pub fn depth_stencil_texture(&self) -> Option<&Rc<Texture<G>>> {
        match self.options.depth_stencil.as_ref()? {
            Attachment::Texture(t) => Some(t),
            _ => None,
        }
    }
    pub fn depth_stencil_renderbuffer(&self) -> Option<&Rc<Renderbuffer<G>>> {
        match self.options.depth_stencil.as_ref()? {
            Attachment::Renderbuffer(r) => Some(r),
            _ => None,
        }
    }
    pub fn stencil_renderbuffer(&self) -> Option<&Rc<Renderbuffer<G>>> {
        self.options.stencil.as_ref()
    }
    pub fn is_destroyed(&self) -> bool {
        self.resource.is_destroyed()
    }
    pub fn destroy(&self) -> Result<()> {
        if self.is_destroyed() {
            return Ok(());
        }
        if self.destroy_attachments.get() {
            for a in
                self.options.colors.iter().chain(self.options.depth.iter()).chain(self.options.depth_stencil.iter())
            {
                a.resource().destroy()?;
            }
            if let Some(r) = &self.options.stencil {
                r.destroy()?;
            }
        }
        self.resource.destroy()
    }
}
impl<G: Gl> Drop for Framebuffer<G> {
    fn drop(&mut self) {
        let _ = self.destroy();
    }
}
impl<G: Gl> Context<G> {
    pub(crate) fn bind_framebuffer(&self, framebuffer: Option<&Framebuffer<G>>) -> Result<()> {
        if let Some(f) = framebuffer {
            self.same(&f.resource.context)?;
            f.bind()?;
            if self.validate_framebuffer.get() {
                let status = self.call("checkFramebufferStatus", &[gl::FRAMEBUFFER.into()])?;
                ensure(
                    matches!(status,GlValue::Number(n) if n==gl::FRAMEBUFFER_COMPLETE as f64),
                    "Framebuffer is incomplete",
                )?;
            }
            if self.webgl2() || self.extension_supported("WEBGL_draw_buffers")? {
                self.call("drawBuffers", &[GlValue::Ints(f.active_color_attachments())])?;
            }
        } else {
            self.call("bindFramebuffer", &[gl::FRAMEBUFFER.into(), GlValue::Null])?;
            if self.webgl2() || self.extension_supported("WEBGL_draw_buffers")? {
                self.call("drawBuffers", &[GlValue::Ints(vec![gl::BACK as i32])])?;
            }
        }
        Ok(())
    }
}

/// Explicit read options shared by typed readback and pixel-pack transfers.
pub struct ReadState<'a, G: Gl> {
    pub region: super::TextureRegion,
    pub framebuffer: Option<&'a Framebuffer<G>>,
}
#[derive(Clone, Debug)]
/// Four-component output, converted from the attachment: RGBA/UNSIGNED_BYTE
/// for normalized colors, RGBA/FLOAT for floats, or RGBA_INTEGER/UNSIGNED_INT.
/// Missing source channels receive WebGL's default components.
pub struct PixelReadback {
    pub bytes: Vec<u8>,
    pub pixel_format: PixelFormat,
    pub pixel_datatype: PixelDatatype,
}
impl<G: Gl> Context<G> {
    // WebGL guarantees RGBA reads for normalized/float color attachments and
    // RGBA_INTEGER/UNSIGNED_INT for unsigned integer color attachments. The
    // output layout, allocation and metadata must describe the same conversion.
    fn readback_layout(&self, read: &ReadState<'_, G>) -> Result<(PixelFormat, PixelDatatype, usize)> {
        let region = read.region;
        let dimensions = read.framebuffer.and_then(Framebuffer::dimensions).unwrap_or(self.drawing_buffer_size()?);
        range(region.x as usize, region.width as usize, dimensions[0] as usize)?;
        range(region.y as usize, region.height as usize, dimensions[1] as usize)?;
        let (attachment_format, attachment_datatype) = read
            .framebuffer
            .and_then(|f| f.color_attachment(0))
            .and_then(Attachment::pixel_format_datatype)
            .unwrap_or((PixelFormat::Rgba, PixelDatatype::UnsignedByte));
        let (format, datatype) = match attachment_format {
            PixelFormat::RedInteger | PixelFormat::RgInteger | PixelFormat::RgbInteger | PixelFormat::RgbaInteger => {
                (PixelFormat::RgbaInteger, PixelDatatype::UnsignedInt)
            }
            _ => (
                PixelFormat::Rgba,
                match attachment_datatype {
                    PixelDatatype::Float | PixelDatatype::HalfFloat => PixelDatatype::Float,
                    _ => PixelDatatype::UnsignedByte,
                },
            ),
        };
        let length = (region.width as usize)
            .checked_mul(region.height as usize)
            .and_then(|n| n.checked_mul(format.components_length()))
            .and_then(|n| n.checked_mul(datatype.size_in_bytes()))
            .ok_or_else(|| RuntimeError::new("Pixel readback overflow"))?;
        size::<G::Object>(length)?;
        Ok((format, datatype, length))
    }
    pub fn read_pixels_from(&self, read: ReadState<'_, G>) -> Result<PixelReadback> {
        let (format, datatype, length) = self.readback_layout(&read)?;
        let region = read.region;
        self.bind_framebuffer(read.framebuffer)?;
        if self.webgl2() {
            self.call("bindBuffer", &[gl::PIXEL_PACK_BUFFER.into(), GlValue::Null])?;
        }
        self.call("pixelStorei", &[gl::PACK_ALIGNMENT.into(), 1.into()])?;
        match self.call(
            "readPixels",
            &[
                region.x.into(),
                region.y.into(),
                region.width.into(),
                region.height.into(),
                (format as u32).into(),
                datatype.to_webgl_constant(self.webgl2()).into(),
                GlValue::Pixels { bytes: vec![0; length], datatype },
            ],
        )? {
            GlValue::Bytes(bytes) if bytes.len() == length => {
                Ok(PixelReadback { bytes, pixel_format: format, pixel_datatype: datatype })
            }
            _ => Err(RuntimeError::new("Invalid pixel readback from host")),
        }
    }
    pub fn read_pixels_to_pbo(&self, read: ReadState<'_, G>) -> Result<super::Buffer<G>> {
        let (format, datatype, length) = self.readback_layout(&read)?;
        let r = read.region;
        let buffer = super::Buffer::new(
            self,
            super::BufferKind::PixelPack,
            super::BufferUsage::DynamicRead,
            super::BufferData::Size(length),
        )?;
        self.bind_framebuffer(read.framebuffer)?;
        self.call("bindBuffer", &[gl::PIXEL_PACK_BUFFER.into(), buffer.resource.value()?])?;
        self.call("pixelStorei", &[gl::PACK_ALIGNMENT.into(), 1.into()])?;
        let result = self.call(
            "readPixels",
            &[
                r.x.into(),
                r.y.into(),
                r.width.into(),
                r.height.into(),
                (format as u32).into(),
                datatype.to_webgl_constant(self.webgl2()).into(),
                0.into(),
            ],
        );
        if self.webgl2() {
            self.call("bindBuffer", &[gl::PIXEL_PACK_BUFFER.into(), GlValue::Null])?;
        }
        result?;
        Ok(buffer)
    }
}
impl<G: Gl> Framebuffer<G> {
    pub fn blit_to(&self, destination: &Self, mask: u32) -> Result<()> {
        self.resource.context.same(&destination.resource.context)?;
        let [sw, sh] = self.dimensions().ok_or_else(|| RuntimeError::new("Source framebuffer has no attachments"))?;
        let [dw, dh] =
            destination.dimensions().ok_or_else(|| RuntimeError::new("Destination framebuffer has no attachments"))?;
        self.bind_read()?;
        destination.bind_draw()?;
        let c = &self.resource.context;
        let result = c.call(
            "blitFramebuffer",
            &[
                0.into(),
                0.into(),
                sw.into(),
                sh.into(),
                0.into(),
                0.into(),
                dw.into(),
                dh.into(),
                mask.into(),
                gl::NEAREST.into(),
            ],
        );
        c.bind_framebuffer(None)?;
        result.map(|_| ())
    }
}
