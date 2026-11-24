//! CesiumJS `SunPostProcess`: a lens-flare post-process that brightens the sun,
//! blurs it in two directions, and additively blends the result back over the
//! scene framebuffer. Six stages run in series; the sun position is projected
//! each frame to drive the additive-blend center/radius and the downsampled
//! scissor rectangles.

use super::{
    post_process_stage::{PostProcessStage, PostProcessStageOptions, PostProcessUniformValue},
    post_process_stage_texture_cache::{get_stage_dependencies, PostProcessStageTextureCache},
    scene_framebuffer::SceneFramebuffer,
    PostProcessStageSampleMode,
};
use crate::{
    core::{math::SOLAR_RADIUS, BoundingRectangle, Cartesian2, Cartesian3, Color, Matrix4, RuntimeError, Transforms},
    renderer::{
        Context, Framebuffer, Gl, PassState, ShaderSource, Texture, TextureBinding, UniformValue, Viewport,
        ViewportCommand, ViewportCommandOptions,
    },
    shaders::post_process_stages::{ADDITIVE_BLEND, BRIGHT_PASS, GAUSSIAN_BLUR1D, PASS_THROUGH},
};
use std::{cell::RefCell, collections::HashMap, rc::Rc};

/// Per-frame values read by the dynamic uniforms of the blur and blend stages.
struct SunState<G: Gl> {
    center: Cartesian2,
    radius: f64,
    step: f64,
    scene_color: Option<Rc<Texture<G>>>,
}

impl<G: Gl> Default for SunState<G> {
    fn default() -> Self {
        Self { center: Cartesian2::new(0.0, 0.0), radius: 0.0, step: 0.0, scene_color: None }
    }
}

/// A lens-flare post-process for the sun.
pub struct SunPostProcess<G: Gl> {
    scene_framebuffer: SceneFramebuffer<G>,
    stages: Vec<PostProcessStage<G>>,
    texture_cache: PostProcessStageTextureCache<G>,
    state: Rc<RefCell<SunState<G>>>,
    copy_command: Option<ViewportCommand<G>>,
    destroyed: bool,
}

