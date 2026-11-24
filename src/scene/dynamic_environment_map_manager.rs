//! CesiumJS `DynamicEnvironmentMapManager`: generates a radiance cube map,
//! convolution mipmaps and spherical-harmonic irradiance for image-based
//! lighting from the scene's current atmosphere and light.
//!
//! Compute commands are owned by this manager and handed to the host
//! (`take_commands`) because the portable Scene command list carries draws,
//! not compute work. The state machine, dirty flags, command budgeting and CPU
//! radiance/irradiance parameters match Cesium.

use super::{frame_state::FrameState, DynamicAtmosphereLightingType, SceneMode};
use crate::{
    core::{math::EPSILON6, Cartesian2, Cartesian3, Cartesian4, Color, JulianDate, RuntimeError, Transforms},
    renderer::{
        Attachment, ComputeCommand, Context, ContextCapabilities, CubeFace, CubeMap, Framebuffer, FramebufferOptions,
        Gl, MipmapHint, PixelDatatype, ReadState, Sampler, ShaderProgram, ShaderProgramOptions, ShaderSource, Texture,
        TextureBinding, TextureMinificationFilter, TextureOptions, TextureRegion, UniformMap, UniformValue,
        VertexArray,
    },
    shaders::root::{
        ATMOSPHERE_COMMON, COMPUTE_IRRADIANCE_FS, COMPUTE_RADIANCE_MAP_FS, CONVOLVE_SPECULAR_MAP_FS,
        CONVOLVE_SPECULAR_MAP_VS,
    },
};
use std::{
    cell::Cell,
    collections::BTreeMap,
    rc::Rc,
    sync::atomic::{AtomicUsize, Ordering},
};

/// Cesium's fallback ContextLimits value before a context is created.
const DEFAULT_MAXIMUM_CUBE_MAP_SIZE: f64 = 4096.0;

static ACTIVE_COMPUTE_COMMAND_COUNT: AtomicUsize = AtomicUsize::new(0);

/// Options for [`DynamicEnvironmentMapManager::new`].
#[derive(Clone, Debug, Default)]
pub struct DynamicEnvironmentMapManagerOptions {
    pub enabled: Option<bool>,
    pub mipmap_levels: Option<u32>,
    pub maximum_seconds_difference: Option<f64>,
    pub maximum_position_epsilon: Option<f64>,
    pub atmosphere_scattering_intensity: Option<f64>,
    pub gamma: Option<f64>,
    pub brightness: Option<f64>,
    pub saturation: Option<f64>,
    pub ground_color: Option<Color>,
    pub ground_albedo: Option<f64>,
}

/// Generates environment maps and image-based lighting coefficients.
pub struct DynamicEnvironmentMapManager<G: Gl> {
    position: Option<Cartesian3>,
    radiance_map_dirty: bool,
    radiance_commands_dirty: bool,
    convolution_commands_dirty: Rc<Cell<bool>>,
    irradiance_command_dirty: bool,
    irradiance_texture_dirty: Rc<Cell<bool>>,
    spherical_harmonic_coefficients_dirty: Rc<Cell<bool>>,
    should_regenerate_shaders: Rc<Cell<bool>>,
    should_reset: bool,

    mipmap_levels: u32,
    radiance_pending: Rc<Cell<u32>>,
    convolution_pending: Rc<Cell<u32>>,
    convolution_completion: Option<Rc<Cell<bool>>>,
    queued_commands: Vec<ComputeCommand<G>>,

    irradiance_map_fs: Option<ShaderSource>,
    convolve_sp: Option<Rc<ShaderProgram<G>>>,
    va: Option<Rc<VertexArray<G>>>,

    radiance_map_textures: Vec<Option<Rc<Texture<G>>>>,
    specular_map_textures: Vec<Option<Rc<Texture<G>>>>,
    radiance_cube_map: Option<Rc<CubeMap<G>>>,
    irradiance_map_texture: Option<Rc<Texture<G>>>,

    spherical_harmonic_coefficients: [Cartesian3; 9],
    last_time: JulianDate,
    texture_dimensions: Cartesian2,
    radii_and_dynamic_atmosphere_color: Cartesian3,
    scene_environment_map: Option<super::FrameResource>,
    background_color: Option<Color>,
    owner: Option<super::FrameResource>,

