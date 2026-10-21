//! Cesium FramebufferManager: reusable, resizeable render targets.
//! Updates construct a candidate before releasing the previous valid target.
use super::{context::*, *};
use crate::core::{PixelFormat, RuntimeError};
use std::rc::Rc;

#[derive(Clone, Copy, Debug)]
pub struct FramebufferManagerOptions {
    pub num_samples: u32,
    pub color_attachments_length: usize,
    pub color: bool,
    pub depth: bool,
    pub depth_stencil: bool,
    pub supports_depth_texture: bool,
    pub create_color_attachments: bool,
    pub create_depth_attachments: bool,
    pub pixel_datatype: Option<PixelDatatype>,
    pub pixel_format: Option<PixelFormat>,
}
impl Default for FramebufferManagerOptions {
    fn default() -> Self {
        Self {
            num_samples: 1,
            color_attachments_length: 1,
            color: true,
            depth: false,
            depth_stencil: false,
            supports_depth_texture: false,
            create_color_attachments: true,
            create_depth_attachments: true,
            pixel_datatype: None,
            pixel_format: None,
        }
    }
}
fn same_handle<T>(a: &Option<Rc<T>>, b: &Option<Rc<T>>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => Rc::ptr_eq(a, b),
        (None, None) => true,
        _ => false,
    }
}
pub struct FramebufferManager<G: Gl> {
    options: FramebufferManagerOptions,
    dimensions: Option<[u32; 2]>,
    samples: u32,
    datatype: Option<PixelDatatype>,
    format: Option<PixelFormat>,
    attachments_dirty: bool,
    framebuffer: Option<Rc<Framebuffer<G>>>,
    multisample: Option<MultisampleFramebuffer<G>>,
    colors: Vec<Option<Rc<Texture<G>>>>,
    color_buffers: Vec<Option<Rc<Renderbuffer<G>>>>,
    depth_texture: Option<Rc<Texture<G>>>,
    depth_buffer: Option<Rc<Renderbuffer<G>>>,
    depth_stencil_texture: Option<Rc<Texture<G>>>,
    depth_stencil_buffer: Option<Rc<Renderbuffer<G>>>,
}
macro_rules! depth_accessors {
    ($get:ident,$set:ident,$field:ident,$ty:ident) => {
        pub fn $get(&self) -> Option<&Rc<$ty<G>>> {
            self.$field.as_ref()
        }
        pub fn $set(&mut self, value: Option<Rc<$ty<G>>>) -> Result<()> {
            ensure(
                !self.options.create_depth_attachments,
                "create_depth_attachments must be false to set depth resources",
            )?;
            self.attachments_dirty |= !same_handle(&self.$field, &value);
            self.$field = value;
            Ok(())
        }
    };
}
impl<G: Gl> FramebufferManager<G> {
    pub fn options(&self) -> FramebufferManagerOptions {
        self.options
    }
    pub fn new(options: FramebufferManagerOptions) -> Result<Self> {
        ensure(
            options.color || options.depth || options.depth_stencil,
            "Must enable at least one framebuffer attachment type",
        )?;
        ensure(!(options.depth && options.depth_stencil), "Cannot have both depth and depth-stencil attachments")?;
        Ok(Self {
            samples: options.num_samples,
            datatype: options.pixel_datatype,
            format: options.pixel_format,
            colors: (0..options.color_attachments_length).map(|_| None).collect(),
            color_buffers: (0..options.color_attachments_length).map(|_| None).collect(),
            options,
            dimensions: None,
            attachments_dirty: false,
            framebuffer: None,
            multisample: None,
            depth_texture: None,
            depth_buffer: None,
            depth_stencil_texture: None,
            depth_stencil_buffer: None,
        })
    }
    pub fn framebuffer(&self) -> Option<&Rc<Framebuffer<G>>> {
        if self.samples > 1 {
            self.multisample.as_ref().map(MultisampleFramebuffer::get_render_framebuffer)
        } else {
            self.framebuffer.as_ref()
        }
    }
    /// Resolve target for explicit readback; for ordinary targets this is framebuffer().
    pub fn color_framebuffer(&self) -> Option<&Rc<Framebuffer<G>>> {
        self.multisample.as_ref().map(MultisampleFramebuffer::get_color_framebuffer).or(self.framebuffer.as_ref())
    }
    pub fn num_samples(&self) -> u32 {
        self.samples
    }
    pub fn status(&self) -> Result<u32> {
        self.framebuffer().ok_or_else(|| RuntimeError::new("Framebuffer manager has not been updated"))?.status()
    }
    pub fn is_dirty(
        &self,
        width: u32,
        height: u32,
        num_samples: Option<u32>,
        datatype: Option<PixelDatatype>,
        format: Option<PixelFormat>,
    ) -> bool {
        let samples = num_samples.unwrap_or(1);
        self.attachments_dirty
            || self.dimensions != Some([width, height])
            || self.samples != samples
            || datatype.is_some_and(|d| Some(d) != self.datatype)
            || format.is_some_and(|f| Some(f) != self.format)
            || (if samples == 1 { self.framebuffer.is_none() } else { self.multisample.is_none() })
            || (self.options.color && self.colors.first().and_then(Option::as_ref).is_none())
    }
    pub fn update(
        &mut self,
        context: &Context<G>,
        width: u32,
        height: u32,
        num_samples: Option<u32>,
        datatype: Option<PixelDatatype>,
        format: Option<PixelFormat>,
    ) -> Result<bool> {
        let samples = num_samples.unwrap_or(1);
        ensure(samples > 0, "Sample count must be positive")?;
        ensure(width > 0 && height > 0, "Framebuffer dimensions must be positive")?;
        ensure(
            !(self.options.depth && samples > 1),
            "Ordinary depth attachments do not support multisampling; use depth_stencil",
        )?;
        let datatype = datatype.or(self.datatype).or(self.options.color.then_some(PixelDatatype::UnsignedByte));
        let format = format.or(self.format).or(self.options.color.then_some(PixelFormat::Rgba));
        if !self.is_dirty(width, height, Some(samples), datatype, format) {
            return Ok(false);
        }
        let mut next = Self::new(self.options)?;
        next.dimensions = Some([width, height]);
        next.samples = samples;
        next.datatype = datatype;
        next.format = format;
        if !self.options.create_color_attachments {
            next.colors = self.colors.clone();
            next.color_buffers = self.color_buffers.clone();
        }
        if !self.options.create_depth_attachments {
            next.depth_texture = self.depth_texture.clone();
            next.depth_buffer = self.depth_buffer.clone();
            next.depth_stencil_texture = self.depth_stencil_texture.clone();
            next.depth_stencil_buffer = self.depth_stencil_buffer.clone();
        }
        let texture = |format, datatype| -> Result<Rc<Texture<G>>> {
            let mut options = TextureOptions::rgba(width, height);
            options.pixel_format = format;
            options.pixel_datatype = datatype;
            options.sampler = Sampler::NEAREST;
            Ok(Rc::new(Texture::new(context, options, None)?))
        };
        let buffer = |format, samples| -> Result<Rc<Renderbuffer<G>>> {
            Ok(Rc::new(Renderbuffer::new(
                context,
                RenderbufferOptions { format, width: Some(width), height: Some(height), num_samples: samples },
            )?))
        };
        if self.options.color && self.options.create_color_attachments {
            for i in 0..self.options.color_attachments_length {
                next.colors[i] = Some(texture(format.unwrap(), datatype.unwrap())?);
                if samples > 1 {
                    next.color_buffers[i] =
                        Some(buffer(RenderbufferFormat::get_color_format(datatype.unwrap() as u32), samples)?);
                }
            }
        }
        if self.options.depth_stencil && self.options.create_depth_attachments {
            if self.options.supports_depth_texture {
                next.depth_stencil_texture = Some(texture(PixelFormat::DepthStencil, PixelDatatype::UnsignedInt24_8)?);
                if samples > 1 {
                    next.depth_stencil_buffer = Some(buffer(RenderbufferFormat::Depth24Stencil8, samples)?);
                }
            } else {
                next.depth_stencil_buffer = Some(buffer(RenderbufferFormat::DepthStencil, 1)?);
            }
        }
        if self.options.depth && self.options.create_depth_attachments {
            if self.options.supports_depth_texture {
                next.depth_texture = Some(texture(PixelFormat::DepthComponent, PixelDatatype::UnsignedInt)?);
            } else {
                next.depth_buffer = Some(buffer(RenderbufferFormat::DepthComponent16, 1)?);
            }
        }
        let colors = if self.options.color {
            next.colors
                .iter()
                .map(|v| v.clone().ok_or_else(|| RuntimeError::new("Missing external color texture")))
                .collect::<Result<Vec<_>>>()?
        } else {
            vec![]
        };
        if samples > 1 {
            let buffers = if self.options.color {
                next.color_buffers
                    .iter()
                    .map(|v| v.clone().ok_or_else(|| RuntimeError::new("Missing external color renderbuffer")))
                    .collect::<Result<Vec<_>>>()?
            } else {
                vec![]
            };
            next.multisample = Some(MultisampleFramebuffer::new(
                context,
                MultisampleFramebufferOptions {
                    width,
                    height,
                    color_textures: colors,
                    color_renderbuffers: buffers,
                    depth_stencil_texture: next.depth_stencil_texture.clone(),
                    depth_stencil_renderbuffer: next.depth_stencil_buffer.clone(),
                    destroy_attachments: false,
                },
            )?);
        } else {
            ensure(
                next.depth_texture.is_none() || next.depth_buffer.is_none(),
                "Cannot have both depth texture and renderbuffer",
            )?;
            ensure(
                next.depth_stencil_texture.is_none() || next.depth_stencil_buffer.is_none(),
                "Cannot have both depth-stencil texture and renderbuffer",
            )?;
            next.framebuffer = Some(Rc::new(Framebuffer::new(
                context,
                FramebufferOptions {
                    colors: colors.into_iter().map(Attachment::Texture).collect(),
                    depth: next
                        .depth_texture
                        .clone()
                        .map(Attachment::Texture)
                        .or_else(|| next.depth_buffer.clone().map(Attachment::Renderbuffer)),
                    depth_stencil: next
                        .depth_stencil_texture
                        .clone()
                        .map(Attachment::Texture)
                        .or_else(|| next.depth_stencil_buffer.clone().map(Attachment::Renderbuffer)),
                    destroy_attachments: false,
                    ..Default::default()
                },
            )?));
        }
        self.destroy()?;
        *self = next;
        Ok(true)
    }
    pub fn color_texture(&self, index: usize) -> Result<Option<&Rc<Texture<G>>>> {
        ensure(index < self.colors.len(), "Color attachment index out of range")?;
        Ok(self.colors[index].as_ref())
    }
    pub fn color_renderbuffer(&self, index: usize) -> Result<Option<&Rc<Renderbuffer<G>>>> {
        ensure(index < self.color_buffers.len(), "Color attachment index out of range")?;
        Ok(self.color_buffers[index].as_ref())
    }
    pub fn set_color_texture(&mut self, index: usize, value: Option<Rc<Texture<G>>>) -> Result<()> {
        ensure(
            !self.options.create_color_attachments,
            "create_color_attachments must be false to set color resources",
        )?;
        ensure(index < self.colors.len(), "Color attachment index out of range")?;
        self.attachments_dirty |= !same_handle(&self.colors[index], &value);
        self.colors[index] = value;
        Ok(())
    }
    pub fn set_color_renderbuffer(&mut self, index: usize, value: Option<Rc<Renderbuffer<G>>>) -> Result<()> {
        ensure(
            !self.options.create_color_attachments,
            "create_color_attachments must be false to set color resources",
        )?;
        ensure(index < self.color_buffers.len(), "Color attachment index out of range")?;
        self.attachments_dirty |= !same_handle(&self.color_buffers[index], &value);
        self.color_buffers[index] = value;
        Ok(())
    }
    depth_accessors!(depth_texture, set_depth_texture, depth_texture, Texture);
    depth_accessors!(depth_renderbuffer, set_depth_renderbuffer, depth_buffer, Renderbuffer);
    depth_accessors!(depth_stencil_texture, set_depth_stencil_texture, depth_stencil_texture, Texture);
    depth_accessors!(depth_stencil_renderbuffer, set_depth_stencil_renderbuffer, depth_stencil_buffer, Renderbuffer);
    pub fn prepare_textures(&self, blit_stencil: bool) -> Result<()> {
        if let Some(f) = &self.multisample {
            f.blit_framebuffers(blit_stencil)?;
        }
        Ok(())
    }
    pub fn clear(&self, context: &Context<G>, command: &ClearCommand<G>, pass: &PassState<G>) -> Result<()> {
        let mut command = command.clone();
        command.framebuffer = Some(
            self.framebuffer().ok_or_else(|| RuntimeError::new("Framebuffer manager has not been updated"))?.clone(),
        );
        command.execute(context, pass)
    }
    pub fn destroy_framebuffer(&mut self) -> Result<()> {
        if let Some(f) = &self.framebuffer {
            f.destroy()?;
        }
        self.framebuffer = None;
        if let Some(f) = &self.multisample {
            f.destroy()?;
        }
        self.multisample = None;
        Ok(())
    }
    pub fn destroy(&mut self) -> Result<()> {
        self.destroy_framebuffer()?;
        fn texture<G: Gl>(v: &mut Option<Rc<Texture<G>>>, owned: bool) -> Result<()> {
            if let Some(t) = v {
                if owned {
                    t.destroy()?;
                }
                if t.is_destroyed() {
                    *v = None;
                }
            }
            Ok(())
        }
        fn buffer<G: Gl>(v: &mut Option<Rc<Renderbuffer<G>>>, owned: bool) -> Result<()> {
            if let Some(t) = v {
                if owned {
                    t.destroy()?;
                }
                if t.is_destroyed() {
                    *v = None;
                }
            }
            Ok(())
        }
        if self.options.color {
            for v in &mut self.colors {
                texture(v, self.options.create_color_attachments)?;
            }
            for v in &mut self.color_buffers {
                buffer(v, self.options.create_color_attachments)?;
            }
        }
        if self.options.depth {
            texture(&mut self.depth_texture, self.options.create_depth_attachments)?;
            buffer(&mut self.depth_buffer, self.options.create_depth_attachments)?;
        }
        if self.options.depth_stencil {
            texture(&mut self.depth_stencil_texture, self.options.create_depth_attachments)?;
            buffer(&mut self.depth_stencil_buffer, self.options.create_depth_attachments)?;
        }
        Ok(())
    }
}
impl<G: Gl> Drop for FramebufferManager<G> {
    fn drop(&mut self) {
        let _ = self.destroy();
    }
}
