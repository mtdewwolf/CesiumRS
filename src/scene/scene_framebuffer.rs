//! CesiumJS `SceneFramebuffer`: multisampled scene color and object-ID targets.
use super::framebuffer_helpers::*;
use crate::{core::Color, renderer::*};
use std::rc::Rc;
pub struct SceneFramebuffer<G: Gl> {
    color: FramebufferManager<G>,
    ids: FramebufferManager<G>,
    num_samples: u32,
    destroyed: bool,
}
impl<G: Gl> SceneFramebuffer<G> {
    pub fn new() -> Result<Self> {
        let options =
            FramebufferManagerOptions { depth_stencil: true, supports_depth_texture: true, ..Default::default() };
        Ok(Self {
            color: FramebufferManager::new(options)?,
            ids: FramebufferManager::new(options)?,
            num_samples: 1,
            destroyed: false,
        })
    }
    pub fn framebuffer(&self) -> Option<&Rc<Framebuffer<G>>> {
        self.color.framebuffer()
    }
    pub fn color_framebuffer(&self) -> Option<&Rc<Framebuffer<G>>> {
        self.color.color_framebuffer()
    }
    pub fn id_framebuffer(&self) -> Option<&Rc<Framebuffer<G>>> {
        self.ids.framebuffer()
    }
    pub fn depth_stencil_texture(&self) -> Option<&Rc<Texture<G>>> {
        self.color.depth_stencil_texture()
    }
    pub fn color_texture(&self) -> Result<Option<&Rc<Texture<G>>>> {
        self.color.color_texture(0)
    }
    pub fn id_texture(&self) -> Result<Option<&Rc<Texture<G>>>> {
        self.ids.color_texture(0)
    }
    pub fn update(&mut self, context: &Context<G>, viewport: Viewport, hdr: bool, num_samples: u32) -> Result<()> {
        alive(self.destroyed)?;
        self.num_samples = num_samples;
        self.color.update(
            context,
            viewport.width,
            viewport.height,
            Some(num_samples),
            Some(hdr_datatype(context, hdr)?),
            None,
        )?;
        self.ids.update(context, viewport.width, viewport.height, None, None, None)?;
        Ok(())
    }
    pub fn clear(&self, context: &Context<G>, pass: &PassState<G>, _clear_color: Color) -> Result<()> {
        alive(self.destroyed)?;
        // The upstream method overwrites clearColor with the transparent ID
        // clear color before clearing both targets.
        let command = ClearCommand { color: Some([0.; 4]), depth: Some(1.), ..Default::default() };
        self.color.clear(context, &command, pass)?;
        self.ids.clear(context, &command, pass)
    }
    pub fn prepare_color_textures(&self) -> Result<()> {
        alive(self.destroyed)?;
        if self.num_samples > 1 {
            self.color.prepare_textures(false)?;
        }
        Ok(())
    }
    pub fn is_destroyed(&self) -> bool {
        self.destroyed
    }
    pub fn destroy(&mut self) -> Result<()> {
        if !self.destroyed {
            self.color.destroy()?;
            self.ids.destroy()?;
            self.destroyed = true;
        }
        Ok(())
    }
}
