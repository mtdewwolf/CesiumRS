//! CesiumJS `Sun`: draws a sun billboard with a procedural glow texture.
//! Only supported in 3D and Columbus view.
//!
//! A `ComputeCommand` (`SunTextureFS`) renders the disc/glow into a square
//! texture the first time (or after size, glow or HDR changes), and a
//! `SceneCommand` (`SunVS`/`SunFS`) draws a two-triangle quad with normalized
//! `direction` attributes, sized in window coordinates.

use super::{
    blending_state,
    scene::{Result, SceneCommand, SceneDrawOptions},
    scene_transforms::{SceneTransformState, SceneTransforms},
    FrameState, SceneMode,
};
use crate::{
    core::{
        math::SOLAR_RADIUS, simon1994_planetary_positions::compute_sun_position_in_earth_inertial_frame,
        BoundingRectangle, BoundingSphere, Cartesian2, Cartesian3, Cartesian4, ComponentDatatype, IndexDatatype,
        RuntimeError, Transforms,
    },
    renderer::{
        Buffer, BufferData, BufferKind, BufferUsage, ComputeCommand, Context, DrawMetadata, Gl, Pass, PixelDatatype,
        ShaderProgram, ShaderProgramOptions, ShaderSource, Texture, TextureOptions, UniformMap, UniformValue,
        VertexArray, VertexAttribute, Viewport,
    },
    shaders::root::{SUN_FS, SUN_TEXTURE_FS, SUN_VS},
};
use std::{collections::BTreeMap, rc::Rc};

/// The commands a [`Sun`] produces for one frame: a draw command and, while
/// the glow texture is being regenerated, a compute command.
pub struct SunCommands<G: Gl> {
    pub draw_command: SceneCommand,
    pub compute_command: Option<ComputeCommand<G>>,
}

/// Draws a sun billboard.
pub struct Sun<G: Gl> {
    pub show: bool,
    context: Context<G>,
    glow_factor: f64,
    glow_factor_dirty: bool,
    texture: Option<Rc<Texture<G>>>,
    drawing_buffer_width: Option<u32>,
    drawing_buffer_height: Option<u32>,
    glow_length_ts: f64,
    size: f64,
    radius_ts: f64,
    use_hdr: Option<bool>,
    vertex_array: Option<Rc<VertexArray<G>>>,
    shader_program: Option<Rc<ShaderProgram<G>>>,
    bounding_volume: BoundingSphere,
    bounding_volume_2d: BoundingSphere,
    compute_command: Option<ComputeCommand<G>>,
    destroyed: bool,
}

