//! Portable Scene frame orchestration and executable renderer submission.
//! Optional Scene components attach to stages; absent features do not enqueue
//! commands. Advanced effect/picking components remain separate ports.
use super::{Atmosphere, Camera, CameraOptions, Fog, FrameFog, FramePasses, FrameState, SceneMode};
use crate::{
    core::*,
    renderer::{self, ClearCommand, Context, FrameTextures, Gl, Pass, PassState, UniformFrame, Viewport},
};
use std::{
    rc::Rc,
    sync::{
        atomic::{AtomicBool, Ordering},
        Arc,
    },
};
pub type Result<T> = std::result::Result<T, RuntimeError>;
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SceneStage {
    ReleaseResources,
    ResetBudgets,
    PrePasses,
    MostDetailedPicks,
    Preload,
    PreloadFlight,
    DeferCheck,
    CreditBegin,
    Environment,
    Resolve,
    GlobeEnd,
    PostPasses,
    CreditEnd,
}
#[derive(Clone, Debug, PartialEq)]
pub struct SceneSubmission {
    pub pass: Pass,
    pub frustum: usize,
    pub near: f64,
    pub far: f64,
    pub draw_mode: super::SceneDrawMode,
    pub oit: Option<super::OitDrawKind>,
    pub metadata_pick: Option<Rc<super::PickMetadataShader>>,
    pub framebuffer: Option<super::FrameResource>,
    pub viewport: Option<Viewport>,
    pub scissor_test: Option<renderer::ScissorTest>,
    pub blending_enabled: Option<bool>,
    pub metadata: renderer::DrawMetadata,
    pub clipping_planes: Option<super::SceneClippingPlanes>,
    pub tileset_draw: Option<super::TilesetDrawKind>,
}
pub type SceneExecute = Rc<dyn Fn(SceneSubmission) -> Result<()>>;
pub type SceneCommandFilter = Rc<dyn Fn(&SceneCommand) -> bool>;
/// Owned inputs for an executable draw, including uniforms, instancing and
/// texture lifetimes. ShaderProgram::from_sources enables automatic variants.
pub struct SceneDrawOptions<G: Gl> {
    pub vertices: Rc<renderer::VertexArray<G>>,
    pub program: Rc<renderer::ShaderProgram<G>>,
    pub state: renderer::RenderState,
    pub primitive_type: PrimitiveType,
    pub offset: usize,
    pub count: Option<usize>,
    pub instance_count: u32,
    pub uniforms: Vec<(String, renderer::UniformValue)>,
    pub uniform_map: Option<renderer::UniformMap>,
    pub textures: Vec<(String, Rc<dyn renderer::SampledTexture<G>>)>,
    pub framebuffer: Option<Rc<renderer::Framebuffer<G>>>,
    pub pick_metadata: Option<super::PickMetadataShader>,
}
impl<G: Gl> SceneDrawOptions<G> {
    pub fn new(vertices: Rc<renderer::VertexArray<G>>, program: Rc<renderer::ShaderProgram<G>>) -> Self {
        Self {
            vertices,
            program,
            state: Default::default(),
            primitive_type: PrimitiveType::Triangles,
            offset: 0,
            count: None,
            instance_count: 0,
            uniforms: Vec::new(),
            uniform_map: None,
            textures: Vec::new(),
            framebuffer: None,
            pick_metadata: None,
        }
    }
}
#[derive(Clone)]
pub struct SceneCommand {
    pub visibility: Vec<Rc<std::cell::Cell<bool>>>,
    pub id: String,
    pub pass: Pass,
    pub metadata: renderer::DrawMetadata,
    pub execute: SceneExecute,
    pub is_clear: bool,
    pub clipping_planes: Option<super::SceneClippingPlanes>,
    pub tileset_draw: Option<super::TilesetDrawKind>,
    pub stencil_clear: Option<u32>,
}
impl SceneCommand {
    pub fn with_visibility(mut self, show: Rc<std::cell::Cell<bool>>) -> Self {
        self.visibility.push(show);
        self
    }
    pub fn shown(&self) -> bool {
        self.visibility.iter().all(|show| show.get())
    }
    pub fn new(id: impl Into<String>, pass: Pass, execute: SceneExecute) -> Self {
        Self {
            visibility: Vec::new(),
            id: id.into(),
            pass,
            metadata: renderer::DrawMetadata::default(),
            execute,
            is_clear: false,
            clipping_planes: None,
            tileset_draw: None,
            stencil_clear: None,
        }
    }
    /// Own the resources while the command is queued; DrawCommand borrows them
    /// only for submission, so Scene never stores dangling resource references.
    pub fn draw<G: Gl + 'static>(
        id: impl Into<String>,
        context: Context<G>,
        vertices: Rc<renderer::VertexArray<G>>,
        program: Rc<renderer::ShaderProgram<G>>,
        pass: Pass,
        state: renderer::RenderState,
        metadata: renderer::DrawMetadata,
    ) -> Self {
        let mut options = SceneDrawOptions::new(vertices, program);
        options.state = state;
        Self::from_draw(id, context, pass, metadata, options)
    }
    pub fn from_draw<G: Gl + 'static>(
        id: impl Into<String>,
        context: Context<G>,
        pass: Pass,
        metadata: renderer::DrawMetadata,
        options: SceneDrawOptions<G>,
    ) -> Self {
        use super::DerivedCommand;
        let programs = std::cell::RefCell::new(std::collections::HashMap::<
            (
                super::SceneDrawMode,
                Option<String>,
                Option<super::ClippingShaderKey>,
                bool,
                Option<super::OitDrawKind>,
                Option<String>,
            ),
            Rc<renderer::ShaderProgram<G>>,
        >::new());
        Self {
            visibility: Vec::new(),
            id: id.into(),
            pass,
            metadata,
            is_clear: false,
            clipping_planes: None,
            tileset_draw: None,
            stencil_clear: None,
            execute: Rc::new(move |submission| {
                let mode = submission.draw_mode;
                let clipping = submission.clipping_planes.as_ref().map(|c| c.prepare(&context)).transpose()?.flatten();
                let polygon_offset = submission.tileset_draw.is_some_and(|d| d.polygon_offset());
                let picked_info = submission.metadata_pick.as_deref().or(options.pick_metadata.as_ref());
                let metadata_key = mode.pick_metadata.then(|| picked_info.map(|info| format!("{info:?}"))).flatten();
                let key =
                    (mode, submission.metadata.pick_id.clone(), clipping, polygon_offset, submission.oit, metadata_key);
                let cached = { programs.borrow().get(&key).cloned() };
                let program = if mode == super::SceneDrawMode::default()
                    && clipping.is_none()
                    && !polygon_offset
                    && submission.oit.is_none()
                {
                    Rc::clone(&options.program)
                } else if let Some(p) = cached {
                    p
                } else {
                    let mut source = options.program.source_options().cloned().ok_or_else(|| {
                        RuntimeError::new("Scene shader variants require ShaderProgram::from_sources")
                    })?;
                    if let Some(key) = clipping {
                        source = super::SceneClippingPlanes::shader(&source, key);
                    }
                    if polygon_offset && !source.fragment_shader_source.defines.iter().any(|d| d == "POLYGON_OFFSET") {
                        source.fragment_shader_source.defines.push("POLYGON_OFFSET".into());
                    }
                    if mode.logarithmic_depth {
                        source = DerivedCommand::logarithmic_depth(&source);
                    }
                    if mode.high_dynamic_range && !mode.pick && !mode.depth_only {
                        source = DerivedCommand::hdr(&source);
                    }
                    if mode.depth_only {
                        source = DerivedCommand::depth_only(&source);
                    } else if mode.pick_metadata {
                        if let Some(info) = picked_info {
                            source = DerivedCommand::pick_metadata(&source, info);
                        }
                    } else if mode.pick {
                        if let Some(id) = &submission.metadata.pick_id {
                            source = DerivedCommand::pick(&source, id);
                        } else {
                            source = DerivedCommand::depth_only(&source);
                        }
                    }
                    if let Some(oit) = submission.oit {
                        source = oit.shader_for_rendering(&source);
                    }
                    let program = Rc::new(renderer::ShaderProgram::from_sources(&context, &source)?);
                    programs.borrow_mut().insert(key, Rc::clone(&program));
                    program
                };
                let mut command = renderer::DrawCommand::new(&options.vertices, &program);
                command.pass = Some(pass);
                command.render_state =
                    if mode.depth_only || mode.pick && !mode.pick_metadata && submission.metadata.pick_id.is_none() {
                        DerivedCommand::depth_only_render_state(&options.state)
                    } else if mode.pick || mode.pick_metadata {
                        DerivedCommand::pick_render_state(&options.state)
                    } else {
                        options.state.clone()
                    };
                command.metadata = submission.metadata;
                if let Some(kind) = submission.tileset_draw {
                    command.render_state = kind.render_state(&command.render_state);
                }
                if let Some(oit) = submission.oit {
                    command.render_state = oit.render_state(&command.render_state);
                }
                command.primitive_type = options.primitive_type;
                command.offset = options.offset;
                command.count = options.count;
                command.instance_count = options.instance_count;
                command.uniforms = options.uniforms.clone();
                let mut scene_uniforms = Vec::new();
                if polygon_offset {
                    scene_uniforms.push((
                        "u_polygonOffset".into(),
                        renderer::UniformValue::Float(
                            if submission.tileset_draw == Some(super::TilesetDrawKind::Backface) {
                                vec![5., 5.]
                            } else {
                                vec![0., 0.]
                            },
                        ),
                    ));
                }
                command.uniform_map = options.uniform_map.clone();
                command.samplers = options
                    .textures
                    .iter()
                    .filter(|(n, _)| clipping.is_none() || n != "model_clippingPlanes")
                    .map(|(n, t)| (n.clone(), t.as_ref()))
                    .collect();
                let clipping_uniforms = if clipping.is_some() {
                    submission.clipping_planes.as_ref().map(|c| c.uniforms(&context)).transpose()?
                } else {
                    None
                };
                if let Some((texture, uniforms)) = &clipping_uniforms {
                    command.samplers.push(("model_clippingPlanes".into(), texture.as_ref()));
                    scene_uniforms.extend(uniforms.clone());
                }
                if !scene_uniforms.is_empty() {
                    let map = options.uniform_map.clone();
                    command.uniform_map = Some(Rc::new(move || {
                        let mut values = map.as_ref().map_or_else(|| Ok(Vec::new()), |map| map())?;
                        values.retain(|(name, _)| !scene_uniforms.iter().any(|(n, _)| n == name));
                        values.extend(scene_uniforms.clone());
                        Ok(values)
                    }));
                }
                command.framebuffer = options.framebuffer.clone();
                let framebuffer = submission
                    .framebuffer
                    .as_ref()
                    .map(|f| {
                        f.get::<Rc<renderer::Framebuffer<G>>>()
                            .cloned()
                            .ok_or_else(|| RuntimeError::new("Scene framebuffer has a different renderer type"))
                    })
                    .transpose()?;
                command.execute(
                    &context,
                    &PassState {
                        framebuffer,
                        viewport: submission.viewport,
                        scissor_test: submission.scissor_test,
                        blending_enabled: submission.blending_enabled,
                    },
                )
            }),
        }
    }
    pub fn clear<G: Gl + 'static>(id: impl Into<String>, context: Context<G>, command: ClearCommand<G>) -> Self {
        let pass = command.pass.unwrap_or(Pass::Opaque);
        let mut result = Self::new(
            id,
            pass,
            Rc::new(move |s| {
                let framebuffer = s
                    .framebuffer
                    .as_ref()
                    .map(|f| {
                        f.get::<Rc<renderer::Framebuffer<G>>>()
                            .cloned()
                            .ok_or_else(|| RuntimeError::new("Scene framebuffer has a different renderer type"))
                    })
                    .transpose()?;
                command.execute(
                    &context,
                    &PassState {
                        framebuffer,
                        viewport: s.viewport,
                        scissor_test: s.scissor_test,
                        blending_enabled: s.blending_enabled,
                    },
                )
            }),
        );
        result.is_clear = true;
        result
    }
}
/// The GPU surface. Scene owns frame decisions and command ordering; the
/// backend applies uniforms, clears and executes the selected commands.
pub trait SceneBackend {
    fn begin_frustum(&mut self, _frame: &FrameState, _index: usize) -> Result<()> {
        Ok(())
    }
    fn begin_translucent(&mut self, _frame: &FrameState, _has_commands: bool) -> Result<u32> {
        Ok(1)
    }
    fn translucent_phase(&mut self, _phase: u32) -> Result<()> {
        Ok(())
    }
    fn end_translucent(&mut self, _frame: &FrameState) -> Result<()> {
        Ok(())
    }
    fn initialize_view(&mut self, _view: &mut super::SceneView, _use_oit: bool) -> Result<()> {
        Ok(())
    }
    fn copy_globe_depth(&mut self, _frame: &FrameState) -> Result<()> {
        Ok(())
    }
    fn update_tileset_depth(&mut self, _frame: &FrameState) -> Result<()> {
        Ok(())
    }
    fn capture_pick_depth(&mut self, _frame: &FrameState, _index: usize) -> Result<()> {
        Ok(())
    }
    fn resolve_framebuffers(&mut self, _frame: &FrameState) -> Result<()> {
        Ok(())
    }
    fn set_pass_overrides(&mut self, scissor: Option<renderer::ScissorTest>, blending: Option<bool>) -> Result<()> {
        if scissor.is_some() || blending.is_some() {
            return Err(RuntimeError::new("Scene backend does not support pass overrides"));
        }
        Ok(())
    }
    fn update_clipping_planes(&mut self, _planes: &mut super::ClippingPlaneCollection) -> Result<()> {
        Err(RuntimeError::new("Scene backend does not support clipping-plane uploads"))
    }
    fn stencil_buffer(&self) -> Result<bool> {
        Ok(false)
    }
    fn clear_stencil(&mut self, _value: i32, _mask: u32) -> Result<()> {
        Err(RuntimeError::new("Scene backend does not support masked stencil clears"))
    }
    fn set_render_target(
        &mut self,
        framebuffer: Option<&super::FrameResource>,
        _viewport: Option<Viewport>,
    ) -> Result<()> {
        if framebuffer.is_some() {
            return Err(RuntimeError::new("Scene backend does not support framebuffer targets"));
        }
        Ok(())
    }
    fn capabilities(&self) -> Result<renderer::ContextCapabilities> {
        Ok(renderer::ContextCapabilities::default())
    }
    fn limits(&self) -> Result<renderer::ContextLimits> {
        Ok(renderer::ContextLimits {
            minimum_aliased_line_width: 1.,
            maximum_aliased_line_width: 1.,
            ..Default::default()
        })
    }
    fn frame_context(&self) -> Option<super::FrameResource> {
        None
    }
    fn dimensions(&self) -> Result<[u32; 2]>;
    fn begin_frame(&mut self, frame: &FrameState) -> Result<()>;
    fn clear(&mut self, color: Color, depth: Option<f64>, stencil: Option<i32>) -> Result<()>;
    fn set_frustum(&mut self, frame: &FrameState, near: f64, far: f64) -> Result<()>;
    fn execute(&mut self, command: &SceneCommand, submission: SceneSubmission) -> Result<()>;
    fn end_frame(&mut self) -> Result<()>;
    fn destroy(&mut self) -> Result<()>;
}
pub struct RendererSceneBackend<G: Gl> {
    pub context: Context<G>,
    pub textures: FrameTextures<G>,
    pass_state: PassState<G>,
    pub(crate) view_resources: Option<Rc<std::cell::RefCell<super::ViewResources<G>>>>,
    original_pass_state: PassState<G>,
    using_globe_depth: bool,
    using_oit: bool,
    oit_used: bool,
    oit_active: bool,
    oit_phase: u32,
    depth_pass: bool,
    translucent_depth_commands: Vec<(SceneCommand, SceneSubmission)>,
    classification_commands: Vec<(SceneCommand, SceneSubmission)>,
}
impl<G: Gl> RendererSceneBackend<G> {
    pub fn new(context: Context<G>) -> Self {
        Self {
            context,
            textures: FrameTextures::default(),
            pass_state: PassState::default(),
            view_resources: None,
            original_pass_state: PassState::default(),
            using_globe_depth: false,
            using_oit: false,
            oit_used: false,
            oit_active: false,
            oit_phase: 0,
            depth_pass: false,
            translucent_depth_commands: Vec::new(),
            classification_commands: Vec::new(),
        }
    }
}
impl<G: Gl + 'static> SceneBackend for RendererSceneBackend<G> {
    fn begin_frustum(&mut self, _frame: &FrameState, _index: usize) -> Result<()> {
        self.translucent_depth_commands.clear();
        self.classification_commands.clear();
        self.oit_phase = 0;
        Ok(())
    }
    fn begin_translucent(&mut self, _frame: &FrameState, has_commands: bool) -> Result<u32> {
        self.oit_active = self.using_oit && has_commands;
        if self.oit_active {
            let resources = self.view_resources.as_ref().expect("initialized");
            let mut resources = resources.borrow_mut();
            // Opaque commands wrote the multisample target; OIT samples the
            // resolved depth texture, so resolve before adjustment and draws.
            if let Some(globe) = &resources.globe_depth {
                globe.prepare_color_textures(false)?;
            }
            let oit = resources.oit.as_mut().expect("supported");
            oit.adjust(&self.context, &self.pass_state)?;
            self.oit_used = true;
            return Ok(if oit.uses_mrt() { 1 } else { 2 });
        }
        Ok(1)
    }
    fn translucent_phase(&mut self, phase: u32) -> Result<()> {
        self.oit_phase = phase;
        Ok(())
    }
    fn end_translucent(&mut self, _frame: &FrameState) -> Result<()> {
        self.oit_active = false;
        if self.depth_pass || !self.using_globe_depth {
            return Ok(());
        }
        let Some(resources) = &self.view_resources else {
            return Ok(());
        };
        let depth = {
            let resources = resources.borrow();
            resources.globe_depth.as_ref().and_then(|g| g.depth_stencil_texture().cloned())
        };
        let Some(depth) = depth else {
            return Ok(());
        };
        let has_flagged = !self.translucent_depth_commands.is_empty();
        let depth_pass = resources.borrow_mut().translucent_tile_classification.begin_translucent_depth(
            &self.context,
            &self.pass_state,
            depth,
            has_flagged,
        )?;
        if let Some(pass) = depth_pass {
            for (command, submission) in &self.translucent_depth_commands {
                let mut submission = submission.clone();
                submission.framebuffer = pass.framebuffer.clone().map(super::FrameResource::new);
                submission.oit = None;
                submission.draw_mode.depth_only = true;
                submission.draw_mode.pick = false;
                submission.draw_mode.pick_metadata = false;
                submission.draw_mode.high_dynamic_range = false;
                (command.execute)(submission)?;
            }
            resources.borrow_mut().translucent_tile_classification.pack_depth(&self.context, &self.pass_state)?;
        }
        let classify_pass = resources
            .borrow_mut()
            .translucent_tile_classification
            .begin_classification(&self.context, &self.pass_state)?;
        if let Some(pass) = classify_pass {
            let result = (|| {
                for (command, submission) in &self.classification_commands {
                    let mut submission = submission.clone();
                    submission.framebuffer = pass.framebuffer.clone().map(super::FrameResource::new);
                    (command.execute)(submission)?;
                }
                Ok(())
            })();
            let restore = resources
                .borrow_mut()
                .translucent_tile_classification
                .end_classification(&self.context, &self.pass_state);
            restore?;
            result?;
        }
        Ok(())
    }
    fn initialize_view(&mut self, view: &mut super::SceneView, use_oit: bool) -> Result<()> {
        let resources = Rc::new(std::cell::RefCell::new(super::ViewResources::new(&self.context, use_oit)?));
        view.set_resources(resources.clone())?;
        self.view_resources = Some(resources);
        Ok(())
    }
    fn copy_globe_depth(&mut self, _frame: &FrameState) -> Result<()> {
        if self.using_globe_depth {
            if let Some(resources) = &self.view_resources {
                if let Some(globe) = &mut resources.borrow_mut().globe_depth {
                    globe.execute_copy_depth(&self.context, &self.pass_state)?;
                }
            }
        }
        Ok(())
    }
    fn update_tileset_depth(&mut self, _frame: &FrameState) -> Result<()> {
        if self.using_globe_depth {
            if let Some(resources) = &self.view_resources {
                if let Some(globe) = &mut resources.borrow_mut().globe_depth {
                    globe.prepare_color_textures(false)?;
                    let depth = globe.depth_stencil_texture().cloned();
                    globe.execute_update_depth(&self.context, &self.pass_state, depth)?;
                }
            }
        }
        Ok(())
    }
    fn capture_pick_depth(&mut self, frame: &FrameState, index: usize) -> Result<()> {
        if !frame.use_depth_picking || !self.using_globe_depth {
            return Ok(());
        }
        let Some(resources) = &self.view_resources else {
            return Ok(());
        };
        let mut resources = resources.borrow_mut();
        let (depth, packed) = if let Some(globe) = &resources.globe_depth {
            globe.prepare_color_textures(false)?;
            (globe.depth_stencil_texture().cloned(), globe.packed_depth_texture()?.cloned())
        } else {
            (None, None)
        };
        if let Some(depth) = depth {
            let pick = resources.get_pick_depth(index)?;
            pick.update(&self.context, depth)?;
            pick.execute_copy_depth(&self.context, &self.pass_state, packed)?;
        }
        Ok(())
    }
    fn resolve_framebuffers(&mut self, _frame: &FrameState) -> Result<()> {
        if self.using_globe_depth {
            if let Some(resources) = &self.view_resources {
                let mut resources = resources.borrow_mut();
                if let Some(globe) = &resources.globe_depth {
                    globe.prepare_color_textures(false)?;
                }
                if self.oit_used {
                    if let Some(oit) = &mut resources.oit {
                        oit.execute(&self.context, &self.original_pass_state)?;
                    }
                }
                resources.translucent_tile_classification.execute(
                    &self.context,
                    if self.oit_used { &self.original_pass_state } else { &self.pass_state },
                    _frame.passes.pick,
                )?;
                if !self.oit_used {
                    if let Some(globe) = &mut resources.globe_depth {
                        globe.execute_copy_color(&self.context, &self.original_pass_state)?;
                    }
                }
            }
        }
        self.pass_state = self.original_pass_state.clone();
        self.using_globe_depth = false;
        Ok(())
    }
    fn set_pass_overrides(&mut self, scissor: Option<renderer::ScissorTest>, blending: Option<bool>) -> Result<()> {
        self.pass_state.scissor_test = scissor;
        self.pass_state.blending_enabled = blending;
        Ok(())
    }
    fn update_clipping_planes(&mut self, planes: &mut super::ClippingPlaneCollection) -> Result<()> {
        planes.update(&self.context)
    }
    fn stencil_buffer(&self) -> Result<bool> {
        self.context.stencil_buffer()
    }
    fn clear_stencil(&mut self, value: i32, mask: u32) -> Result<()> {
        ClearCommand {
            stencil: Some(value),
            render_state: renderer::RenderState { stencil_mask: mask as i64, ..Default::default() },
            ..Default::default()
        }
        .execute(&self.context, &self.pass_state)
    }
    fn set_render_target(
        &mut self,
        framebuffer: Option<&super::FrameResource>,
        viewport: Option<Viewport>,
    ) -> Result<()> {
        self.pass_state.framebuffer = framebuffer
            .map(|f| {
                f.get::<Rc<renderer::Framebuffer<G>>>()
                    .cloned()
                    .ok_or_else(|| RuntimeError::new("Scene framebuffer has a different renderer type"))
            })
            .transpose()?;
        self.pass_state.viewport = viewport;
        self.using_globe_depth = false;
        Ok(())
    }
    fn capabilities(&self) -> Result<renderer::ContextCapabilities> {
        self.context.capabilities()
    }
    fn limits(&self) -> Result<renderer::ContextLimits> {
        self.context.limits()
    }
    fn frame_context(&self) -> Option<super::FrameResource> {
        Some(super::FrameResource::new(self.context.clone()))
    }
    fn dimensions(&self) -> Result<[u32; 2]> {
        self.context.drawing_buffer_size()
    }
    fn begin_frame(&mut self, frame: &FrameState) -> Result<()> {
        let [width, height] = self.dimensions()?;
        if self.pass_state.viewport.is_none() {
            self.pass_state.viewport = Some(Viewport { x: 0, y: 0, width, height });
        }
        self.context.begin_frame();
        self.context.update_frame(&frame.uniform, &self.textures)?;
        self.original_pass_state = self.pass_state.clone();
        self.using_globe_depth = false;
        self.using_oit = false;
        self.oit_used = false;
        self.oit_active = false;
        self.depth_pass = frame.passes.depth;
        if let Some(resources) = &self.view_resources {
            let mut resources = resources.borrow_mut();
            let super::ViewResources { globe_depth, oit, .. } = &mut *resources;
            if let Some(globe) = globe_depth {
                globe.picking = frame.passes.pick || frame.passes.pick_voxel;
                globe.update(
                    &self.context,
                    &self.pass_state,
                    self.pass_state.viewport.expect("initialized above"),
                    frame.msaa_samples.max(1),
                    frame.high_dynamic_range,
                    false,
                )?;
                self.pass_state.framebuffer = globe.framebuffer().cloned();
                self.using_globe_depth = true;
                if frame.order_independent_translucency
                    && !frame.passes.pick
                    && !frame.passes.pick_voxel
                    && !frame.passes.depth
                {
                    if let Some(oit) = oit {
                        oit.update(
                            &self.context,
                            &self.pass_state,
                            globe.color_framebuffer_manager(),
                            frame.high_dynamic_range,
                            frame.msaa_samples,
                        )?;
                        self.using_oit = oit.is_supported();
                    }
                }
            }
        }
        Ok(())
    }
    fn clear(&mut self, color: Color, depth: Option<f64>, stencil: Option<i32>) -> Result<()> {
        if depth.is_none() && self.using_oit {
            if let Some(resources) = &self.view_resources {
                if let Some(oit) = &resources.borrow().oit {
                    oit.clear(&self.context, &self.pass_state, color)?;
                }
            }
        }
        ClearCommand {
            color: if depth.is_none() { Some([color.red, color.green, color.blue, color.alpha]) } else { None },
            depth,
            stencil,
            ..ClearCommand::default()
        }
        .execute(&self.context, &self.pass_state)
    }
    fn set_frustum(&mut self, frame: &FrameState, near: f64, far: f64) -> Result<()> {
        let mut camera = frame.uniform.camera.clone();
        let (near, far) = if frame.mode == SceneMode::Scene2D {
            camera.position.x += 1. - near;
            camera.view = Matrix4::compute_view(camera.position, camera.direction, camera.up, camera.right);
            camera.inverse_view = camera.view.inverse_transformation();
            (1., (far - near).max(1.))
        } else {
            (near, far)
        };
        camera.frustum = match camera.frustum {
            renderer::UniformFrustum::Perspective(mut f) => {
                f.near = near;
                f.far = far;
                renderer::UniformFrustum::Perspective(f)
            }
            renderer::UniformFrustum::PerspectiveOffCenter(mut f) => {
                let scale = near / f.near;
                f.left *= scale;
                f.right *= scale;
                f.top *= scale;
                f.bottom *= scale;
                f.near = near;
                f.far = far;
                renderer::UniformFrustum::PerspectiveOffCenter(f)
            }
            renderer::UniformFrustum::Orthographic(mut f) => {
                f.near = near;
                f.far = far;
                renderer::UniformFrustum::Orthographic(f)
            }
            renderer::UniformFrustum::OrthographicOffCenter(mut f) => {
                f.near = near;
                f.far = far;
                renderer::UniformFrustum::OrthographicOffCenter(f)
            }
        };
        let mut state = self.context.uniform_state();
        state.update_camera_snapshot(&camera, frame.uniform.map_projection.ellipsoid())?;
        state.entire_frustum = frame.uniform.camera.frustum.parameters().2;
        state.update_frustum_snapshot(&camera.frustum);
        Ok(())
    }
    fn execute(&mut self, command: &SceneCommand, submission: SceneSubmission) -> Result<()> {
        let mut submission = submission;
        if self.using_globe_depth {
            submission.framebuffer = self.pass_state.framebuffer.clone().map(super::FrameResource::new);
        }
        if command.pass == Pass::Cesium3DTileClassification {
            self.classification_commands.push((command.clone(), submission.clone()));
        }
        if command.pass == Pass::Translucent
            && command.metadata.depth_for_translucent_classification
            && self.oit_phase == 0
        {
            self.translucent_depth_commands.push((command.clone(), submission.clone()));
        }
        if self.oit_active && command.pass == Pass::Translucent && !command.is_clear {
            if let Some(resources) = &self.view_resources {
                if let Some(oit) = &resources.borrow().oit {
                    submission.oit = Some(oit.draw_kind(self.oit_phase));
                    submission.framebuffer = oit.framebuffer(self.oit_phase).cloned().map(super::FrameResource::new);
                }
            }
        }
        (command.execute)(submission)
    }
    fn end_frame(&mut self) -> Result<()> {
        self.context.end_frame()
    }
    fn destroy(&mut self) -> Result<()> {
        if let Some(resources) = self.view_resources.take() {
            resources.borrow_mut().destroy()?;
        }
        self.context.destroy()
    }
}
/// A primitive may produce ordinary renderer commands or perform additional
/// tileset passes. Pre/post work runs even when request-render mode skips draws.
pub trait ScenePrimitive {
    fn most_detailed_pick_targets(&self) -> Vec<super::PickedObject> {
        Vec::new()
    }
    fn update_most_detailed_pick(
        &mut self,
        _frame: &mut FrameState,
        _pass: &mut super::TilesetPassState,
        _targets: &[super::PickedObject],
    ) -> Result<bool> {
        Err(RuntimeError::new("Primitive does not implement most-detailed preload"))
    }
    fn pre_passes_update(&mut self, frame: &mut FrameState) -> Result<()>;
    fn update(&mut self, frame: &mut FrameState) -> Result<()>;
    fn post_passes_update(&mut self, frame: &mut FrameState) -> Result<()>;
    fn update_for_pass(&mut self, frame: &mut FrameState, pass: super::Cesium3DTilePass) -> Result<()>;
    fn destroy(&mut self) -> Result<()>;
}
pub trait SceneComponent {
    fn stage(&mut self, stage: SceneStage, frame: &mut FrameState) -> Result<()>;
    fn destroy(&mut self) -> Result<()>;
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SceneEventKind {
    PreUpdate,
    PostUpdate,
    PreRender,
    PostRender,
    RenderError,
}
#[derive(Clone, Debug)]
pub struct SceneNotification {
    pub kind: SceneEventKind,
    pub time: JulianDate,
    pub error: Option<String>,
}
#[derive(Clone)]
pub struct SceneRenderRequest(Arc<AtomicBool>);
impl std::task::Wake for SceneRenderRequest {
    fn wake(self: Arc<Self>) {
        self.request_render();
    }
    fn wake_by_ref(self: &Arc<Self>) {
        self.request_render();
    }
}
impl SceneRenderRequest {
    pub fn waker(&self) -> std::task::Waker {
        std::task::Waker::from(Arc::new(self.clone()))
    }
    pub fn request_render(&self) {
        self.0.store(true, Ordering::Release);
    }
}
#[derive(Clone, Debug)]
pub struct SceneOptions {
    pub order_independent_translucency: bool,
    pub request_render_mode: bool,
    pub maximum_render_time_change: Option<f64>,
    pub scene_3d_only: bool,
    pub logarithmic_depth_buffer: bool,
    pub high_dynamic_range: bool,
    pub gamma: f64,
    pub far_to_near_ratio: f64,
    pub logarithmic_depth_far_to_near_ratio: f64,
    pub near_to_far_distance_2d: f64,
    pub debug_show_frustums: bool,
    pub camera_event_wait_time: f64,
}
impl Default for SceneOptions {
    fn default() -> Self {
        Self {
            order_independent_translucency: true,
            request_render_mode: false,
            maximum_render_time_change: Some(0.),
            scene_3d_only: false,
            logarithmic_depth_buffer: true,
            high_dynamic_range: false,
            gamma: 2.2,
            far_to_near_ratio: 1000.,
            logarithmic_depth_far_to_near_ratio: 1e9,
            near_to_far_distance_2d: 1.75e6,
            debug_show_frustums: false,
            camera_event_wait_time: 500.,
        }
    }
}
pub struct Scene<B: SceneBackend> {
    pub(crate) detailed_picks: Vec<super::detailed_picking::DetailedPickJob<B>>,
    pub(crate) post_render_serial: u64,
    pub(crate) pick_offscreen_camera: Option<Camera>,
    pub(crate) pick_offscreen_view: Option<super::SceneView>,
    pub(crate) tileset_pick_pass: Option<super::Cesium3DTilePass>,
    pub(crate) metadata_pick: Option<super::PickedMetadataInfo>,
    pub use_depth_picking: bool,
    pub pick_translucent_depth: bool,
    pub(crate) pick_position_cache: std::collections::HashMap<String, Option<Cartesian3>>,
    pub backend: B,
    pub capabilities: renderer::ContextCapabilities,
    pub context_limits: renderer::ContextLimits,
    pub msaa_samples: u32,
    pub camera: Camera,
    pub view: super::SceneView,
    pub frame_state: FrameState,
    pub options: SceneOptions,
    pub background_color: Color,
    pub atmosphere: Atmosphere,
    pub fog: Fog,
    pub frame_fog: FrameFog,
    pub primitives: Vec<Box<dyn ScenePrimitive>>,
    pub components: Vec<Box<dyn SceneComponent>>,
    pub primitives_show: bool,
    pub events: Event<SceneNotification>,
    pub rethrow_render_errors: bool,
    pub minimum_disable_depth_test_distance: f64,
    pub minimum_aliased_line_width: f64,
    pub maximum_aliased_line_width: f64,
    pub occluder: Option<Occluder>,
    pub framebuffer: Option<super::FrameResource>,
    pub viewport: Option<Viewport>,
    pub scissor_test: Option<renderer::ScissorTest>,
    pub blending_enabled: Option<bool>,
    pub debug_command_filter: Option<SceneCommandFilter>,
    render_requested: SceneRenderRequest,
    last_render_time: Option<JulianDate>,
    camera_event_clock_ms: f64,
    hdr_dirty: bool,
    log_depth_dirty: bool,
    shader_frame_count: u32,
    destroyed: bool,
}
impl<B: SceneBackend> Scene<B> {
    pub fn new(
        backend: B,
        mut camera_options: CameraOptions,
        mut options: SceneOptions,
        time: JulianDate,
    ) -> Result<Self> {
        let capabilities = backend.capabilities()?;
        let context_limits = backend.limits()?;
        options.logarithmic_depth_buffer &= capabilities.fragment_depth;
        options.high_dynamic_range &=
            capabilities.depth_texture && (capabilities.color_buffer_float || capabilities.color_buffer_half_float);
        let [width, height] = backend.dimensions()?;
        camera_options.drawing_buffer_width = width as f64;
        camera_options.drawing_buffer_height = height as f64;
        let projection = Rc::clone(&camera_options.map_projection);
        let earth = camera_options.ellipsoid == Ellipsoid::WGS84;
        let mut camera = Camera::new(camera_options);
        if options.logarithmic_depth_buffer {
            if let super::CameraFrustum::Perspective(f) = &mut camera.frustum {
                f.near = 0.1;
                f.far = 1e10;
            }
        }
        camera.preload_flight_camera = Some(Box::new(camera.clone_camera()));
        let snapshot = camera.uniform_camera();
        let volume = camera.frustum.compute_culling_volume(snapshot.position, snapshot.direction, snapshot.up);
        let mut uniform = UniformFrame::new(snapshot, time);
        uniform.map_projection = projection;
        let mut frame_state = FrameState::new(uniform, volume, [width, height]);
        frame_state.context = backend.frame_context();
        frame_state.context_capabilities = capabilities.clone();
        frame_state.stencil_buffer = backend.stencil_buffer()?;
        let mut fog = Fog::default();
        fog.enabled = earth;
        let mut result = Self {
            detailed_picks: Vec::new(),
            post_render_serial: 0,
            pick_offscreen_camera: None,
            pick_offscreen_view: None,
            tileset_pick_pass: None,
            metadata_pick: None,
            use_depth_picking: true,
            pick_translucent_depth: false,
            pick_position_cache: std::collections::HashMap::new(),
            backend,
            minimum_aliased_line_width: context_limits.minimum_aliased_line_width,
            maximum_aliased_line_width: context_limits.maximum_aliased_line_width,
            msaa_samples: 4.min(context_limits.maximum_samples),
            context_limits,
            capabilities,
            camera,
            view: super::SceneView::default(),
            frame_state,
            options,
            background_color: Color::BLACK,
            atmosphere: Atmosphere::default(),
            fog,
            frame_fog: FrameFog::default(),
            primitives: Vec::new(),
            components: Vec::new(),
            primitives_show: true,
            events: Event::new(),
            rethrow_render_errors: false,
            minimum_disable_depth_test_distance: 0.,
            occluder: None,
            framebuffer: None,
            viewport: None,
            scissor_test: None,
            blending_enabled: None,
            debug_command_filter: None,
            render_requested: SceneRenderRequest(Arc::new(AtomicBool::new(true))),
            last_render_time: None,
            camera_event_clock_ms: 0.,
            hdr_dirty: false,
            log_depth_dirty: false,
            shader_frame_count: 0,
            destroyed: false,
        };
        result.frame_state.render_request = Some(result.request_handle());
        result.backend.initialize_view(&mut result.view, result.options.order_independent_translucency)?;
        result.view.initialize_camera(&result.camera);
        result.update_frame_state();
        Ok(result)
    }
    fn alive(&self) -> Result<()> {
        if self.destroyed {
            Err(RuntimeError::new("Scene has been destroyed"))
        } else {
            Ok(())
        }
    }
    pub fn request_handle(&self) -> SceneRenderRequest {
        self.render_requested.clone()
    }
    pub fn request_render(&self) {
        self.render_requested.request_render();
    }
    pub fn render_requested(&self) -> bool {
        self.render_requested.0.load(Ordering::Acquire)
    }
    pub fn last_render_time(&self) -> Option<JulianDate> {
        self.last_render_time
    }
    pub fn is_destroyed(&self) -> bool {
        self.destroyed
    }
    pub fn high_dynamic_range_supported(&self) -> bool {
        self.capabilities.depth_texture
            && (self.capabilities.color_buffer_float || self.capabilities.color_buffer_half_float)
    }
    pub fn set_high_dynamic_range(&mut self, value: bool) {
        let value = value && self.high_dynamic_range_supported();
        self.hdr_dirty = value != self.options.high_dynamic_range;
        self.options.high_dynamic_range = value;
    }
    pub fn pick_position_supported(&self) -> bool {
        self.capabilities.depth_texture
    }
    pub fn sample_height_supported(&self) -> bool {
        self.capabilities.depth_texture
    }
    pub fn clamp_to_height_supported(&self) -> bool {
        self.capabilities.depth_texture
    }
    pub fn invert_classification_supported(&self) -> bool {
        self.capabilities.depth_texture
    }
    pub fn msaa_supported(&self) -> bool {
        self.capabilities.webgl2
    }
    pub fn set_msaa_samples(&mut self, value: u32) {
        self.msaa_samples = value.min(self.context_limits.maximum_samples);
    }
    pub fn set_pixel_ratio(&mut self, value: f64) {
        self.camera.pixel_ratio = value;
        self.frame_state.pixel_ratio = value;
    }
    pub fn set_minimum_disable_depth_test_distance(&mut self, value: f64) -> Result<()> {
        if value < 0. {
            return Err(RuntimeError::new("minimumDisableDepthTestDistance must be greater than or equal to 0.0."));
        }
        self.minimum_disable_depth_test_distance = value;
        Ok(())
    }
    pub fn get_compressed_texture_format_supported(&self, format: &str) -> bool {
        match format {
            "s3tc" | "WEBGL_compressed_texture_s3tc" => self.capabilities.s3tc,
            "pvrtc" | "WEBGL_compressed_texture_pvrtc" => self.capabilities.pvrtc,
            "etc" | "WEBGL_compressed_texture_etc" => self.capabilities.etc,
            "etc1" | "WEBGL_compressed_texture_etc1" => self.capabilities.etc1,
            "astc" | "WEBGL_compressed_texture_astc" => self.capabilities.astc,
            "bc7" | "EXT_texture_compression_bptc" => self.capabilities.bc7,
            _ => false,
        }
    }
    pub fn set_logarithmic_depth_buffer(&mut self, value: bool) {
        let value = value && self.capabilities.fragment_depth;
        if value != self.options.logarithmic_depth_buffer {
            self.options.logarithmic_depth_buffer = value;
            self.log_depth_dirty = true;
        }
    }
    pub fn clamp_line_width(&self, value: f64) -> f64 {
        value.min(self.maximum_aliased_line_width).max(self.minimum_aliased_line_width)
    }
    fn notify(&mut self, kind: SceneEventKind, time: JulianDate, error: Option<String>) {
        self.events.raise_event(&SceneNotification { kind, time, error });
    }
    fn stage(&mut self, stage: SceneStage) -> Result<()> {
        for c in &mut self.components {
            c.stage(stage, &mut self.frame_state)?;
        }
        Ok(())
    }
    fn catch(&mut self, result: Result<()>, time: JulianDate) -> Result<()> {
        if let Err(error) = result {
            self.notify(SceneEventKind::RenderError, time, Some(error.to_string()));
            if self.rethrow_render_errors {
                return Err(error);
            }
        }
        Ok(())
    }
    pub fn initialize_frame(&mut self, monotonic_seconds: f64) -> Result<()> {
        self.alive()?;
        self.camera_event_clock_ms = monotonic_seconds * 1000.;
        if self.shader_frame_count == 120 {
            self.shader_frame_count = 0;
            self.stage(SceneStage::ReleaseResources)?;
        } else {
            self.shader_frame_count += 1;
        }
        self.camera.update_flights(monotonic_seconds).map_err(|e| RuntimeError::new(e.to_string()))?;
        self.camera.update(self.camera.mode()).map_err(|e| RuntimeError::new(e.to_string()))?;
        self.camera.update_camera_changed(monotonic_seconds * 1000.);
        Ok(())
    }
    pub fn update_frame_state(&mut self) {
        let frame = &mut self.frame_state;
        frame.use_depth_picking = self.use_depth_picking;
        frame.high_dynamic_range = self.options.high_dynamic_range;
        frame.msaa_samples = self.msaa_samples.max(1);
        frame.order_independent_translucency = self.options.order_independent_translucency;
        let snapshot = self.camera.uniform_camera();
        frame.command_list.clear();
        frame.executed_commands.clear();
        frame.shadow_maps.clear();
        frame.minimum_terrain_height = 0.;
        frame.minimum_disable_depth_test_distance = Some(self.minimum_disable_depth_test_distance);
        frame.maximum_screen_space_error = 2.;
        frame.atmosphere = Some(self.atmosphere);
        frame.light = Some(frame.uniform.light);
        frame.scene_3d_only = self.options.scene_3d_only;
        frame.pixel_ratio = self.camera.pixel_ratio;
        frame.occluder = self.occluder;
        frame.tileset_pass_state = None;
        frame.mode = self.camera.mode();
        frame.morph_time = frame.mode.get_morph_time().unwrap_or(frame.morph_time);
        frame.culling_volume =
            self.camera.frustum.compute_culling_volume(snapshot.position, snapshot.direction, snapshot.up);
        frame.uniform.camera = snapshot;
        frame.camera_position_local = self.camera.position;
        frame.camera_position_delta = self.camera.position_wc_delta_magnitude;
        frame.camera_position_delta_last_frame = self.camera.position_wc_delta_magnitude_last_frame;
        frame.camera_time_since_moved = self.camera.time_since_moved;
        frame.uniform.mode = frame.mode.value() as u32;
        frame.uniform.morph_time = frame.morph_time;
        frame.uniform.pixel_ratio = self.camera.pixel_ratio;
        frame.uniform.frame_number = frame.frame_number as f64;
        frame.uniform.time = frame.time;
        frame.uniform.gamma = Some(self.options.gamma);
        frame.uniform.background_color = self.background_color;
        frame.uniform.minimum_disable_depth_test_distance = self.minimum_disable_depth_test_distance;
        frame.uniform.atmosphere = Some(self.atmosphere.uniform());
        frame.use_log_depth = self.options.logarithmic_depth_buffer
            && matches!(self.camera.frustum, super::CameraFrustum::Perspective(_));
        frame.uniform.split_position = frame.split_position;
        frame.passes = FramePasses::default();
    }
    fn pre_passes(&mut self) -> Result<()> {
        self.pick_position_cache.clear();
        self.frame_state.clipping_plane_updates.clear();
        self.stage(SceneStage::ResetBudgets)?;
        for p in &mut self.primitives {
            p.pre_passes_update(&mut self.frame_state)?;
        }
        for planes in std::mem::take(&mut self.frame_state.clipping_plane_updates) {
            self.backend.update_clipping_planes(&mut planes.borrow_mut())?;
        }
        self.stage(SceneStage::PrePasses)
    }
    fn post_passes(&mut self) -> Result<()> {
        for p in &mut self.primitives {
            p.post_passes_update(&mut self.frame_state)?;
        }
        self.stage(SceneStage::PostPasses)
    }
    fn tileset_pass(&mut self, pass: super::Cesium3DTilePass) -> Result<()> {
        for p in &mut self.primitives {
            p.update_for_pass(&mut self.frame_state, pass)?;
        }
        Ok(())
    }
    pub fn render(&mut self, time: JulianDate) -> Result<bool> {
        self.alive()?;
        self.notify(SceneEventKind::PreUpdate, time, None);
        self.frame_state.new_frame = false;
        let snapshot = self.camera.uniform_camera();
        self.frame_state.culling_volume =
            self.camera.frustum.compute_culling_volume(snapshot.position, snapshot.direction, snapshot.up);
        self.frame_state.uniform.camera = snapshot;
        let camera_changed = self.view.check_for_camera_updates(
            &mut self.camera,
            self.camera_event_clock_ms,
            self.options.camera_event_wait_time,
        );
        let mut should_render = !self.options.request_render_mode
            || self.render_requested()
            || camera_changed
            || self.log_depth_dirty
            || self.hdr_dirty
            || self.camera.mode() == SceneMode::Morphing;
        if !should_render {
            if let (Some(maximum), Some(last)) = (self.options.maximum_render_time_change, self.last_render_time) {
                should_render = JulianDate::seconds_difference(&last, &time).abs() > maximum;
            }
        }
        if should_render {
            self.last_render_time = Some(time);
            self.render_requested.0.store(false, Ordering::Release);
            self.hdr_dirty = false;
            self.log_depth_dirty = false;
            self.frame_state.frame_number =
                if self.frame_state.frame_number >= 15_000_000 { 1 } else { self.frame_state.frame_number + 1 };
            self.frame_state.time = time;
            self.frame_state.new_frame = true;
        }
        let result = self.pre_passes();
        self.catch(result, time)?;
        if self.primitives_show {
            let result = self.update_most_detailed_ray_picks().and_then(|()| self.stage(SceneStage::MostDetailedPicks));
            self.catch(result, time)?;
            let result =
                self.stage(SceneStage::Preload).and_then(|_| self.tileset_pass(super::Cesium3DTilePass::Preload));
            self.catch(result, time)?;
            if self.camera.can_preload_flight() {
                // A destination pass must never reuse the current view's camera.
                if let Some(preload) = self.camera.preload_flight_camera.as_mut() {
                    let camera = preload.uniform_camera();
                    let volume = self.camera.preload_flight_culling_volume.clone();
                    let original_camera = std::mem::replace(&mut self.frame_state.uniform.camera, camera);
                    let original_volume = volume.map(|v| std::mem::replace(&mut self.frame_state.culling_volume, v));
                    let result = self
                        .stage(SceneStage::PreloadFlight)
                        .and_then(|_| self.tileset_pass(super::Cesium3DTilePass::PreloadFlight));
                    self.frame_state.uniform.camera = original_camera;
                    if let Some(volume) = original_volume {
                        self.frame_state.culling_volume = volume;
                    }
                    self.catch(result, time)?;
                }
            }
            if !should_render {
                let result = self
                    .stage(SceneStage::DeferCheck)
                    .and_then(|_| self.tileset_pass(super::Cesium3DTilePass::RequestRenderModeDeferCheck));
                self.catch(result, time)?;
            }
        }
        self.notify(SceneEventKind::PostUpdate, time, None);
        if should_render {
            self.notify(SceneEventKind::PreRender, time, None);
            self.stage(SceneStage::CreditBegin)?;
            let result = self.draw_frame();
            self.catch(result, time)?;
        }
        let result = self.post_passes();
        self.catch(result, time)?;
        // Snapshot before dispatch: newly queued work belongs to the next frame.
        let callbacks = std::mem::take(&mut self.frame_state.after_render);
        for callback in callbacks {
            if callback()? {
                self.request_render();
            }
        }
        if should_render {
            self.notify(SceneEventKind::PostRender, time, None);
            self.stage(SceneStage::CreditEnd)?;
            self.post_render_serial = self.post_render_serial.wrapping_add(1);
        }
        self.complete_most_detailed_ray_picks();
        Ok(should_render)
    }
    pub fn force_render(&mut self, time: JulianDate) -> Result<bool> {
        self.request_render();
        self.render(time)
    }
    fn draw_frame(&mut self) -> Result<()> {
        self.draw_passes(FramePasses { render: true, ..Default::default() })
    }
    /// Draw the current view with caller-supplied pass flags and target. Picking
    /// owns hit decoding/readback; this executes the same primitive and shader
    /// path as a normal frame without changing frame numbers or firing events.
    pub fn draw_passes(&mut self, passes: FramePasses) -> Result<()> {
        self.pick_position_cache.clear();
        self.draw_passes_with_culling(passes, None, None)?;
        self.backend.end_frame()
    }
    pub(crate) fn draw_passes_with_culling(
        &mut self,
        passes: FramePasses,
        culling: Option<CullingVolume>,
        clear_color: Option<Color>,
    ) -> Result<()> {
        self.alive()?;
        let [width, height] = self.backend.dimensions()?;
        self.frame_state.drawing_buffer_width = width;
        self.frame_state.drawing_buffer_height = height;
        self.update_frame_state();
        self.frame_state.passes = passes;
        if let Some(info) = &self.metadata_pick {
            self.frame_state.picking_metadata = true;
            self.frame_state.picked_metadata_info = Some(
                serde_json::json!({"schemaId":info.shader.schema_id,"className":info.shader.class_name,"propertyName":info.shader.property_name}),
            );
        }
        if let Some(culling) = culling {
            self.frame_state.culling_volume = culling;
        }
        if passes.pick {
            self.frame_state.invert_classification = false;
        }
        let mut color = clear_color.unwrap_or(self.background_color);
        if self.options.high_dynamic_range {
            color.red = color.red.powf(self.options.gamma);
            color.green = color.green.powf(self.options.gamma);
            color.blue = color.blue.powf(self.options.gamma);
        }
        self.frame_state.uniform.background_color = color;
        self.frame_state.background_color = Some(color);
        self.frame_state.tileset_pass_state =
            Some(super::TilesetPassState::new(if passes.pick || passes.depth || passes.pick_voxel {
                self.tileset_pick_pass.unwrap_or(super::Cesium3DTilePass::Pick)
            } else {
                super::Cesium3DTilePass::Render
            }));
        let camera = &self.frame_state.uniform.camera;
        self.fog.update(
            &mut self.frame_fog,
            self.frame_state.mode,
            camera.cartographic.map(|c| c.height),
            camera.position,
            camera.direction,
        );
        self.frame_state.fog = self.frame_fog;
        self.frame_state.uniform.fog_density = self.frame_fog.density;
        self.frame_state.uniform.fog_visual_density_scalar = self.frame_fog.visual_density_scalar;
        self.frame_state.uniform.fog_minimum_brightness = self.frame_fog.minimum_brightness;
        self.backend.set_render_target(self.framebuffer.as_ref(), self.viewport)?;
        self.backend.set_pass_overrides(self.scissor_test.clone(), self.blending_enabled)?;
        self.backend.begin_frame(&self.frame_state)?;
        self.stage(SceneStage::Environment)?;
        if self.primitives_show {
            for p in &mut self.primitives {
                p.update(&mut self.frame_state)?;
            }
        }
        self.backend.clear(color, None, Some(0))?;
        self.frame_state.command_list.retain(SceneCommand::shown);
        self.execute_commands()?;
        self.stage(SceneStage::Resolve)?;
        self.backend.resolve_framebuffers(&self.frame_state)?;
        if !passes.pick && !passes.pick_voxel && !passes.depth {
            self.execute_pass(Pass::Overlay, usize::MAX, 0., 0.)?;
        }
        self.stage(SceneStage::GlobeEnd)?;
        Ok(())
    }
    pub fn is_visible(&self, command: &SceneCommand) -> bool {
        command.visible_in(&self.frame_state.culling_volume, self.occluder.as_ref())
    }
    /// Canvas coordinates for placing overlays at an ellipsoid-fixed position.
    pub fn cartesian_to_canvas_coordinates(&mut self, position: Cartesian3) -> Result<Option<crate::core::Cartesian2>> {
        let state = super::SceneTransformState::from_frame(
            &self.frame_state,
            crate::core::Cartesian2::new(self.camera.client_width, self.camera.client_height),
        );
        super::SceneTransforms::world_to_window_coordinates(&state, &mut self.camera, position)
    }
    fn execute_pass(&mut self, pass: Pass, frustum: usize, near: f64, far: f64) -> Result<()> {
        let camera = self.frame_state.uniform.camera.position;
        let mut commands = if frustum == usize::MAX {
            self.frame_state.command_list.iter().filter(|c| c.pass == pass).cloned().collect::<Vec<_>>()
        } else {
            self.view.frustum_commands_list[frustum].commands[pass as usize].clone()
        };
        commands
            .retain(|c| !c.metadata.pick_only || self.frame_state.passes.pick || self.frame_state.passes.pick_voxel);
        if matches!(pass, Pass::Translucent | Pass::Voxels | Pass::GaussianSplats) {
            let front_to_back = pass == Pass::Translucent && !self.frame_state.passes.render;
            commands.sort_by(|a, b| {
                let distance = |c: &SceneCommand| {
                    if pass == Pass::GaussianSplats {
                        c.metadata.bounding_volume.map_or(0., |s| s.center.distance_squared(camera))
                    } else {
                        c.distance_squared(camera)
                    }
                };
                if front_to_back {
                    distance(a).total_cmp(&distance(b))
                } else {
                    distance(b).total_cmp(&distance(a))
                }
            });
        }
        for command in commands {
            if self.debug_command_filter.as_ref().is_some_and(|f| !f(&command)) {
                continue;
            }
            let passes = self.frame_state.passes;
            let draw_mode = if command.is_clear || matches!(pass, Pass::Compute | Pass::Environment | Pass::Overlay) {
                super::SceneDrawMode::default()
            } else {
                super::SceneDrawMode {
                    logarithmic_depth: self.frame_state.use_log_depth,
                    high_dynamic_range: self.options.high_dynamic_range
                        && !passes.pick
                        && !passes.pick_voxel
                        && !passes.depth,
                    depth_only: passes.depth,
                    pick: passes.pick && !passes.depth && !self.frame_state.picking_metadata,
                    pick_metadata: passes.pick
                        && !passes.depth
                        && self.frame_state.picking_metadata
                        && command.metadata.pick_metadata_allowed,
                }
            };
            let submission = SceneSubmission {
                pass,
                frustum,
                near,
                far,
                draw_mode,
                oit: None,
                metadata_pick: self.metadata_pick.as_ref().map(|info| info.shader.clone()),
                framebuffer: self.framebuffer.clone(),
                viewport: self.viewport,
                scissor_test: self.scissor_test.clone(),
                blending_enabled: self.blending_enabled,
                metadata: command.metadata.clone(),
                clipping_planes: command.clipping_planes.clone(),
                tileset_draw: command.tileset_draw,
            };
            if let Some(mask) = command.stencil_clear {
                self.backend.clear_stencil(0, mask)?;
            } else {
                self.backend.execute(&command, submission.clone())?;
            }
            self.frame_state.executed_commands.push(submission);
        }
        Ok(())
    }
    fn execute_commands(&mut self) -> Result<()> {
        self.view.create_potentially_visible_set(&mut self.frame_state, &self.options)?;
        self.execute_pass(Pass::Compute, usize::MAX, 0., 0.)?;
        let (_, _, range, _) = self.frame_state.uniform.camera.frustum.parameters();
        if !self.frame_state.passes.pick && !self.frame_state.passes.pick_voxel {
            self.execute_pass(Pass::Environment, usize::MAX, range[0], range[1])?;
        }
        let ranges: Vec<_> = self.view.frustum_commands_list.iter().map(|f| (f.near, f.far)).collect();
        for (index, (near, far)) in ranges.into_iter().enumerate().rev() {
            self.backend.begin_frustum(&self.frame_state, index)?;
            let opaque_near =
                if index == 0 || self.frame_state.mode == SceneMode::Scene2D { near } else { near * 0.9999 };
            self.backend.set_frustum(&self.frame_state, opaque_near, far)?;
            self.backend.clear(self.background_color, Some(1.), Some(0))?;
            for pass in [
                Pass::Globe,
                Pass::TerrainClassification,
                Pass::Cesium3DTileEdges,
                Pass::Cesium3DTile,
                Pass::Cesium3DTileClassification,
                Pass::Voxels,
                Pass::Opaque,
                Pass::GaussianSplats,
            ] {
                if self.frame_state.passes.depth
                    && matches!(pass, Pass::TerrainClassification | Pass::Cesium3DTileClassification)
                {
                    continue;
                }
                self.execute_pass(pass, index, opaque_near, far)?;
                if pass == Pass::Cesium3DTile
                    && !self.view.frustum_commands_list[index].commands[pass as usize].is_empty()
                {
                    self.backend.update_tileset_depth(&self.frame_state)?;
                }
                if pass == Pass::Globe {
                    self.backend.copy_globe_depth(&self.frame_state)?;
                }
            }
            if index != 0 && self.frame_state.mode != SceneMode::Scene2D {
                self.backend.set_frustum(&self.frame_state, near, far)?;
            }
            let has_translucent =
                !self.view.frustum_commands_list[index].commands[Pass::Translucent as usize].is_empty();
            let phases = self.backend.begin_translucent(&self.frame_state, has_translucent)?;
            for phase in 0..phases {
                self.backend.translucent_phase(phase)?;
                self.execute_pass(Pass::Translucent, index, near, far)?;
            }
            self.backend.end_translucent(&self.frame_state)?;
            self.backend.capture_pick_depth(&self.frame_state, index)?;
        }
        Ok(())
    }
    pub fn destroy(&mut self) -> Result<()> {
        if self.destroyed {
            return Ok(());
        }
        for p in &mut self.primitives {
            p.destroy()?;
        }
        for c in &mut self.components {
            c.destroy()?;
        }
        self.primitives.clear();
        self.components.clear();
        self.frame_state.after_render.clear();
        self.detailed_picks.clear();
        self.camera.cancel_flight();
        self.view.destroy()?;
        if let Some(view) = &mut self.pick_offscreen_view {
            view.destroy()?;
        }
        self.backend.destroy()?;
        self.destroyed = true;
        Ok(())
    }
}
