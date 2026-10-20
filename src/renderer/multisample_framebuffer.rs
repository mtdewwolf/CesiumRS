//! CesiumJS MultisampleFramebuffer: paired render and resolve targets.
use super::{context::*, *};
use crate::core::webgl_constants as gl;
use std::rc::Rc;
pub struct MultisampleFramebufferOptions<G: Gl> {
    pub width: u32,
    pub height: u32,
    pub color_textures: Vec<Rc<Texture<G>>>,
    pub color_renderbuffers: Vec<Rc<Renderbuffer<G>>>,
    pub depth_stencil_texture: Option<Rc<Texture<G>>>,
    pub depth_stencil_renderbuffer: Option<Rc<Renderbuffer<G>>>,
    pub destroy_attachments: bool,
}
pub struct MultisampleFramebuffer<G: Gl> {
    width: u32,
    height: u32,
    render: Rc<Framebuffer<G>>,
    color: Rc<Framebuffer<G>>,
}
impl<G: Gl> MultisampleFramebuffer<G> {
    pub fn new(context: &Context<G>, options: MultisampleFramebufferOptions<G>) -> Result<Self> {
        ensure(
            options.color_textures.len() == options.color_renderbuffers.len(),
            "Both color texture and renderbuffer attachments must be provided",
        )?;
        ensure(
            options.depth_stencil_texture.is_some() == options.depth_stencil_renderbuffer.is_some(),
            "Both depth-stencil texture and renderbuffer attachments must be provided",
        )?;
        let render = Rc::new(Framebuffer::new(
            context,
            FramebufferOptions {
                colors: options.color_renderbuffers.into_iter().map(Attachment::Renderbuffer).collect(),
                depth_stencil: options.depth_stencil_renderbuffer.map(Attachment::Renderbuffer),
                destroy_attachments: false,
                ..Default::default()
            },
        )?);
        let color = Rc::new(Framebuffer::new(
            context,
            FramebufferOptions {
                colors: options.color_textures.into_iter().map(Attachment::Texture).collect(),
                depth_stencil: options.depth_stencil_texture.map(Attachment::Texture),
                destroy_attachments: false,
                ..Default::default()
            },
        )?);
        render.set_destroy_attachments(options.destroy_attachments);
        color.set_destroy_attachments(options.destroy_attachments);
        Ok(Self { width: options.width, height: options.height, render, color })
    }
    pub fn get_render_framebuffer(&self) -> &Rc<Framebuffer<G>> {
        &self.render
    }
    pub fn get_color_framebuffer(&self) -> &Rc<Framebuffer<G>> {
        &self.color
    }
    pub fn blit_framebuffers(&self, blit_stencil: bool) -> Result<()> {
        self.render.bind_read()?;
        self.color.bind_draw()?;
        let c = &self.render.resource.context;
        let mut mask = 0;
        if self.color.number_of_color_attachments() > 0 {
            mask |= gl::COLOR_BUFFER_BIT;
        }
        if self.color.depth_stencil_texture().is_some() {
            mask |= gl::DEPTH_BUFFER_BIT | if blit_stencil { gl::STENCIL_BUFFER_BIT } else { 0 };
        }
        let result = c.call(
            "blitFramebuffer",
            &[
                0.into(),
                0.into(),
                self.width.into(),
                self.height.into(),
                0.into(),
                0.into(),
                self.width.into(),
                self.height.into(),
                mask.into(),
                gl::NEAREST.into(),
            ],
        );
        c.call("bindFramebuffer", &[gl::READ_FRAMEBUFFER.into(), GlValue::Null])?;
        c.call("bindFramebuffer", &[gl::DRAW_FRAMEBUFFER.into(), GlValue::Null])?;
        result.map(|_| ())
    }
    pub fn is_destroyed(&self) -> bool {
        self.render.is_destroyed() && self.color.is_destroyed()
    }
    pub fn destroy(&self) -> Result<()> {
        self.render.destroy()?;
        self.color.destroy()
    }
}