impl<G: Gl + 'static> Sun<G> {
    pub fn new(context: Context<G>) -> Self {
        Self {
            show: true,
            context,
            glow_factor: 1.0,
            glow_factor_dirty: false,
            texture: None,
            drawing_buffer_width: None,
            drawing_buffer_height: None,
            glow_length_ts: 0.0,
            size: 0.0,
            radius_ts: 0.0,
            use_hdr: None,
            vertex_array: None,
            shader_program: None,
            bounding_volume: BoundingSphere::new(Cartesian3::ZERO, 0.0),
            bounding_volume_2d: BoundingSphere::new(Cartesian3::ZERO, 0.0),
            compute_command: None,
            destroyed: false,
        }
    }

    /// Controls how bright the lens flare appears; zero shows only the disc.
    pub fn glow_factor(&self) -> f64 {
        self.glow_factor
    }

    pub fn set_glow_factor(&mut self, glow_factor: f64) {
        self.glow_factor = glow_factor.max(0.0);
        self.glow_factor_dirty = true;
    }

    /// The generated glow texture, if it has been created.
    pub fn texture(&self) -> Option<&Rc<Texture<G>>> {
        self.texture.as_ref()
    }

    /// The pending glow-texture compute command, if the texture must be regenerated.
    pub fn compute_command(&self) -> Option<&ComputeCommand<G>> {
        self.compute_command.as_ref()
    }

    /// Takes the pending compute command so the caller can execute it once.
    pub fn take_compute_command(&mut self) -> Option<ComputeCommand<G>> {
        self.compute_command.take()
    }

    /// The computed on-screen size (in pixels) of the sun billboard.
    pub fn size(&self) -> f64 {
        self.size
    }

    /// `_glowLengthTS`.
    pub fn glow_length_ts(&self) -> f64 {
        self.glow_length_ts
    }

    /// The bounding volume of the sun billboard in world coordinates.
    pub fn bounding_volume(&self) -> &BoundingSphere {
        &self.bounding_volume
    }

    /// The bounding volume used in Columbus view.
    pub fn bounding_volume_2d(&self) -> &BoundingSphere {
        &self.bounding_volume_2d
    }

    fn regenerate_texture(&mut self, width: u32, height: u32, use_hdr: bool) -> Result<()> {
        if let Some(texture) = self.texture.take() {
            let _ = texture.destroy();
        }
        self.drawing_buffer_width = Some(width);
        self.drawing_buffer_height = Some(height);
        self.glow_factor_dirty = false;
        self.use_hdr = Some(use_hdr);

        let mut size = f64::from(width.max(height)).log2().ceil().exp2() / 4.0;
        // The size can be less than one on tiny drawing buffers; clamp to 1 to
        // avoid invalid textures.
        size = size.max(1.0);

        let capabilities = self.context.capabilities()?;
        let pixel_datatype = if use_hdr {
            if capabilities.half_floating_point_texture {
                PixelDatatype::HalfFloat
            } else {
                PixelDatatype::Float
            }
        } else {
            PixelDatatype::UnsignedByte
        };
        let mut options = TextureOptions::rgba(size as u32, size as u32);
        options.pixel_datatype = pixel_datatype;
        let texture = Rc::new(Texture::new(&self.context, options, None)?);

        self.glow_length_ts = self.glow_factor * 5.0;
        self.radius_ts = (1.0 / (1.0 + 2.0 * self.glow_length_ts)) * 0.5;

        let radius_ts = self.radius_ts;
        let uniform_map: UniformMap =
            Rc::new(move || Ok(vec![("u_radiusTS".into(), UniformValue::Float(vec![radius_ts as f32]))]));
        self.compute_command = Some(ComputeCommand {
            fragment_shader_source: Some(ShaderSource::new(SUN_TEXTURE_FS.source)),
            output_texture: Some(texture.clone()),
            uniform_map: Some(uniform_map),
            persists: false,
            owner: Some("Sun".into()),
            ..Default::default()
        });
        self.texture = Some(texture);
        Ok(())
    }

    fn build_draw_resources(&mut self) -> Result<()> {
        if self.vertex_array.is_none() {
            // Two triangles covering the screen quad, direction in [0, 1].
            let directions: [u8; 8] = [0, 0, 255, 0, 255, 255, 0, 255];
            let vertex_buffer = Buffer::new(
                &self.context,
                BufferKind::Vertex,
                BufferUsage::StaticDraw,
                BufferData::Bytes(&directions),
            )?;
            let index_buffer = Buffer::new(
                &self.context,
                BufferKind::Index(IndexDatatype::UnsignedShort),
                BufferUsage::StaticDraw,
                BufferData::Bytes(&[0u8, 0, 1, 0, 2, 0, 0, 0, 2, 0, 3, 0]),
            )?;
            let mut direction = VertexAttribute::buffer(0, vertex_buffer, 2);
            direction.datatype = ComponentDatatype::UnsignedByte;
            direction.normalize = true;
            let vertices = VertexArray::new(&self.context, vec![direction], Some(index_buffer))?;
            self.vertex_array = Some(Rc::new(vertices));
        }
        if self.shader_program.is_none() {
            let program = ShaderProgram::from_sources(
                &self.context,
                &ShaderProgramOptions {
                    vertex_shader_source: ShaderSource::new(SUN_VS.source),
                    fragment_shader_source: ShaderSource::new(SUN_FS.source),
                    attribute_locations: BTreeMap::from([("direction".to_string(), 0)]),
                },
            )?;
            self.shader_program = Some(Rc::new(program));
        }
        Ok(())
    }

    /// `Sun.prototype.update`. `viewport` is the pass viewport (in drawing
    /// buffer pixels) and `use_hdr` selects the texture datatype.
    pub fn update(&mut self, frame: &FrameState, viewport: Viewport, use_hdr: bool) -> Result<Option<SunCommands<G>>> {
        if self.destroyed {
            return Err(RuntimeError::new("This object was destroyed, i.e., destroy() was called."));
        }
        if !self.show {
            return Ok(None);
        }
        if matches!(frame.mode, SceneMode::Scene2D | SceneMode::Morphing) {
            return Ok(None);
        }
        if !frame.passes.render {
            return Ok(None);
        }

        if self.texture.is_none()
            || self.drawing_buffer_width != Some(viewport.width)
            || self.drawing_buffer_height != Some(viewport.height)
            || self.glow_factor_dirty
            || self.use_hdr != Some(use_hdr)
        {
            self.regenerate_texture(viewport.width, viewport.height, use_hdr)?;
        }
        self.build_draw_resources()?;

        // The sun position is a scene automatic uniform; compute it from the
        // frame's time and celestial rotation rather than a stale context.
        let rotation = frame
            .uniform
            .celestial_rotation
            .unwrap_or_else(|| Transforms::compute_teme_to_pseudo_fixed_matrix(&frame.uniform.time));
        let sun_position =
            rotation.multiply_by_vector(compute_sun_position_in_earth_inertial_frame(&frame.uniform.time));
        let sun_position_cv = frame
            .uniform
            .map_projection
            .ellipsoid()
            .cartesian_to_cartographic(sun_position)
            .map(|cartographic| frame.uniform.map_projection.project(&cartographic))
            .unwrap_or(Cartesian3::ZERO);

        self.bounding_volume.center = sun_position;
        self.bounding_volume_2d.center = Cartesian3::new(sun_position_cv.z, sun_position_cv.x, sun_position_cv.y);
        let radius = SOLAR_RADIUS + SOLAR_RADIUS * self.glow_length_ts;
        self.bounding_volume.radius = radius;
        self.bounding_volume_2d.radius = radius;

        let (projection, camera_position) =
            (frame.uniform.camera.frustum.parameters().0, frame.uniform.camera.position);
        let transform_state = SceneTransformState::from_frame(
            frame,
            Cartesian2::new(f64::from(viewport.width), f64::from(viewport.height)),
        );
        let Some(position) = SceneTransforms::compute_actual_ellipsoid_position(&transform_state, sun_position) else {
            return Ok(None);
        };
        let distance = (position - camera_position).magnitude();

        let viewport_rectangle = BoundingRectangle::new(
            f64::from(viewport.x),
            f64::from(viewport.y),
            f64::from(viewport.width),
            f64::from(viewport.height),
        );
        let mut position_ec = Cartesian4::new(0.0, 0.0, -distance, 1.0);
        let position_cc = projection.multiply_by_vector(position_ec);
        let position_wc = SceneTransforms::clip_to_gl_window_coordinates(&viewport_rectangle, position_cc);

        position_ec.x = SOLAR_RADIUS;
        let limb_cc = projection.multiply_by_vector(position_ec);
        let limb_wc = SceneTransforms::clip_to_gl_window_coordinates(&viewport_rectangle, limb_cc);

        self.size = (2.0 * (limb_wc - position_wc).magnitude() * (1.0 + 2.0 * self.glow_length_ts)).ceil();

        let vertices = self.vertex_array.as_ref().expect("built above").clone();
        let program = self.shader_program.as_ref().expect("compiled above").clone();
        let texture = self.texture.as_ref().expect("built above").clone();
        let mut options = SceneDrawOptions::new(vertices, program);
        options.primitive_type = crate::core::PrimitiveType::Triangles;
        options.state = crate::renderer::RenderState { blending: blending_state::alpha_blend(), ..Default::default() };
        options.uniforms = vec![("u_size".into(), UniformValue::Float(vec![self.size as f32]))];
        options.textures = vec![("u_texture".into(), texture as Rc<dyn crate::renderer::SampledTexture<G>>)];

        let bounding_volume =
            if frame.mode == SceneMode::Scene3D { self.bounding_volume } else { self.bounding_volume_2d };
        let metadata = DrawMetadata {
            bounding_volume: Some(bounding_volume),
            cull: false,
            owner: Some("Sun".into()),
            ..Default::default()
        };
        let draw_command = SceneCommand::from_draw("Sun", self.context.clone(), Pass::Environment, metadata, options);

        Ok(Some(SunCommands { draw_command, compute_command: self.compute_command.take() }))
    }

    pub fn is_destroyed(&self) -> bool {
        self.destroyed
    }

    pub fn destroy(&mut self) {
        if let Some(vertices) = self.vertex_array.take() {
            let _ = vertices.destroy();
        }
        if let Some(program) = self.shader_program.take() {
            let _ = program.destroy();
        }
        if let Some(texture) = self.texture.take() {
            let _ = texture.destroy();
        }
        self.compute_command = None;
        self.destroyed = true;
    }
}

/// Formats a sun command list for callers that fold the compute command into
/// the frame's compute queue. Kept here so Scene has no Sun-specific branch.
pub fn split_commands<G: Gl>(commands: SunCommands<G>) -> (SceneCommand, Option<ComputeCommand<G>>) {
    (commands.draw_command, commands.compute_command)
}

/// `Matrix3` rotation reused by GPU-free callers that need the sun direction.
pub fn sun_position(frame: &FrameState) -> Cartesian3 {
    let rotation = frame
        .uniform
        .celestial_rotation
        .unwrap_or_else(|| Transforms::compute_teme_to_pseudo_fixed_matrix(&frame.uniform.time));
    rotation.multiply_by_vector(compute_sun_position_in_earth_inertial_frame(&frame.uniform.time))
}
