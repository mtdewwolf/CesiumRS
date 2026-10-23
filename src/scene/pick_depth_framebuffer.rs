//! CesiumJS `PickDepthFramebuffer`: a depth/stencil target for a one-pixel pass.
use super::framebuffer_helpers::*;
use crate::{core::Cartesian2, renderer::*};
use std::rc::Rc;
pub struct PickDepthFramebuffer<G: Gl> {
    framebuffer: FramebufferManager<G>,
    pass: PassState<G>,
    destroyed: bool,
}
impl<G: Gl> PickDepthFramebuffer<G> {
    pub fn new() -> Result<Self> {
        Ok(Self {
            framebuffer: FramebufferManager::new(FramebufferManagerOptions {
                color: false,
                depth_stencil: true,
                supports_depth_texture: true,
                ..Default::default()
            })?,
            pass: PassState::default(),
            destroyed: false,
        })
    }
    pub fn framebuffer(&self) -> Option<&Rc<Framebuffer<G>>> {
        self.framebuffer.framebuffer()
    }
    pub fn depth_stencil_texture(&self) -> Option<&Rc<Texture<G>>> {
        self.framebuffer.depth_stencil_texture()
    }
    pub fn update(&mut self, context: &Context<G>, position: Cartesian2, viewport: Viewport) -> Result<PassState<G>> {
        alive(self.destroyed)?;
        if self.framebuffer.is_dirty(viewport.width, viewport.height, None, None, None) {
            let [width, height] = context.drawing_buffer_size()?;
            self.framebuffer.update(context, width, height, None, None, None)?;
        }
        self.pass = PassState {
            framebuffer: self.framebuffer.framebuffer().cloned(),
            viewport: Some(Viewport { x: 0, y: 0, ..viewport }),
            blending_enabled: Some(false),
            scissor_test: Some(ScissorTest {
                enabled: true,
                rectangle: Viewport {
                    x: position.x as i32,
                    y: (viewport.height as f64 - position.y) as i32,
                    width: 1,
                    height: 1,
                },
            }),
        };
        Ok(self.pass.clone())
    }
    pub fn is_destroyed(&self) -> bool {
        self.destroyed
    }
    pub fn destroy(&mut self) -> Result<()> {
        if !self.destroyed {
            self.framebuffer.destroy()?;
            self.pass = PassState::default();
            self.destroyed = true;
        }
        Ok(())
    }
}
