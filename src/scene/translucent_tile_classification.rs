//! CesiumJS `TranslucentTileClassification`: translucent depth, classification
//! accumulation across frustums, and color/pick compositing.
use super::{blending_state, framebuffer_helpers::*};
use crate::{
    core::{webgl_constants as gl, PixelFormat, RuntimeError},
    renderer::{render_state::StencilTest, *},
    shaders::{post_process_stages::COMPOSITE_TRANSLUCENT_CLASSIFICATION, root::COMPARE_AND_PACK_TRANSLUCENT_DEPTH},
};
use std::rc::Rc;
pub struct TranslucentTileClassification<G: Gl> {
    classification: FramebufferManager<G>,
    accumulation: FramebufferManager<G>,
    packed: FramebufferManager<G>,
    translucent_depth: Option<Rc<Texture<G>>>,
    opaque_depth: Option<Rc<Texture<G>>>,
    pack_command: Option<ViewportCommand<G>>,
    accumulate_command: Option<ViewportCommand<G>>,
    composite_command: Option<ViewportCommand<G>>,
    pick_command: Option<ViewportCommand<G>>,
    state: RenderState,
    accumulate_state: RenderState,
    composite_state: RenderState,
    supported: bool,
    has_translucent_depth: bool,
    frustums_drawn: u32,
    saved_globe_depth: Option<TextureBinding<G>>,
    destroyed: bool,
}
impl<G: Gl> TranslucentTileClassification<G> {
    pub fn new(context: &Context<G>) -> Result<Self> {
        let target = FramebufferManagerOptions { create_depth_attachments: false, ..Default::default() };
        Ok(Self {
            classification: FramebufferManager::new(target)?,
            accumulation: FramebufferManager::new(target)?,
            packed: FramebufferManager::new(FramebufferManagerOptions::default())?,
            translucent_depth: None,
            opaque_depth: None,
            pack_command: None,
            accumulate_command: None,
            composite_command: None,
            pick_command: None,
            state: RenderState::default(),
            accumulate_state: RenderState::default(),
            composite_state: RenderState::default(),
            supported: context.capabilities()?.depth_texture,
            has_translucent_depth: false,
            frustums_drawn: 0,
            saved_globe_depth: None,
            destroyed: false,
        })
    }
    pub fn is_supported(&self) -> bool {
        self.supported
    }
    pub fn has_translucent_depth(&self) -> bool {
        self.has_translucent_depth
    }
    pub fn packed_depth_texture(&self) -> Result<Option<&Rc<Texture<G>>>> {
        self.packed.color_texture(0)
    }
    fn update(&mut self, context: &Context<G>, pass: &PassState<G>, opaque_depth: Rc<Texture<G>>) -> Result<()> {
        alive(self.destroyed)?;
        if !self.supported {
            return Ok(());
        }
        let (width, height) = (opaque_depth.width(), opaque_depth.height());
        self.opaque_depth = Some(opaque_depth);
        if self.classification.is_dirty(width, height, None, None, None) {
            self.classification.destroy()?;
            self.accumulation.destroy()?;
            self.packed.destroy()?;
            if let Some(texture) = self.translucent_depth.take() {
                texture.destroy()?;
            }
            let mut options = TextureOptions::rgba(width, height);
            options.pixel_format = PixelFormat::DepthStencil;
            options.pixel_datatype = PixelDatatype::UnsignedInt24_8;
            options.sampler = Sampler::NEAREST;
            let texture = Rc::new(Texture::new(context, options, None)?);
            self.translucent_depth = Some(texture.clone());
            self.classification.set_depth_stencil_texture(Some(texture.clone()))?;
            self.accumulation.set_depth_stencil_texture(Some(texture))?;
            self.classification.update(context, width, height, None, None, None)?;
            self.accumulation.update(context, width, height, None, None, None)?;
            self.packed.update(context, width, height, None, None, None)?;
        }
        self.state = copy_state(width, height, pass);
        self.accumulate_state = self.state.clone();
        self.accumulate_state.stencil_test =
            StencilTest { enabled: true, front_function: gl::EQUAL, reference: 0x80, ..Default::default() };
        self.composite_state = self.state.clone();
        self.composite_state.blending = blending_state::alpha_blend();
        Ok(())
    }
    /// Returns the pass state for the flagged translucent commands' depth-only
    /// variants. The caller executes those commands and then calls pack_depth.
    pub fn begin_translucent_depth(
        &mut self,
        context: &Context<G>,
        pass: &PassState<G>,
        opaque_depth: Rc<Texture<G>>,
        has_flagged_commands: bool,
    ) -> Result<Option<PassState<G>>> {
        alive(self.destroyed)?;
        self.has_translucent_depth |= has_flagged_commands;
        if !self.supported || !self.has_translucent_depth {
            return Ok(None);
        }
        self.update(context, pass, opaque_depth)?;
        let mut depth_pass = pass.clone();
        depth_pass.framebuffer = self.classification.framebuffer().cloned();
        ClearCommand { depth: Some(1.), stencil: Some(0), ..Default::default() }.execute(context, &depth_pass)?;
        self.frustums_drawn += 1;
        Ok(Some(depth_pass))
    }
    pub fn pack_depth(&mut self, context: &Context<G>, pass: &PassState<G>) -> Result<()> {
        alive(self.destroyed)?;
        if !self.has_translucent_depth {
            return Ok(());
        }
        if self.pack_command.is_none() {
            self.pack_command = Some(context.create_viewport_quad_command(
                ShaderSource::new(COMPARE_AND_PACK_TRANSLUCENT_DEPTH.source),
                ViewportCommandOptions::default(),
            )?);
        }
        let command = self.pack_command.as_mut().expect("initialized");
        command.options.framebuffer = self.packed.framebuffer().cloned();
        command.options.render_state = self.state.clone();
        command.textures = vec![
            (
                "u_opaqueDepthTexture".into(),
                TextureBinding::Texture(self.opaque_depth.as_ref().expect("updated").clone()),
            ),
            (
                "u_translucentDepthTexture".into(),
                TextureBinding::Texture(self.translucent_depth.as_ref().expect("updated").clone()),
            ),
        ];
        command.execute(context, pass)
    }
    fn accumulate(&mut self, context: &Context<G>, pass: &PassState<G>) -> Result<()> {
        copy(
            context,
            &mut self.accumulate_command,
            COMPOSITE_TRANSLUCENT_CLASSIFICATION.source,
            "colorTexture",
            color(&self.classification, 0)?,
            (self.accumulation.framebuffer().cloned(), self.accumulate_state.clone()),
            pass,
        )
    }
    /// Returns the classification target after installing its packed depth as
    /// the globe-depth sampler. end_classification restores the prior sampler.
    pub fn begin_classification(&mut self, context: &Context<G>, pass: &PassState<G>) -> Result<Option<PassState<G>>> {
        alive(self.destroyed)?;
        if !self.has_translucent_depth {
            return Ok(None);
        }
        self.accumulate(context, pass)?;
        let mut classify_pass = pass.clone();
        classify_pass.framebuffer = self.classification.framebuffer().cloned();
        if self.frustums_drawn > 1 {
            ClearCommand { color: Some([0.; 4]), ..Default::default() }.execute(context, &classify_pass)?;
        }
        context.uniform_state().update_pass(Pass::Cesium3DTileClassification);
        self.saved_globe_depth = context.automatic_texture("czm_globeDepthTexture")?;
        context
            .set_automatic_texture("czm_globeDepthTexture", Some(TextureBinding::Texture(color(&self.packed, 0)?)))?;
        Ok(Some(classify_pass))
    }
    pub fn end_classification(&mut self, context: &Context<G>, pass: &PassState<G>) -> Result<()> {
        alive(self.destroyed)?;
        if !self.has_translucent_depth {
            return Ok(());
        }
        context.set_automatic_texture("czm_globeDepthTexture", self.saved_globe_depth.take())?;
        if self.frustums_drawn > 1 {
            self.accumulate(context, pass)?;
        }
        Ok(())
    }
    pub fn execute(&mut self, context: &Context<G>, pass: &PassState<G>, picking: bool) -> Result<()> {
        alive(self.destroyed)?;
        if !self.has_translucent_depth {
            return Ok(());
        }
        let texture =
            if self.frustums_drawn == 1 { color(&self.classification, 0)? } else { color(&self.accumulation, 0)? };
        let command = if picking { &mut self.pick_command } else { &mut self.composite_command };
        if command.is_none() {
            let mut source = ShaderSource::new(COMPOSITE_TRANSLUCENT_CLASSIFICATION.source);
            if picking {
                source.defines.push("PICK".into());
            }
            *command = Some(context.create_viewport_quad_command(source, ViewportCommandOptions::default())?);
        }
        let command = command.as_mut().ok_or_else(|| RuntimeError::new("Classification composite is unavailable"))?;
        command.options.render_state = self.composite_state.clone();
        command.textures = vec![("colorTexture".into(), TextureBinding::Texture(texture))];
        command.execute(context, pass)?;
        self.classification.clear(context, &ClearCommand { color: Some([0.; 4]), ..Default::default() }, pass)?;
        if self.frustums_drawn > 1 {
            self.accumulation.clear(context, &ClearCommand { color: Some([0.; 4]), ..Default::default() }, pass)?;
        }
        self.has_translucent_depth = false;
        self.frustums_drawn = 0;
        Ok(())
    }
    pub fn is_destroyed(&self) -> bool {
        self.destroyed
    }
    pub fn destroy(&mut self) -> Result<()> {
        if !self.destroyed {
            self.classification.destroy()?;
            self.accumulation.destroy()?;
            self.packed.destroy()?;
            if let Some(depth) = self.translucent_depth.take() {
                depth.destroy()?;
            }
            for command in [
                &mut self.pack_command,
                &mut self.accumulate_command,
                &mut self.composite_command,
                &mut self.pick_command,
            ] {
                destroy_command(command)?;
            }
            self.opaque_depth = None;
            self.saved_globe_depth = None;
            self.destroyed = true;
        }
        Ok(())
    }
}
