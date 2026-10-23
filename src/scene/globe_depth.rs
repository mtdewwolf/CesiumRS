//! CesiumJS `GlobeDepth`: globe targets, depth packing and stencil-masked merging.
use super::framebuffer_helpers::*;
use crate::{
    core::{webgl_constants as gl, Color, RuntimeError},
    renderer::{render_state::StencilTest, *},
    shaders::post_process_stages::{PASS_THROUGH, PASS_THROUGH_DEPTH},
};
use std::rc::Rc;
pub struct GlobeDepth<G: Gl> {
    pub picking: bool,
    pick: FramebufferManager<G>,
    output: FramebufferManager<G>,
    packed: FramebufferManager<G>,
    temp: FramebufferManager<G>,
    merged: FramebufferManager<G>,
    copy_color: Option<ViewportCommand<G>>,
    copy_depth: Option<ViewportCommand<G>>,
    copy_temp: Option<ViewportCommand<G>>,
    update_depth: Option<ViewportCommand<G>>,
    state: RenderState,
    update_state: RenderState,
    num_samples: u32,
    clear_globe_depth: bool,
    initialized: bool,
    destroyed: bool,
}
impl<G: Gl> GlobeDepth<G> {
    pub fn new() -> Result<Self> {
        let target =
            FramebufferManagerOptions { depth_stencil: true, supports_depth_texture: true, ..Default::default() };
        Ok(Self {
            picking: false,
            pick: FramebufferManager::new(target)?,
            output: FramebufferManager::new(target)?,
            packed: FramebufferManager::new(FramebufferManagerOptions::default())?,
            temp: FramebufferManager::new(FramebufferManagerOptions::default())?,
            merged: FramebufferManager::new(FramebufferManagerOptions {
                create_color_attachments: false,
                create_depth_attachments: false,
                depth_stencil: true,
                ..Default::default()
            })?,
            copy_color: None,
            copy_depth: None,
            copy_temp: None,
            update_depth: None,
            state: RenderState::default(),
            update_state: RenderState::default(),
            num_samples: 1,
            clear_globe_depth: false,
            initialized: false,
            destroyed: false,
        })
    }
    pub fn color_framebuffer_manager(&self) -> &FramebufferManager<G> {
        if self.picking {
            &self.pick
        } else {
            &self.output
        }
    }
    pub fn framebuffer(&self) -> Option<&Rc<Framebuffer<G>>> {
        self.color_framebuffer_manager().framebuffer()
    }
    pub fn depth_stencil_texture(&self) -> Option<&Rc<Texture<G>>> {
        self.color_framebuffer_manager().depth_stencil_texture()
    }
    pub fn color_texture(&self) -> Result<Option<&Rc<Texture<G>>>> {
        self.color_framebuffer_manager().color_texture(0)
    }
    pub fn packed_depth_texture(&self) -> Result<Option<&Rc<Texture<G>>>> {
        self.packed.color_texture(0)
    }
    pub fn packed_depth_framebuffer(&self) -> Option<&Rc<Framebuffer<G>>> {
        self.packed.framebuffer()
    }
    pub fn update(
        &mut self,
        context: &Context<G>,
        pass: &PassState<G>,
        viewport: Viewport,
        num_samples: u32,
        hdr: bool,
        clear_globe_depth: bool,
    ) -> Result<()> {
        alive(self.destroyed)?;
        self.num_samples = num_samples;
        if self.picking {
            self.pick.update(context, viewport.width, viewport.height, None, None, None)?;
        } else {
            self.output.update(
                context,
                viewport.width,
                viewport.height,
                Some(num_samples),
                Some(hdr_datatype(context, hdr)?),
                None,
            )?;
        }
        self.packed.update(context, viewport.width, viewport.height, None, None, None)?;
        self.state = copy_state(viewport.width, viewport.height, pass);
        self.update_state = self.state.clone();
        self.update_state.stencil_test = StencilTest {
            enabled: true,
            front_function: gl::EQUAL,
            back_function: gl::NEVER,
            reference: 0x80,
            mask: 0x80,
            ..Default::default()
        };
        context.set_automatic_texture("czm_globeDepthTexture", None)?;
        self.clear_globe_depth = clear_globe_depth;
        self.initialized = true;
        Ok(())
    }
    pub fn prepare_color_textures(&self, blit_stencil: bool) -> Result<()> {
        alive(self.destroyed)?;
        if !self.picking && self.num_samples > 1 {
            self.output.prepare_textures(blit_stencil)?;
        }
        Ok(())
    }
    pub fn execute_copy_depth(&mut self, context: &Context<G>, pass: &PassState<G>) -> Result<()> {
        alive(self.destroyed)?;
        if !self.initialized {
            return Ok(());
        }
        self.prepare_color_textures(false)?;
        let texture = depth(self.color_framebuffer_manager())?;
        copy(
            context,
            &mut self.copy_depth,
            PASS_THROUGH_DEPTH.source,
            "u_depthTexture",
            texture,
            (self.packed.framebuffer().cloned(), self.state.clone()),
            pass,
        )?;
        context.set_automatic_texture("czm_globeDepthTexture", Some(TextureBinding::Texture(color(&self.packed, 0)?)))
    }
    pub fn execute_update_depth(
        &mut self,
        context: &Context<G>,
        pass: &PassState<G>,
        depth_texture: Option<Rc<Texture<G>>>,
    ) -> Result<()> {
        alive(self.destroyed)?;
        let texture = depth_texture
            .or_else(|| pass.framebuffer.as_ref().and_then(|f| f.depth_stencil_texture().cloned()))
            .ok_or_else(|| RuntimeError::new("Depth texture is unavailable"))?;
        if !self.clear_globe_depth && self.depth_stencil_texture().is_some_and(|t| Rc::ptr_eq(t, &texture)) {
            if self.initialized {
                let texture = depth(self.color_framebuffer_manager())?;
                copy(
                    context,
                    &mut self.copy_depth,
                    PASS_THROUGH_DEPTH.source,
                    "u_depthTexture",
                    texture,
                    (self.packed.framebuffer().cloned(), self.state.clone()),
                    pass,
                )?;
            }
            return Ok(());
        }
        if !self.initialized {
            return Ok(());
        }
        let packed = color(&self.packed, 0)?;
        let changed = self.merged.framebuffer().is_none()
            || self.merged.depth_stencil_texture().is_none_or(|t| !Rc::ptr_eq(t, &texture))
            || self.merged.color_texture(0)?.is_none_or(|t| !Rc::ptr_eq(t, &packed));
        if changed {
            self.temp.destroy()?;
            self.temp.update(context, packed.width(), packed.height(), None, None, None)?;
            self.merged.set_color_texture(0, Some(packed.clone()))?;
            self.merged.set_depth_stencil_texture(Some(texture.clone()))?;
            self.merged.update(context, packed.width(), packed.height(), None, None, None)?;
            self.state = copy_state(packed.width(), packed.height(), pass);
            self.update_state = self.state.clone();
            self.update_state.stencil_test = StencilTest {
                enabled: true,
                front_function: gl::EQUAL,
                back_function: gl::NEVER,
                reference: 0x80,
                mask: 0x80,
                ..Default::default()
            };
        }
        copy(
            context,
            &mut self.copy_temp,
            PASS_THROUGH_DEPTH.source,
            "u_depthTexture",
            texture,
            (self.temp.framebuffer().cloned(), self.state.clone()),
            pass,
        )?;
        copy(
            context,
            &mut self.update_depth,
            PASS_THROUGH.source,
            "colorTexture",
            color(&self.temp, 0)?,
            (self.merged.framebuffer().cloned(), self.update_state.clone()),
            pass,
        )
    }
    pub fn execute_copy_color(&mut self, context: &Context<G>, pass: &PassState<G>) -> Result<()> {
        alive(self.destroyed)?;
        if !self.initialized {
            return Ok(());
        }
        let texture = color(self.color_framebuffer_manager(), 0)?;
        copy(
            context,
            &mut self.copy_color,
            PASS_THROUGH.source,
            "colorTexture",
            texture,
            (None, self.state.clone()),
            pass,
        )
    }
    pub fn clear(&self, context: &Context<G>, pass: &PassState<G>, clear_color: Color) -> Result<()> {
        alive(self.destroyed)?;
        if !self.initialized {
            return Ok(());
        }
        self.color_framebuffer_manager().clear(
            context,
            &ClearCommand {
                color: Some([clear_color.red, clear_color.green, clear_color.blue, clear_color.alpha]),
                stencil: Some(0),
                ..Default::default()
            },
            pass,
        )
    }
    pub fn is_destroyed(&self) -> bool {
        self.destroyed
    }
    pub fn destroy(&mut self) -> Result<()> {
        if !self.destroyed {
            self.pick.destroy()?;
            self.output.destroy()?;
            self.packed.destroy()?;
            self.temp.destroy()?;
            self.merged.destroy()?;
            destroy_command(&mut self.copy_color)?;
            destroy_command(&mut self.copy_depth)?;
            destroy_command(&mut self.copy_temp)?;
            destroy_command(&mut self.update_depth)?;
            self.destroyed = true;
        }
        Ok(())
    }
}