    pub enabled: bool,
    pub should_update: bool,
    pub maximum_seconds_difference: f64,
    pub maximum_position_epsilon: f64,
    pub atmosphere_scattering_intensity: f64,
    pub gamma: f64,
    pub brightness: f64,
    pub saturation: f64,
    pub ground_color: Color,
    pub ground_albedo: f64,
    destroyed: bool,
}

fn option_ptr_eq<T>(a: &Option<T>, b: &Option<T>) -> bool
where
    T: PointerEquality,
{
    match (a, b) {
        (Some(a), Some(b)) => a.ptr_eq(b),
        (None, None) => true,
        _ => false,
    }
}

trait PointerEquality {
    fn ptr_eq(&self, other: &Self) -> bool;
}
impl PointerEquality for super::FrameResource {
    fn ptr_eq(&self, other: &Self) -> bool {
        super::FrameResource::ptr_eq(self, other)
    }
}

impl<G: Gl + 'static> DynamicEnvironmentMapManager<G> {
    /// `AVERAGE_EARTH_GROUND_COLOR`.
    pub const AVERAGE_EARTH_GROUND_COLOR: Color =
        Color { red: 113.0 / 255.0, green: 113.0 / 255.0, blue: 69.0 / 255.0, alpha: 1.0 };

    /// `DEFAULT_SPHERICAL_HARMONIC_COEFFICIENTS`.
    pub const DEFAULT_SPHERICAL_HARMONIC_COEFFICIENTS: [Cartesian3; 9] = [
        Cartesian3 { x: 0.35449, y: 0.35449, z: 0.35449 },
        Cartesian3::ZERO,
        Cartesian3::ZERO,
        Cartesian3::ZERO,
        Cartesian3::ZERO,
        Cartesian3::ZERO,
        Cartesian3::ZERO,
        Cartesian3::ZERO,
        Cartesian3::ZERO,
    ];

    pub fn new(options: DynamicEnvironmentMapManagerOptions) -> Self {
        let mipmap_levels =
            (f64::from(options.mipmap_levels.unwrap_or(7))).min(DEFAULT_MAXIMUM_CUBE_MAP_SIZE.log2()).floor().max(0.0)
                as u32;
        let array_length = mipmap_levels.saturating_sub(1) as usize * 6;
        let width = 2f64.powi(mipmap_levels.saturating_sub(1) as i32).max(1.0);
        Self {
            position: None,
            radiance_map_dirty: false,
            radiance_commands_dirty: false,
            convolution_commands_dirty: Rc::new(Cell::new(false)),
            irradiance_command_dirty: false,
            irradiance_texture_dirty: Rc::new(Cell::new(false)),
            spherical_harmonic_coefficients_dirty: Rc::new(Cell::new(false)),
            should_regenerate_shaders: Rc::new(Cell::new(false)),
            should_reset: false,
            mipmap_levels,
            radiance_pending: Rc::new(Cell::new(0)),
            convolution_pending: Rc::new(Cell::new(0)),
            convolution_completion: None,
            queued_commands: Vec::new(),
            irradiance_map_fs: None,
            convolve_sp: None,
            va: None,
            radiance_map_textures: vec![None; 6],
            specular_map_textures: vec![None; array_length],
            radiance_cube_map: None,
            irradiance_map_texture: None,
            spherical_harmonic_coefficients: Self::DEFAULT_SPHERICAL_HARMONIC_COEFFICIENTS,
            last_time: JulianDate::default(),
            texture_dimensions: Cartesian2::new(width, width),
            radii_and_dynamic_atmosphere_color: Cartesian3::ZERO,
            scene_environment_map: None,
            background_color: None,
            owner: None,
            enabled: options.enabled.unwrap_or(true),
            should_update: true,
            maximum_seconds_difference: options.maximum_seconds_difference.unwrap_or(60.0 * 60.0),
            maximum_position_epsilon: options.maximum_position_epsilon.unwrap_or(1000.0),
            atmosphere_scattering_intensity: options.atmosphere_scattering_intensity.unwrap_or(2.0),
            gamma: options.gamma.unwrap_or(1.0),
            brightness: options.brightness.unwrap_or(1.0),
            saturation: options.saturation.unwrap_or(1.0),
            ground_color: options.ground_color.unwrap_or(Self::AVERAGE_EARTH_GROUND_COLOR),
            ground_albedo: options.ground_albedo.unwrap_or(0.31),
            destroyed: false,
        }
    }

    pub fn owner(&self) -> Option<&super::FrameResource> {
        self.owner.as_ref()
    }

    pub fn should_regenerate_shaders(&self) -> bool {
        self.should_regenerate_shaders.get()
    }

    pub fn position(&self) -> Option<Cartesian3> {
        self.position
    }

    pub fn set_position(&mut self, value: Option<Cartesian3>) {
        if let (Some(value), Some(current)) = (value, self.position) {
            if value.equals_epsilon_with(current, 0.0, self.maximum_position_epsilon) {
                return;
            }
        } else if value.is_none() && self.position.is_none() {
            return;
        }
        self.position = value;
        self.should_reset = true;
    }

    pub fn radiance_cube_map(&self) -> Option<&Rc<CubeMap<G>>> {
        self.radiance_cube_map.as_ref()
    }

    pub fn maximum_mipmap_level(&self) -> u32 {
        self.mipmap_levels
    }

    pub fn spherical_harmonic_coefficients(&self) -> &[Cartesian3; 9] {
        &self.spherical_harmonic_coefficients
    }

    pub fn radii_and_dynamic_atmosphere_color(&self) -> Cartesian3 {
        self.radii_and_dynamic_atmosphere_color
    }

    pub fn radiance_map_dirty(&self) -> bool {
        self.radiance_map_dirty
    }

    pub fn convolution_commands_dirty(&self) -> bool {
        self.convolution_commands_dirty.get()
    }

    pub fn irradiance_texture_dirty(&self) -> bool {
        self.irradiance_texture_dirty.get()
    }

    pub fn queued_commands(&self) -> &[ComputeCommand<G>] {
        &self.queued_commands
    }

    /// Takes commands queued for the host's compute engine.
    pub fn take_commands(&mut self) -> Vec<ComputeCommand<G>> {
        std::mem::take(&mut self.queued_commands)
    }

    /// `setOwner`: installs the manager only if it has no owner, destroying the
    /// slot's previous manager.
    pub fn set_owner(
        slot: &mut Option<Box<Self>>,
        manager: Option<Box<Self>>,
        owner: &super::FrameResource,
    ) -> Result<(), RuntimeError> {
        if let Some(manager) = &manager {
            if manager.owner.as_ref().is_some_and(|current| !current.ptr_eq(owner)) {
                return Err(RuntimeError::new("DynamicEnvironmentMapManager should only be assigned to one object"));
            }
        }
        if let Some(mut previous) = slot.take() {
            previous.destroy()?;
        }
        if let Some(mut manager) = manager {
            manager.owner = Some(owner.clone());
            *slot = Some(manager);
        }
        Ok(())
    }

    /// Cancels in-progress commands and marks the radiance map dirty.
    pub fn reset(&mut self) {
        self.queued_commands.clear();
        self.radiance_map_dirty = true;
        self.radiance_commands_dirty = true;
        self.convolution_commands_dirty.set(false);
        self.irradiance_command_dirty = false;
        self.radiance_pending.set(0);
        self.convolution_pending.set(0);
    }

    pub fn is_dynamic_update_supported(capabilities: &ContextCapabilities) -> bool {
        capabilities.half_floating_point_texture || capabilities.color_buffer_float
    }

    fn atmosphere_needs_update(&mut self, frame: &FrameState) -> bool {
        let Some(position) = self.position else { return false };
        let atmosphere = frame.atmosphere.unwrap_or_default();
        let ellipsoid = frame.uniform.map_projection.ellipsoid();
        let surface_position = ellipsoid.scale_to_geodetic_surface(position);
        let radius = surface_position.map_or(ellipsoid.maximum_radius, |surface| surface.magnitude());
        let packed = Cartesian3::new(radius * 1.025, radius, atmosphere.dynamic_lighting.value() as f64);
        let changed = !packed.equals_epsilon_with(self.radii_and_dynamic_atmosphere_color, EPSILON6, EPSILON6)
            || !option_ptr_eq(&frame.environment_map, &self.scene_environment_map)
            || frame.background_color != self.background_color;
        if changed {
            self.radii_and_dynamic_atmosphere_color = packed;
            self.scene_environment_map = frame.environment_map.clone();
            self.background_color = frame.background_color;
        }
        changed
    }

    fn queue_command(&mut self, command: ComputeCommand<G>) {
        const MAXIMUM: usize = 8;
        if ACTIVE_COMPUTE_COMMAND_COUNT.load(Ordering::Relaxed) >= MAXIMUM {
            self.queued_commands.push(command);
            return;
        }
        ACTIVE_COMPUTE_COMMAND_COUNT.fetch_add(1, Ordering::Relaxed);
        self.queued_commands.push(command);
    }

    fn update_radiance_map(&mut self, context: &Context<G>, frame: &FrameState) -> Result<(), RuntimeError> {
        let texture_dimensions = self.texture_dimensions;
        if self.radiance_cube_map.is_none() {
            let mut options = TextureOptions::rgba(texture_dimensions.x as u32, texture_dimensions.y as u32);
            options.pixel_datatype = PixelDatatype::UnsignedByte;
            self.radiance_cube_map = Some(Rc::new(CubeMap::new(context, options, None)?));
        }
        if !self.radiance_commands_dirty {
            return Ok(());
        }
        let atmosphere = frame.atmosphere.unwrap_or_default();
        let mut fragment = ShaderSource {
            sources: vec![ATMOSPHERE_COMMON.source.to_string(), COMPUTE_RADIANCE_MAP_FS.source.to_string()],
            ..Default::default()
        };
        if atmosphere.requires_color_correct() {
            fragment.defines.push("ATMOSPHERE_COLOR_CORRECT".into());
        }
        if self.brightness != 1.0 || self.saturation != 1.0 || self.gamma != 1.0 {
            fragment.defines.push("ENVIRONMENT_COLOR_CORRECT".into());
        }

        let position = self.position.expect("radiance map requires a position");
        let ellipsoid = frame.uniform.map_projection.ellipsoid();
        let enu_to_fixed = Transforms::east_north_up_to_fixed_frame(position, ellipsoid);
        let radii = self.radii_and_dynamic_atmosphere_color;
        let adjustments =
            Cartesian4::new(self.brightness, self.saturation, self.gamma, self.atmosphere_scattering_intensity);
        let ground_color = self.ground_color.with_alpha(self.ground_albedo);
        let pending = self.radiance_pending.clone();
        let convolution_dirty = self.convolution_commands_dirty.clone();
        let shaders_dirty = self.should_regenerate_shaders.clone();
        pending.set(6);

        for face in CubeFace::ALL {
            let index = face.index();
            if let Some(texture) = self.radiance_map_textures[index].take() {
                let _ = texture.destroy();
            }
            let mut options = TextureOptions::rgba(texture_dimensions.x as u32, texture_dimensions.y as u32);
            options.pixel_datatype = PixelDatatype::UnsignedByte;
            let texture = Rc::new(Texture::new(context, options, None)?);
            self.radiance_map_textures[index] = Some(texture.clone());

            let direction = face.direction();
            let uniform_map: UniformMap = Rc::new(move || {
                Ok(vec![
                    (
                        "u_radiiAndDynamicAtmosphereColor".into(),
                        UniformValue::Float(vec![radii.x as f32, radii.y as f32, radii.z as f32]),
                    ),
                    (
                        "u_enuToFixedFrame".into(),
                        UniformValue::Matrix { dimension: 4, values: enu_to_fixed.0.map(|v| v as f32).to_vec() },
                    ),
                    (
                        "u_faceDirection".into(),
                        UniformValue::Float(vec![direction.x as f32, direction.y as f32, direction.z as f32]),
                    ),
                    (
                        "u_positionWC".into(),
                        UniformValue::Float(vec![position.x as f32, position.y as f32, position.z as f32]),
                    ),
                    (
                        "u_brightnessSaturationGammaIntensity".into(),
                        UniformValue::Float(vec![
                            adjustments.x as f32,
                            adjustments.y as f32,
                            adjustments.z as f32,
                            adjustments.w as f32,
                        ]),
                    ),
                    (
                        "u_groundColor".into(),
                        UniformValue::Float(vec![
                            ground_color.red as f32,
                            ground_color.green as f32,
                            ground_color.blue as f32,
                            ground_color.alpha as f32,
                        ]),
                    ),
                ])
            });
            let cube_map = self.radiance_cube_map.clone();
            let post_texture = texture.clone();
            let pending = pending.clone();
            let convolution_dirty = convolution_dirty.clone();
            let shaders_dirty = shaders_dirty.clone();
            let command = ComputeCommand {
                fragment_shader_source: Some(fragment.clone()),
                output_texture: Some(texture),
                uniform_map: Some(uniform_map),
                owner: Some("DynamicEnvironmentMapManager".into()),
                post_execute: Some(Box::new(move |_| {
                    ACTIVE_COMPUTE_COMMAND_COUNT.fetch_sub(1, Ordering::Relaxed);
                    if let Some(cube_map) = &cube_map {
                        cube_map.copy_face(post_texture.clone(), face, 0)?;
                    }
                    pending.set(pending.get().saturating_sub(1));
                    if pending.get() == 0 {
                        convolution_dirty.set(true);
                        shaders_dirty.set(true);
                    }
                    Ok(())
                })),
                ..Default::default()
            };
            self.queue_command(command);
        }
        self.radiance_commands_dirty = false;
        Ok(())
    }

    fn update_specular_maps(&mut self, context: &Context<G>) -> Result<(), RuntimeError> {
        let Some(radiance_cube_map) = self.radiance_cube_map.clone() else { return Ok(()) };
        radiance_cube_map.generate_mipmap(MipmapHint::DontCare)?;
        let mipmap_levels = self.mipmap_levels;
        let mut width = self.texture_dimensions.x as u32 / 2;
        let mut height = self.texture_dimensions.y as u32 / 2;
        let mut index = 0usize;
        let pending = self.convolution_pending.clone();
        let irradiance_dirty = Rc::new(Cell::new(false));
        let shaders_dirty = self.should_regenerate_shaders.clone();
        pending.set((mipmap_levels.saturating_sub(1)) * 6);
        for level in 1..mipmap_levels {
            for face in CubeFace::ALL {
                if let Some(texture) = self.specular_map_textures[index].take() {
                    let _ = texture.destroy();
                }
                let mut options = TextureOptions::rgba(width, height);
                options.pixel_datatype = PixelDatatype::UnsignedByte;
                let texture = Rc::new(Texture::new(context, options, None)?);
                self.specular_map_textures[index] = Some(texture.clone());

                let vertex_array = if let Some(va) = &self.va {
                    va.clone()
                } else {
                    let va = Rc::new(CubeMap::create_vertex_array(context)?);
                    self.va = Some(va.clone());
                    va
                };
                let shader_program = if let Some(program) = &self.convolve_sp {
                    program.clone()
                } else {
                    let program = Rc::new(ShaderProgram::from_sources(
                        context,
                        &ShaderProgramOptions {
                            vertex_shader_source: ShaderSource::new(CONVOLVE_SPECULAR_MAP_VS.source),
                            fragment_shader_source: ShaderSource::new(CONVOLVE_SPECULAR_MAP_FS.source),
                            attribute_locations: BTreeMap::from([("positions".to_string(), 0)]),
                        },
                    )?);
                    self.convolve_sp = Some(program.clone());
                    program
                };
                let direction = face.direction();
                let roughness = f64::from(level) / f64::from(mipmap_levels - 1);
                let uniform_map: UniformMap = Rc::new(move || {
                    Ok(vec![
                        ("u_roughness".into(), UniformValue::Float(vec![roughness as f32])),
                        (
                            "u_faceDirection".into(),
                            UniformValue::Float(vec![direction.x as f32, direction.y as f32, direction.z as f32]),
                        ),
                    ])
                });
                let texture_face = texture.clone();
                let radiance = radiance_cube_map.clone();
                let pending = pending.clone();
                let irradiance_dirty = irradiance_dirty.clone();
                let shaders_dirty = shaders_dirty.clone();
                let command = ComputeCommand {
                    vertex_array: Some(vertex_array),
                    shader_program: Some(shader_program),
                    output_texture: Some(texture),
                    uniform_map: Some(uniform_map),
                    samplers: vec![("u_radianceTexture".into(), TextureBinding::CubeMap(radiance.clone()))],
                    persists: true,
                    owner: Some("DynamicEnvironmentMapManager".into()),
                    post_execute: Some(Box::new(move |_| {
                        ACTIVE_COMPUTE_COMMAND_COUNT.fetch_sub(1, Ordering::Relaxed);
                        radiance.copy_face(texture_face.clone(), face, level)?;
                        pending.set(pending.get().saturating_sub(1));
                        if pending.get() == 0 {
                            irradiance_dirty.set(true);
                            radiance.set_sampler(Sampler {
                                minification_filter: TextureMinificationFilter::LinearMipmapLinear,
                                ..Default::default()
                            })?;
                            shaders_dirty.set(true);
                        }
                        Ok(())
                    })),
                    ..Default::default()
                };
                self.queue_command(command);
                index += 1;
            }
            width /= 2;
            height /= 2;
        }
        self.convolution_commands_dirty.set(false);
        // When the last face completes, the compute post-execute requests the
        // irradiance pass. Keep one shared flag for the next update.
        if self.convolution_pending.get() == 0 {
            self.irradiance_command_dirty = true;
        }
        self.convolution_completion = Some(irradiance_dirty);
        Ok(())
    }

    fn update_irradiance_resources(&mut self, context: &Context<G>) -> Result<(), RuntimeError> {
        if let Some(texture) = self.irradiance_map_texture.take() {
            let _ = texture.destroy();
        }
        let mut options = TextureOptions::rgba(3, 3);
        options.pixel_datatype = PixelDatatype::Float;
        let texture = Rc::new(Texture::new(context, options, None)?);
        self.irradiance_map_texture = Some(texture.clone());
        let fragment =
            self.irradiance_map_fs.clone().unwrap_or_else(|| ShaderSource::new(COMPUTE_IRRADIANCE_FS.source));
        self.irradiance_map_fs = Some(fragment.clone());
        let radiance = self.radiance_cube_map.clone();
        let spherical_dirty = self.spherical_harmonic_coefficients_dirty.clone();
        let irradiance_texture_dirty = self.irradiance_texture_dirty.clone();
        let command = ComputeCommand {
            fragment_shader_source: Some(fragment),
            output_texture: Some(texture),
            owner: Some("DynamicEnvironmentMapManager".into()),
            samplers: radiance
                .map(|radiance| vec![("u_radianceMap".into(), TextureBinding::CubeMap(radiance))])
                .unwrap_or_default(),
            post_execute: Some(Box::new(move |_| {
                ACTIVE_COMPUTE_COMMAND_COUNT.fetch_sub(1, Ordering::Relaxed);
                irradiance_texture_dirty.set(false);
                spherical_dirty.set(true);
                Ok(())
            })),
            ..Default::default()
        };
        self.queue_command(command);
        self.irradiance_texture_dirty.set(true);
        self.irradiance_command_dirty = false;
        Ok(())
    }

    /// `update`: advances the dirty-flag state machine. Commands are collected
    /// in `queued_commands`; execute them with [`ComputeEngine`](crate::renderer::ComputeEngine).
    pub fn update(&mut self, frame: &FrameState) -> Result<(), RuntimeError> {
        if self.destroyed {
            return Err(RuntimeError::new("This object was destroyed, i.e., destroy() was called."));
        }
        let context = frame.context.as_ref().and_then(|resource| resource.get::<Context<G>>()).cloned();
        let supported = context.as_ref().is_some_and(|context| {
            context.capabilities().is_ok_and(|capabilities| Self::is_dynamic_update_supported(&capabilities))
        }) && self.mipmap_levels >= 1;
        if !supported
            || !self.enabled
            || !self.should_update
            || self.position.is_none()
            || frame.mode == SceneMode::Morphing
        {
            self.should_regenerate_shaders.set(false);
            return Ok(());
        }

        let dynamic_lighting = frame
            .atmosphere
            .as_ref()
            .map_or(DynamicAtmosphereLightingType::None, |atmosphere| atmosphere.dynamic_lighting);
        let regenerate_environment_map = self.atmosphere_needs_update(frame)
            || (dynamic_lighting == DynamicAtmosphereLightingType::Sunlight
                && !frame.uniform.time.equals_epsilon(&self.last_time, self.maximum_seconds_difference));

        if self.should_reset || regenerate_environment_map {
            self.reset();
            self.should_reset = false;
            self.last_time = frame.uniform.time;
            return Ok(());
        }

        if self.convolution_completion.as_ref().is_some_and(|flag| flag.get()) {
            self.irradiance_command_dirty = true;
            self.convolution_completion = None;
        }

        let Some(context) = context else { return Ok(()) };
        if self.radiance_map_dirty {
            self.update_radiance_map(&context, frame)?;
            self.radiance_map_dirty = false;
        }
        if self.convolution_commands_dirty.get() {
            self.update_specular_maps(&context)?;
        }
        if self.irradiance_command_dirty {
            self.update_irradiance_resources(&context)?;
        }
        if self.irradiance_texture_dirty.get() {
            self.should_regenerate_shaders.set(false);
            return Ok(());
        }
        if self.spherical_harmonic_coefficients_dirty.get() {
            self.update_spherical_harmonic_coefficients(&context)?;
            return Ok(());
        }
        self.should_regenerate_shaders.set(false);
        Ok(())
    }

    fn update_spherical_harmonic_coefficients(&mut self, context: &Context<G>) -> Result<(), RuntimeError> {
        let Some(texture) = self.irradiance_map_texture.as_ref().cloned() else {
            return Ok(());
        };
        let framebuffer = Framebuffer::new(
            context,
            FramebufferOptions {
                colors: vec![Attachment::Texture(texture)],
                destroy_attachments: false,
                ..Default::default()
            },
        )?;
        let readback = context.read_pixels_from(ReadState {
            region: TextureRegion { x: 0, y: 0, width: 3, height: 3 },
            framebuffer: Some(&framebuffer),
        })?;
        let bytes = &readback.bytes;
        for (index, coefficient) in self.spherical_harmonic_coefficients.iter_mut().enumerate() {
            let offset = index * 16;
            if offset + 12 > bytes.len() {
                break;
            }
            let component =
                |i: usize| f32::from_le_bytes(bytes[offset + i * 4..offset + i * 4 + 4].try_into().unwrap()) as f64;
            let value = Cartesian3::new(component(0), component(1), component(2))
                .multiply_by_scalar(self.atmosphere_scattering_intensity);
            *coefficient = value;
        }
        framebuffer.destroy()?;
        if let Some(texture) = self.irradiance_map_texture.take() {
            let _ = texture.destroy();
        }
        self.spherical_harmonic_coefficients_dirty.set(false);
        self.should_regenerate_shaders.set(true);
        Ok(())
    }

    pub fn is_destroyed(&self) -> bool {
        self.destroyed
    }

    pub fn destroy(&mut self) -> Result<(), RuntimeError> {
        self.queued_commands.clear();
        self.radiance_pending.set(0);
        self.convolution_pending.set(0);
        for texture in &mut self.radiance_map_textures {
            if let Some(texture) = texture.take() {
                if !texture.is_destroyed() {
                    let _ = texture.destroy();
                }
            }
        }
        for texture in &mut self.specular_map_textures {
            if let Some(texture) = texture.take() {
                if !texture.is_destroyed() {
                    let _ = texture.destroy();
                }
            }
        }
        if let Some(cube_map) = self.radiance_cube_map.take() {
            let _ = cube_map.destroy();
        }
        if let Some(texture) = self.irradiance_map_texture.take() {
            if !texture.is_destroyed() {
                let _ = texture.destroy();
            }
        }
        self.va = None;
        self.convolve_sp = None;
        self.destroyed = true;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::renderer::GlValue;

    struct TestGl;
    impl Gl for TestGl {
        type Object = u32;
        fn drawing_buffer_size(&self) -> Result<[u32; 2], RuntimeError> {
            Ok([1, 1])
        }
        fn invoke(&self, method: &str, _: &[GlValue<u32>]) -> Result<GlValue<u32>, RuntimeError> {
            Ok(match method {
                "getParameter" => GlValue::Number(16384.0),
                "getExtension" => GlValue::Object(1),
                "getShaderParameter" | "getProgramParameter" => GlValue::Bool(true),
                "checkFramebufferStatus" => GlValue::Number(36053.0),
                _ if method.starts_with("create") => GlValue::Object(1),
                _ => GlValue::Null,
            })
        }
    }

    #[test]
    fn irradiance_completion_clears_pending_readback_gate() {
        let context = Context::new_webgl2(TestGl);
        let mut manager = DynamicEnvironmentMapManager::<TestGl>::new(DynamicEnvironmentMapManagerOptions::default());
        manager.update_irradiance_resources(&context).unwrap();
        assert!(manager.irradiance_texture_dirty());
        let mut command = manager.take_commands().remove(0);
        let output = command.output_texture.clone().unwrap();
        let mut post = command.post_execute.take().unwrap();
        post(&output).unwrap();
        assert!(!manager.irradiance_texture_dirty());
        assert!(manager.spherical_harmonic_coefficients_dirty.get());
    }
}
