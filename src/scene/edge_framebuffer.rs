//! CesiumJS `EdgeFramebuffer`: MRT color, object ID and packed depth attachments.
use super::framebuffer_helpers::*;
use crate::{
    core::{Color, PixelFormat},
    renderer::*,
};
use std::rc::Rc;
pub struct EdgeFramebuffer<G: Gl> {
    manager: FramebufferManager<G>,
    color: Option<Rc<Texture<G>>>,
    depth: Option<Rc<Texture<G>>>,
    clear_color: [f64; 4],
    destroyed: bool,
}
impl<G: Gl> EdgeFramebuffer<G> {
    pub fn new() -> Result<Self> {
        Ok(Self {
            manager: FramebufferManager::new(FramebufferManagerOptions {
                color_attachments_length: 3,
                depth_stencil: true,
                supports_depth_texture: true,
                ..Default::default()
            })?,
            color: None,
            depth: None,
            clear_color: [0.; 4],
            destroyed: false,
        })
    }
    pub fn framebuffer(&self) -> Option<&Rc<Framebuffer<G>>> {
        self.manager.framebuffer()
    }
    pub fn color_texture(&self) -> Option<&Rc<Texture<G>>> {
        self.color.as_ref()
    }
    pub fn id_texture(&self) -> Result<Option<&Rc<Texture<G>>>> {
        self.manager.color_texture(1)
    }
    pub fn depth_texture(&self) -> Result<Option<&Rc<Texture<G>>>> {
        self.manager.color_texture(2)
    }
    pub fn depth_stencil_texture(&self) -> Option<&Rc<Texture<G>>> {
        self.depth.as_ref()
    }
    pub fn update(
        &mut self,
        context: &Context<G>,
        viewport: Viewport,
        hdr: bool,
        existing_color: Option<Rc<Texture<G>>>,
        existing_depth: Option<Rc<Texture<G>>>,
    ) -> Result<bool> {
        alive(self.destroyed)?;
        let changed = self.manager.update(
            context,
            viewport.width,
            viewport.height,
            Some(1),
            Some(hdr_datatype(context, hdr)?),
            Some(PixelFormat::Rgba),
        )?;
        // Upstream overrides the exposed handles, without replacing the actual
        // framebuffer's owned attachments.
        self.color = existing_color.or(self.manager.color_texture(0)?.cloned());
        self.depth = existing_depth.or(self.manager.depth_stencil_texture().cloned());
        Ok(changed)
    }
    pub fn get_clear_command(&mut self, clear_color: Option<Color>) -> Result<ClearCommand<G>> {
        alive(self.destroyed)?;
        if let Some(c) = clear_color {
            self.clear_color = [c.red, c.green, c.blue, c.alpha];
        }
        Ok(ClearCommand {
            framebuffer: self.framebuffer().cloned(),
            color: Some(self.clear_color),
            depth: Some(1.),
            stencil: Some(0),
            ..Default::default()
        })
    }
    pub fn clear(&mut self, context: &Context<G>, pass: &PassState<G>, clear_color: Option<Color>) -> Result<()> {
        self.get_clear_command(clear_color)?.execute(context, pass)
    }
    pub fn is_destroyed(&self) -> bool {
        self.destroyed
    }
    pub fn destroy(&mut self) -> Result<()> {
        if !self.destroyed {
            self.manager.destroy()?;
            self.color = None;
            self.depth = None;
            self.destroyed = true;
        }
        Ok(())
    }
}