impl<G: Gl + 'static> SunPostProcess<G> {
    pub fn new(_context: &Context<G>) -> Result<Self, RuntimeError> {
        let scale = 0.125;
        let mut stages = Vec::with_capacity(6);
        let state: Rc<RefCell<SunState<G>>> = Rc::new(RefCell::new(SunState::default()));

        // Stage 0: PassThrough, downsampled.
        stages.push(PostProcessStage::new(PostProcessStageOptions {
            fragment_shader: PASS_THROUGH.source.to_string(),
            texture_scale: Some(scale),
            force_power_of_two: Some(true),
            sample_mode: Some(PostProcessStageSampleMode::Linear),
            ..PostProcessStageOptions::new(PASS_THROUGH.source)
        })?);

        // Stage 1: BrightPass.
        let mut bright_pass = PostProcessStage::new(PostProcessStageOptions {
            fragment_shader: BRIGHT_PASS.source.to_string(),
            texture_scale: Some(scale),
            force_power_of_two: Some(true),
            ..PostProcessStageOptions::new(BRIGHT_PASS.source)
        })?;
        bright_pass.set_uniform("avgLuminance", 0.5);
        bright_pass.set_uniform("threshold", 0.25);
        bright_pass.set_uniform("offset", 0.1);
        stages.push(bright_pass);

        // Stages 2 and 3: horizontal and vertical GaussianBlur1D.
        for direction in [0.0f32, 1.0f32] {
            let mut blur = PostProcessStage::new(PostProcessStageOptions {
                fragment_shader: GAUSSIAN_BLUR1D.source.to_string(),
                texture_scale: Some(scale),
                force_power_of_two: Some(true),
                ..PostProcessStageOptions::new(GAUSSIAN_BLUR1D.source)
            })?;
            let s = state.clone();
            blur.set_uniform(
                "step",
                PostProcessUniformValue::Dynamic(Rc::new(move || {
                    let v = s.borrow().step as f32;
                    Some(PostProcessUniformValue::Vec2([v, v]))
                })),
            );
            blur.set_uniform("delta", 1.0);
            blur.set_uniform("sigma", 2.0);
            blur.set_uniform("direction", direction);
            stages.push(blur);
        }

        // Stage 4: PassThrough at full resolution.
        stages.push(PostProcessStage::new(PostProcessStageOptions {
            fragment_shader: PASS_THROUGH.source.to_string(),
            sample_mode: Some(PostProcessStageSampleMode::Linear),
            ..PostProcessStageOptions::new(PASS_THROUGH.source)
        })?);

        // Stage 5: AdditiveBlend.
        let mut additive = PostProcessStage::new(PostProcessStageOptions {
            fragment_shader: ADDITIVE_BLEND.source.to_string(),
            ..PostProcessStageOptions::new(ADDITIVE_BLEND.source)
        })?;
        let s = state.clone();
        additive.set_uniform(
            "center",
            PostProcessUniformValue::Dynamic(Rc::new(move || {
                let c = s.borrow().center;
                Some(PostProcessUniformValue::Vec2([c.x as f32, c.y as f32]))
            })),
        );
        let s = state.clone();
        additive.set_uniform(
            "radius",
            PostProcessUniformValue::Dynamic(Rc::new(move || {
                Some(PostProcessUniformValue::Float(s.borrow().radius as f32))
            })),
        );
        let s = state.clone();
        additive.set_uniform(
            "colorTexture2",
            PostProcessUniformValue::Dynamic(Rc::new(move || {
                s.borrow().scene_color.clone().map(PostProcessUniformValue::Texture)
            })),
        );
        stages.push(additive);

        Ok(Self {
            scene_framebuffer: SceneFramebuffer::new()?,
            stages,
            texture_cache: PostProcessStageTextureCache::new(),
            state,
            copy_command: None,
            destroyed: false,
        })
    }

    /// The number of stages (six).
    pub fn len(&self) -> usize {
        self.stages.len()
    }

    pub fn is_empty(&self) -> bool {
        self.stages.is_empty()
    }

    pub fn get(&self, index: usize) -> Option<&PostProcessStage<G>> {
        self.stages.get(index)
    }

    pub fn get_stage_by_name(&self, name: &str) -> Option<&PostProcessStage<G>> {
        self.stages.iter().find(|stage| stage.name() == name)
    }

    /// The projected additive-blend center and radius, for validation.
    pub fn sun_center_and_radius(&self) -> (Cartesian2, f64) {
        let state = self.state.borrow();
        (state.center, state.radius)
    }

    /// `updateSunPosition`: projects the sun and updates the blend and scissor state.
    fn update_sun_position(&mut self, context: &Context<G>, viewport: &BoundingRectangle) -> Result<(), RuntimeError> {
        let (sun_position, view, view_projection, projection) = {
            let state = context.uniform_state();
            let sun = match state.get("czm_sunPositionWC")? {
                Some(UniformValue::Float(v)) if v.len() >= 3 => Cartesian3::new(v[0] as f64, v[1] as f64, v[2] as f64),
                _ => return Ok(()),
            };
            (sun, state.view, state.projection.multiply(&state.view), state.projection)
        };
        let [drawing_width, drawing_height] = context.drawing_buffer_size().unwrap_or([1, 1]);

        let viewport_transformation = Matrix4::compute_viewport_transformation(viewport, 0.0, 1.0);
        let sun_position_ec = view.multiply_by_point(sun_position);
        let mut sun_position_wc =
            Transforms::point_to_gl_window_coordinates(&view_projection, &viewport_transformation, sun_position);

        let mut sun_limb_ec = sun_position_ec;
        sun_limb_ec.x += SOLAR_RADIUS;
        let limb_wc = Transforms::point_to_gl_window_coordinates(&projection, &viewport_transformation, sun_limb_ec);
        let sun_size = limb_wc.subtract(sun_position_wc).magnitude() * 30.0 * 2.0;

        let mut size = Cartesian2::new(sun_size, sun_size);
        {
            let mut state = self.state.borrow_mut();
            state.center = sun_position_wc;
            state.radius = size.x.max(size.y) * 0.15;
        }

        let down_sample_width = self.stages[0].output_texture().map_or(1, |texture| texture.width()) as f64;
        let down_sample_height = self.stages[0].output_texture().map_or(1, |texture| texture.height()) as f64;
        let down_sample_viewport = BoundingRectangle::new(0.0, 0.0, down_sample_width, down_sample_height);
        let down_sample_transform = Matrix4::compute_viewport_transformation(&down_sample_viewport, 0.0, 1.0);
        sun_position_wc =
            Transforms::point_to_gl_window_coordinates(&view_projection, &down_sample_transform, sun_position);

        size.x *= down_sample_width / f64::from(drawing_width);
        size.y *= down_sample_height / f64::from(drawing_height);

        let scissor = BoundingRectangle::new(
            (sun_position_wc.x - size.x * 0.5).max(0.0),
            (sun_position_wc.y - size.y * 0.5).max(0.0),
            size.x.min(f64::from(drawing_width)),
            size.y.min(f64::from(drawing_height)),
        );
        for stage in self.stages.iter_mut().take(4) {
            stage.set_scissor_rectangle(Some(scissor));
        }
        Ok(())
    }

    /// `SunPostProcess.prototype.clear`.
    pub fn clear(&mut self, context: &Context<G>, pass: &PassState<G>, clear_color: Color) -> Result<(), RuntimeError> {
        self.scene_framebuffer.clear(context, pass, clear_color)?;
        self.texture_cache.clear(context)
    }

    /// `SunPostProcess.prototype.update`: refreshes the scene framebuffer and
    /// stages, then projects the sun. Returns the scene framebuffer.
    pub fn update(
        &mut self,
        context: &Context<G>,
        viewport: Viewport,
    ) -> Result<Option<Rc<Framebuffer<G>>>, RuntimeError> {
        self.scene_framebuffer.update(context, viewport, false, 1)?;

        self.state.borrow_mut().scene_color = self.scene_framebuffer.color_texture()?.cloned();

        let active: Vec<&PostProcessStage<G>> = self.stages.iter().collect();
        let mut dependencies = HashMap::new();
        let mut previous: Option<String> = None;
        for stage in &self.stages {
            previous = get_stage_dependencies(&mut dependencies, stage, previous.as_deref());
        }
        self.texture_cache.update(context, &active, &dependencies)?;
        for stage in &mut self.stages {
            stage.set_framebuffer(
                self.texture_cache.get_framebuffer(stage.name()),
                self.texture_cache.get_output_texture(stage.name()),
            );
            stage.update(context, false)?;
        }

        // The blur step is one texel of the bright-pass output.
        let bright_width = self.stages[1].output_texture().map_or(1, |texture| texture.width());
        self.state.borrow_mut().step = 1.0 / f64::from(bright_width);

        let viewport_rectangle = BoundingRectangle::new(
            f64::from(viewport.x),
            f64::from(viewport.y),
            f64::from(viewport.width),
            f64::from(viewport.height),
        );
        self.update_sun_position(context, &viewport_rectangle)?;

        Ok(self.scene_framebuffer.framebuffer().cloned())
    }

    /// `SunPostProcess.prototype.execute`: runs the six stages in series.
    pub fn execute(&mut self, context: &Context<G>) -> Result<(), RuntimeError> {
        let scene_color = match self.scene_framebuffer.color_texture()?.cloned() {
            Some(texture) => texture,
            None => {
                let mut options = crate::renderer::TextureOptions::rgba(1, 1);
                options.sampler = crate::renderer::Sampler::NEAREST;
                Rc::new(crate::renderer::Texture::new(context, options, None)?)
            }
        };
        let resolver = |name: &str| -> Option<Rc<Texture<G>>> { self.texture_cache.get_output_texture(name) };
        let mut current = scene_color;
        for stage in &mut self.stages {
            stage.execute(context, &current, None, None, Some(&resolver))?;
            if let Some(output) = stage.output_texture() {
                current = output;
            }
        }
        Ok(())
    }

    /// `SunPostProcess.prototype.copy`: copies the last stage output to a framebuffer.
    pub fn copy(&mut self, context: &Context<G>, framebuffer: Option<Rc<Framebuffer<G>>>) -> Result<(), RuntimeError> {
        if self.copy_command.is_none() {
            self.copy_command = Some(context.create_viewport_quad_command(
                ShaderSource::new(PASS_THROUGH.source),
                ViewportCommandOptions::default(),
            )?);
        }
        if let Some(output) = self.stages.last().and_then(|stage| stage.output_texture()) {
            let command = self.copy_command.as_mut().expect("created above");
            command.options.framebuffer = framebuffer;
            command.textures = vec![("colorTexture".into(), TextureBinding::Texture(output))];
            command.execute(context, &PassState::default())?;
        }
        Ok(())
    }

    pub fn is_destroyed(&self) -> bool {
        self.destroyed
    }

    pub fn destroy(&mut self) -> Result<(), RuntimeError> {
        for stage in &mut self.stages {
            stage.destroy()?;
        }
        self.texture_cache.destroy()?;
        self.scene_framebuffer.destroy()?;
        if let Some(command) = self.copy_command.take() {
            command.shader_program.destroy()?;
        }
        self.destroyed = true;
        Ok(())
    }
}
