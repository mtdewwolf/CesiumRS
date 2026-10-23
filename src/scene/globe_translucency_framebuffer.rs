//! CesiumJS `GlobeTranslucencyFramebuffer`: classification and packed depth targets.
use super::framebuffer_helpers::*;
use crate::{renderer::*, shaders::post_process_stages::PASS_THROUGH_DEPTH};
use std::rc::Rc;
pub struct GlobeTranslucencyFramebuffer<G: Gl> {
    framebuffer: FramebufferManager<G>,
    packed: FramebufferManager<G>,
    state: RenderState,
    command: Option<ViewportCommand<G>>,
    destroyed: bool,
}
impl<G: Gl> GlobeTranslucencyFramebuffer<G> {
    pub fn new() -> Result<Self> {
        Ok(Self {
            framebuffer: FramebufferManager::new(FramebufferManagerOptions {
                depth_stencil: true,
                supports_depth_texture: true,
                ..Default::default()
            })?,
            packed: FramebufferManager::new(FramebufferManagerOptions::default())?,
            state: RenderState::default(),
            command: None,
            destroyed: false,
        })
    }
    pub fn classification_framebuffer(&self) -> Option<&Rc<Framebuffer<G>>> {
        self.framebuffer.framebuffer()
    }
    pub fn classification_texture(&self) -> Result<Option<&Rc<Texture<G>>>> {
        self.framebuffer.color_texture(0)
    }
    pub fn packed_depth_framebuffer(&self) -> Option<&Rc<Framebuffer<G>>> {
        self.packed.framebuffer()
    }
    pub fn packed_depth_texture(&self) -> Result<Option<&Rc<Texture<G>>>> {
        self.packed.color_texture(0)
    }
    pub fn depth_stencil_texture(&self) -> Option<&Rc<Texture<G>>> {
        self.framebuffer.depth_stencil_texture()
    }
    pub fn depth_stencil_renderbuffer(&self) -> Option<&Rc<Renderbuffer<G>>> {
        self.framebuffer.depth_stencil_renderbuffer()
    }
    /// As in the source, allocation and command update do not themselves clear.
    pub fn update_and_clear(
        &mut self,
        hdr: bool,
        viewport: Viewport,
        context: &Context<G>,
        pass: &PassState<G>,
    ) -> Result<()> {
        alive(self.destroyed)?;
        self.framebuffer.update(
            context,
            viewport.width,
            viewport.height,
            Some(1),
            Some(hdr_datatype(context, hdr)?),
            None,
        )?;
        self.packed.update(context, viewport.width, viewport.height, None, None, None)?;
        self.state = copy_state(viewport.width, viewport.height, pass);
        Ok(())
    }
    pub fn clear_classification(&self, context: &Context<G>, pass: &PassState<G>) -> Result<()> {
        alive(self.destroyed)?;
        self.framebuffer.clear(
            context,
            &ClearCommand {
                color: Some([0.; 4]),
                depth: Some(1.),
                stencil: Some(0),
                render_state: self.state.clone(),
                ..Default::default()
            },
            pass,
        )
    }
    pub fn pack_depth(&mut self, context: &Context<G>, pass: &PassState<G>) -> Result<Rc<Texture<G>>> {
        alive(self.destroyed)?;
        copy(
            context,
            &mut self.command,
            PASS_THROUGH_DEPTH.source,
            "u_depthTexture",
            depth(&self.framebuffer)?,
            (self.packed.framebuffer().cloned(), self.state.clone()),
            pass,
        )?;
        color(&self.packed, 0)
    }
    pub fn is_destroyed(&self) -> bool {
        self.destroyed
    }
    pub fn destroy(&mut self) -> Result<()> {
        if !self.destroyed {
            self.framebuffer.destroy()?;
            self.packed.destroy()?;
            destroy_command(&mut self.command)?;
            self.destroyed = true;
        }
        Ok(())
    }
}
