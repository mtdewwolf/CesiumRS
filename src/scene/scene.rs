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
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SceneSubmission {
    pub pass: Pass,
    pub frustum: usize,
    pub near: f64,
    pub far: f64,
}
pub type SceneExecute = Rc<dyn Fn(SceneSubmission) -> Result<()>>;
#[derive(Clone)]
pub struct SceneCommand {
    pub id: String,
    pub pass: Pass,
    pub metadata: renderer::DrawMetadata,
    pub execute: SceneExecute,
}
impl SceneCommand {
    pub fn new(id: impl Into<String>, pass: Pass, execute: SceneExecute) -> Self {
        Self { id: id.into(), pass, metadata: renderer::DrawMetadata::default(), execute }
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
        let copy = metadata.clone();
        Self {
            id: id.into(),
            pass,
            metadata,
            execute: Rc::new(move |_| {
                let mut command = renderer::DrawCommand::new(&vertices, &program);
                command.pass = Some(pass);
                command.render_state = state.clone();
                command.metadata = copy.clone();
                command.execute(&context, &PassState::default())
            }),
        }
    }
}
/// The GPU surface. Scene owns frame decisions and command ordering; the
/// backend applies uniforms, clears and executes the selected commands.
pub trait SceneBackend {
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
}
impl<G: Gl> RendererSceneBackend<G> {
    pub fn new(context: Context<G>) -> Self {
        Self { context, textures: FrameTextures::default(), pass_state: PassState::default() }
    }
}
impl<G: Gl> SceneBackend for RendererSceneBackend<G> {
    fn dimensions(&self) -> Result<[u32; 2]> {
        self.context.drawing_buffer_size()
    }
    fn begin_frame(&mut self, frame: &FrameState) -> Result<()> {
        let [width, height] = self.dimensions()?;
        self.pass_state.viewport = Some(Viewport { x: 0, y: 0, width, height });
        self.context.begin_frame();
        self.context.update_frame(&frame.uniform, &self.textures)
    }
    fn clear(&mut self, color: Color, depth: Option<f64>, stencil: Option<i32>) -> Result<()> {
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
        self.context.uniform_state().update_frustum_snapshot(&camera.frustum);
        Ok(())
    }
    fn execute(&mut self, command: &SceneCommand, submission: SceneSubmission) -> Result<()> {
        (command.execute)(submission)
    }
    fn end_frame(&mut self) -> Result<()> {
        self.context.end_frame()
    }
    fn destroy(&mut self) -> Result<()> {
        self.context.destroy()
    }
}
/// A primitive may produce ordinary renderer commands or perform additional
/// tileset passes. Pre/post work runs even when request-render mode skips draws.
pub trait ScenePrimitive {
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
    pub request_render_mode: bool,
    pub maximum_render_time_change: Option<f64>,
    pub scene_3d_only: bool,
    pub logarithmic_depth_buffer: bool,
    pub high_dynamic_range: bool,
    pub gamma: f64,
    pub far_to_near_ratio: f64,
    pub logarithmic_depth_far_to_near_ratio: f64,
    pub near_to_far_distance_2d: f64,
}
impl Default for SceneOptions {
    fn default() -> Self {
        Self {
            request_render_mode: false,
            maximum_render_time_change: Some(0.),
            scene_3d_only: false,
            logarithmic_depth_buffer: true,
            high_dynamic_range: false,
            gamma: 2.2,
            far_to_near_ratio: 1000.,
            logarithmic_depth_far_to_near_ratio: 1e9,
            near_to_far_distance_2d: 1.75e6,
        }
    }
}
pub struct Scene<B: SceneBackend> {
    pub backend: B,
    pub camera: Camera,
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
    render_requested: SceneRenderRequest,
    last_render_time: Option<JulianDate>,
    last_camera: Option<(Cartesian3, Cartesian3, Cartesian3, Matrix4, super::CameraFrustum)>,
    hdr_dirty: bool,
    log_depth_dirty: bool,
    shader_frame_count: u32,
    destroyed: bool,
}
impl<B: SceneBackend> Scene<B> {
    pub fn new(backend: B, mut camera_options: CameraOptions, options: SceneOptions, time: JulianDate) -> Result<Self> {
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
        let frame_state = FrameState::new(uniform, volume, [width, height]);
        let mut fog = Fog::default();
        fog.enabled = earth;
        let mut result = Self {
            backend,
            camera,
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
            minimum_aliased_line_width: 1.,
            maximum_aliased_line_width: 1.,
            occluder: None,
            render_requested: SceneRenderRequest(Arc::new(AtomicBool::new(true))),
            last_render_time: None,
            last_camera: None,
            hdr_dirty: false,
            log_depth_dirty: false,
            shader_frame_count: 0,
            destroyed: false,
        };
        result.frame_state.render_request = Some(result.request_handle());
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
    pub fn set_high_dynamic_range(&mut self, value: bool) {
        if value != self.options.high_dynamic_range {
            self.options.high_dynamic_range = value;
            self.hdr_dirty = true;
        }
    }
    pub fn set_logarithmic_depth_buffer(&mut self, value: bool) {
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
        let snapshot = self.camera.uniform_camera();
        frame.command_list.clear();
        frame.executed_commands.clear();
        frame.mode = self.camera.mode();
        frame.morph_time = frame.mode.get_morph_time().unwrap_or(frame.morph_time);
        frame.culling_volume =
            self.camera.frustum.compute_culling_volume(snapshot.position, snapshot.direction, snapshot.up);
        frame.uniform.camera = snapshot;
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
        frame.use_log_depth = self.options.logarithmic_depth_buffer;
        frame.passes = FramePasses::default();
    }
    fn pre_passes(&mut self) -> Result<()> {
        self.stage(SceneStage::ResetBudgets)?;
        for p in &mut self.primitives {
            p.pre_passes_update(&mut self.frame_state)?;
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
        let pose = (snapshot.position, snapshot.direction, snapshot.up, self.camera.transform(), self.camera.frustum);
        self.frame_state.culling_volume =
            self.camera.frustum.compute_culling_volume(snapshot.position, snapshot.direction, snapshot.up);
        self.frame_state.uniform.camera = snapshot;
        let camera_changed = self.last_camera != Some(pose);
        self.last_camera = Some(pose);
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
            let result = self.stage(SceneStage::MostDetailedPicks);
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
        }
        Ok(should_render)
    }
    pub fn force_render(&mut self, time: JulianDate) -> Result<bool> {
        self.request_render();
        self.render(time)
    }
    fn draw_frame(&mut self) -> Result<()> {
        let [width, height] = self.backend.dimensions()?;
        self.frame_state.drawing_buffer_width = width;
        self.frame_state.drawing_buffer_height = height;
        self.update_frame_state();
        self.frame_state.passes.render = true;
        let mut color = self.background_color;
        if self.options.high_dynamic_range {
            color.red = color.red.powf(self.options.gamma);
            color.green = color.green.powf(self.options.gamma);
            color.blue = color.blue.powf(self.options.gamma);
        }
        self.frame_state.uniform.background_color = color;
        let camera = &self.frame_state.uniform.camera;
        self.fog.update(
            &mut self.frame_fog,
            self.frame_state.mode,
            camera.cartographic.map(|c| c.height),
            camera.position,
            camera.direction,
        );
        self.frame_state.uniform.fog_density = self.frame_fog.density;
        self.frame_state.uniform.fog_visual_density_scalar = self.frame_fog.visual_density_scalar;
        self.frame_state.uniform.fog_minimum_brightness = self.frame_fog.minimum_brightness;
        self.backend.begin_frame(&self.frame_state)?;
        self.stage(SceneStage::Environment)?;
        if self.primitives_show {
            for p in &mut self.primitives {
                p.update(&mut self.frame_state)?;
            }
        }
        self.backend.clear(color, None, Some(0))?;
        self.execute_commands()?;
        self.stage(SceneStage::Resolve)?;
        self.execute_pass(Pass::Overlay, usize::MAX, 0., 0.)?;
        self.stage(SceneStage::GlobeEnd)?;
        self.backend.end_frame()
    }
    pub fn is_visible(&self, command: &SceneCommand) -> bool {
        let Some(bounds) = &command.metadata.bounding_volume else {
            return true;
        };
        if !command.metadata.cull {
            return true;
        }
        self.frame_state.culling_volume.compute_visibility(bounds) != Intersect::Outside
            && !(command.metadata.occlude
                && self.occluder.as_ref().is_some_and(|o| !o.is_bounding_sphere_visible(bounds)))
    }
    fn execute_pass(&mut self, pass: Pass, frustum: usize, near: f64, far: f64) -> Result<()> {
        let camera = self.frame_state.uniform.camera.position;
        let mut commands: Vec<_> = self
            .frame_state
            .command_list
            .iter()
            .filter(|c| c.pass == pass && !c.metadata.pick_only && self.is_visible(c))
            .cloned()
            .collect();
        if pass == Pass::Translucent {
            commands.sort_by(|a, b| {
                let distance =
                    |c: &SceneCommand| c.metadata.bounding_volume.map_or(0., |s| s.distance_squared_to(camera));
                distance(b).total_cmp(&distance(a))
            });
        }
        for command in commands {
            if frustum != usize::MAX {
                if let Some(bounds) = command.metadata.bounding_volume {
                    let range = bounds.compute_plane_distances(
                        self.frame_state.uniform.camera.position,
                        self.frame_state.uniform.camera.direction,
                    );
                    if range.stop < near
                        || range.start > far
                        || (command.metadata.execute_in_closest_frustum && frustum > 0 && range.start <= near)
                    {
                        continue;
                    }
                } else if command.metadata.execute_in_closest_frustum && frustum > 0 {
                    continue;
                }
            }
            let submission = SceneSubmission { pass, frustum, near, far };
            self.backend.execute(&command, submission)?;
            self.frame_state.executed_commands.push(submission);
        }
        Ok(())
    }
    fn execute_commands(&mut self) -> Result<()> {
        self.execute_pass(Pass::Compute, usize::MAX, 0., 0.)?;
        let (_, _, range, _) = self.frame_state.uniform.camera.frustum.parameters();
        self.execute_pass(Pass::Environment, usize::MAX, range[0], range[1])?;
        let ratio = if self.options.logarithmic_depth_buffer {
            self.options.logarithmic_depth_far_to_near_ratio
        } else {
            self.options.far_to_near_ratio
        };
        if ratio <= 1. || !ratio.is_finite() {
            return Err(RuntimeError::new("farToNearRatio must be finite and greater than one"));
        }
        let mut ranges = Vec::new();
        let mut near = range[0].max(1e-8);
        while near < range[1] {
            let far = if self.frame_state.mode == SceneMode::Scene2D {
                (near + self.options.near_to_far_distance_2d).min(range[1])
            } else {
                (near * ratio).min(range[1])
            };
            if far <= near {
                return Err(RuntimeError::new("invalid frustum range"));
            }
            ranges.push((near, far));
            near = far;
        }
        for (index, (near, far)) in ranges.into_iter().enumerate().rev() {
            self.backend.set_frustum(&self.frame_state, near, far)?;
            self.backend.clear(self.background_color, Some(1.), Some(0))?;
            for pass in [
                Pass::Globe,
                Pass::TerrainClassification,
                Pass::Cesium3DTileEdges,
                Pass::Cesium3DTile,
                Pass::Cesium3DTileClassification,
                Pass::Cesium3DTileClassificationIgnoreShow,
                Pass::Opaque,
                Pass::Translucent,
                Pass::Voxels,
                Pass::GaussianSplats,
            ] {
                self.execute_pass(pass, index, near, far)?;
            }
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
        self.camera.cancel_flight();
        self.backend.destroy()?;
        self.destroyed = true;
        Ok(())
    }
}
