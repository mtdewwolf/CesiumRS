//! CesiumJS `ShadowMap`: cascaded directional, spot and omnidirectional shadow
//! maps.
//!
//! The CPU camera fitting, cascade splits, visibility/darkness rules, render
//! states, framebuffer layouts, texture offsets and cast/receive shader
//! derivation follow Cesium. Scene integration is through
//! [`ShadowMap::update`], [`ShadowMap::update_pass`] and the
//! `create_*_command` helpers; `ShadowMap` also implements [`SceneEffect`].

use super::{
    scene::{Result, SceneCommand, SceneDrawOptions, SceneEffect},
    shadow_map_shader::{self, ReceiveBias, ReceiveOptions},
    CullFace, FrameState,
};
use crate::{
    core::{
        webgl_constants as gl, BoundingSphere, Cartesian2, Cartesian3, Cartesian4, Cartographic, CullingVolume,
        Intersect, Matrix4, OrthographicFrustum, OrthographicOffCenterFrustum, PerspectiveFrustum,
        PerspectiveOffCenterFrustum, PixelFormat, RuntimeError,
    },
    renderer::{
        render_state::{ColorMask, Cull, DepthTest, PolygonOffset},
        Attachment, ClearCommand, Context, ContextCapabilities, CubeFace, CubeMap, DrawMetadata, Framebuffer,
        FramebufferOptions, Gl, Pass, PassState, PixelDatatype, RenderState, Renderbuffer, RenderbufferFormat,
        RenderbufferOptions, Sampler, ShaderProgram, ShaderProgramOptions, Texture, TextureBinding, TextureOptions,
        UniformValue, Viewport,
    },
    scene::{Camera, CameraFrustum},
};
use std::rc::Rc;

/// `ShadowMap.MAXIMUM_DISTANCE`.
pub const MAXIMUM_DISTANCE: f64 = 20000.0;

/// One of Cesium's shadow biases.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShadowBias {
    pub polygon_offset: bool,
    pub polygon_offset_factor: f64,
    pub polygon_offset_units: f64,
    pub normal_offset: bool,
    pub normal_offset_scale: f64,
    pub normal_shading: bool,
    pub normal_shading_smooth: f64,
    pub depth_bias: f64,
}

impl ShadowBias {
    fn receive(&self) -> ReceiveBias {
        ReceiveBias {
            normal_shading: self.normal_shading,
            normal_shading_smooth: self.normal_shading_smooth,
            normal_offset: self.normal_offset,
        }
    }
}

/// A frustum type the shadow camera can hold.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum ShadowMapFrustum {
    Perspective(PerspectiveFrustum),
    PerspectiveOffCenter(PerspectiveOffCenterFrustum),
    Orthographic(OrthographicFrustum),
    OrthographicOffCenter(OrthographicOffCenterFrustum),
}

impl ShadowMapFrustum {
    pub fn projection_matrix(&self) -> Matrix4 {
        match self {
            Self::Perspective(f) => f.projection_matrix(),
            Self::PerspectiveOffCenter(f) => f.projection_matrix(),
            Self::Orthographic(f) => f.projection_matrix(),
            Self::OrthographicOffCenter(f) => f.projection_matrix(),
        }
    }
    pub fn near(&self) -> f64 {
        match self {
            Self::Perspective(f) => f.near,
            Self::PerspectiveOffCenter(f) => f.near,
            Self::Orthographic(f) => f.near,
            Self::OrthographicOffCenter(f) => f.near,
        }
    }
    pub fn far(&self) -> f64 {
        match self {
            Self::Perspective(f) => f.far,
            Self::PerspectiveOffCenter(f) => f.far,
            Self::Orthographic(f) => f.far,
            Self::OrthographicOffCenter(f) => f.far,
        }
    }
    pub fn set_near_far(&mut self, near: f64, far: f64) {
        match self {
            Self::Perspective(f) => {
                f.near = near;
                f.far = far;
            }
            Self::PerspectiveOffCenter(f) => {
                f.near = near;
                f.far = far;
            }
            Self::Orthographic(f) => {
                f.near = near;
                f.far = far;
            }
            Self::OrthographicOffCenter(f) => {
                f.near = near;
                f.far = far;
            }
        }
    }
    pub fn compute_culling_volume(&self, position: Cartesian3, direction: Cartesian3, up: Cartesian3) -> CullingVolume {
        match self {
            Self::Perspective(f) => f.compute_culling_volume(position, direction, up),
            Self::PerspectiveOffCenter(f) => f.compute_culling_volume(position, direction, up),
            Self::Orthographic(f) => f.compute_culling_volume(position, direction, up),
            Self::OrthographicOffCenter(f) => f.compute_culling_volume(position, direction, up),
        }
    }
    pub fn from_uniform(frustum: &crate::renderer::UniformFrustum) -> Self {
        match frustum {
            crate::renderer::UniformFrustum::Perspective(f) => Self::Perspective(*f),
            crate::renderer::UniformFrustum::PerspectiveOffCenter(f) => Self::PerspectiveOffCenter(*f),
            crate::renderer::UniformFrustum::Orthographic(f) => Self::Orthographic(*f),
            crate::renderer::UniformFrustum::OrthographicOffCenter(f) => Self::OrthographicOffCenter(*f),
        }
    }
    pub fn from_camera(frustum: &CameraFrustum) -> Self {
        match frustum {
            CameraFrustum::Perspective(f) => Self::Perspective(*f),
            CameraFrustum::Orthographic(f) => Self::OrthographicOffCenter(f.off_center_frustum()),
            CameraFrustum::OrthographicOffCenter(f) => Self::OrthographicOffCenter(*f),
        }
    }
}

/// A camera describing the light source.
#[derive(Clone, Copy, Debug)]
pub struct ShadowMapLightCamera {
    pub view_matrix: Matrix4,
    pub inverse_view_matrix: Matrix4,
    pub frustum: ShadowMapFrustum,
    pub position_cartographic: Cartographic,
    pub position_wc: Cartesian3,
    pub direction_wc: Cartesian3,
    pub up_wc: Cartesian3,
    pub right_wc: Cartesian3,
}

impl ShadowMapLightCamera {
    pub fn new(
        position_wc: Cartesian3,
        direction_wc: Cartesian3,
        up_wc: Cartesian3,
        frustum: ShadowMapFrustum,
    ) -> Result<Self> {
        let direction = direction_wc.normalize();
        let right = direction.cross(up_wc).normalize();
        let up = right.cross(direction).normalize();
        let view = Matrix4::compute_view(position_wc, direction, up, right);
        Ok(Self {
            view_matrix: view,
            inverse_view_matrix: view.inverse()?,
            frustum,
            position_cartographic: Cartographic::default(),
            position_wc,
            direction_wc: direction,
            up_wc: up,
            right_wc: right,
        })
    }

    pub fn from_camera(camera: &Camera) -> Result<Self> {
        let view = Matrix4::compute_view(camera.position, camera.direction, camera.up, camera.right);
        Ok(Self {
            view_matrix: view,
            inverse_view_matrix: view.inverse()?,
            frustum: ShadowMapFrustum::from_camera(&camera.frustum),
            position_cartographic: camera.ellipsoid.cartesian_to_cartographic(camera.position).unwrap_or_default(),
            position_wc: camera.position,
            direction_wc: camera.direction,
            up_wc: camera.up,
            right_wc: camera.right,
        })
    }
}

/// A camera used for shadow-map fitting.
#[derive(Clone, Copy, Debug)]
pub struct ShadowMapCamera {
    pub view_matrix: Matrix4,
    pub inverse_view_matrix: Matrix4,
    pub frustum: ShadowMapFrustum,
    pub position_cartographic: Cartographic,
    pub position_wc: Cartesian3,
    pub direction_wc: Cartesian3,
    pub up_wc: Cartesian3,
    pub right_wc: Cartesian3,
    pub view_projection_matrix: Matrix4,
}

impl ShadowMapCamera {
    fn new(frustum: ShadowMapFrustum) -> Self {
        Self {
            view_matrix: Matrix4::IDENTITY,
            inverse_view_matrix: Matrix4::IDENTITY,
            frustum,
            position_cartographic: Cartographic::default(),
            position_wc: Cartesian3::ZERO,
            direction_wc: Cartesian3::UNIT_Z,
            up_wc: Cartesian3::UNIT_Y,
            right_wc: Cartesian3::UNIT_X,
            view_projection_matrix: Matrix4::IDENTITY,
        }
    }

    fn clone_from(&mut self, camera: &Self) {
        self.view_matrix = camera.view_matrix;
        self.inverse_view_matrix = camera.inverse_view_matrix;
        self.frustum = camera.frustum;
        self.position_cartographic = camera.position_cartographic;
        self.position_wc = camera.position_wc;
        self.direction_wc = camera.direction_wc;
        self.up_wc = camera.up_wc;
        self.right_wc = camera.right_wc;
    }

    pub fn get_view_projection(&mut self) -> Matrix4 {
        let scale_bias = Matrix4::new(0.5, 0.0, 0.0, 0.5, 0.0, 0.5, 0.0, 0.5, 0.0, 0.0, 0.5, 0.5, 0.0, 0.0, 0.0, 1.0);
        self.view_projection_matrix =
            scale_bias.multiply(&self.frustum.projection_matrix()).multiply(&self.view_matrix);
        self.view_projection_matrix
    }
}

/// One cascade or point-light face.
pub struct ShadowPass<G: Gl> {
    pub camera: ShadowMapCamera,
    pub framebuffer: Option<Rc<Framebuffer<G>>>,
    pub texture_offsets: Option<Matrix4>,
    pub viewport: Viewport,
    pub culling_volume: Option<CullingVolume>,
}

/// Options for [`ShadowMap::new`].
pub struct ShadowMapOptions {
    pub light_camera: ShadowMapLightCamera,
    pub enabled: Option<bool>,
    pub is_point_light: Option<bool>,
    pub point_light_radius: Option<f64>,
    pub cascades_enabled: Option<bool>,
    pub number_of_cascades: Option<u32>,
    pub maximum_distance: Option<f64>,
    pub size: Option<u32>,
    pub soft_shadows: Option<bool>,
    pub darkness: Option<f64>,
    pub normal_offset: Option<bool>,
    pub fading_enabled: Option<bool>,
    pub from_light_source: Option<bool>,
}

impl Default for ShadowMapOptions {
    fn default() -> Self {
        Self {
            light_camera: ShadowMapLightCamera::new(
                Cartesian3::ZERO,
                Cartesian3::UNIT_Z,
                Cartesian3::UNIT_Y,
                ShadowMapFrustum::Perspective(PerspectiveFrustum::new(std::f64::consts::FRAC_PI_3, 1.0)),
            )
            .expect("valid default light camera"),
            enabled: None,
            is_point_light: None,
            point_light_radius: None,
            cascades_enabled: None,
            number_of_cascades: None,
            maximum_distance: None,
            size: None,
            soft_shadows: None,
            darkness: None,
            normal_offset: None,
            fading_enabled: None,
            from_light_source: None,
        }
    }
}

/// A shadow map.
pub struct ShadowMap<G: Gl + 'static> {
    context: Context<G>,
    enabled: bool,
    soft_shadows: bool,
    normal_offset: bool,
    pub dirty: bool,
    pub from_light_source: bool,
    pub darkness: f64,
    darkness_current: f64,
    pub fading_enabled: bool,
    pub maximum_distance: f64,
    out_of_view: bool,
    out_of_view_previous: bool,
    needs_update: bool,
    polygon_offset_supported: bool,
    terrain_bias: ShadowBias,
    primitive_bias: ShadowBias,
    point_bias: ShadowBias,
    depth_attachment: Option<Attachment<G>>,
    color_attachment: Option<Attachment<G>>,
    shadow_map_matrix: Matrix4,
    shadow_map_texture: Option<TextureBinding<G>>,
    light_direction_ec: Cartesian3,
    light_position_ec: Cartesian4,
    distance: f64,
    light_camera: ShadowMapLightCamera,
    shadow_map_camera: ShadowMapCamera,
    shadow_map_culling_volume: Option<CullingVolume>,
    scene_camera: Option<ShadowMapCamera>,
    bounding_sphere: BoundingSphere,
    is_point_light: bool,
    point_light_radius: f64,
    cascades_enabled: bool,
    number_of_cascades: u32,
    fit_near_far: bool,
    maximum_cascade_distances: [f64; 4],
    texture_size: Cartesian2,
    is_spot_light: bool,
    cascade_splits: [Cartesian4; 2],
    cascade_matrices: [Matrix4; 4],
    cascade_distances: Cartesian4,
    passes: Vec<ShadowPass<G>>,
    pub debug_show: bool,
    pub debug_freeze_frame: bool,
    debug_freeze_frame_current: bool,
    debug_cascade_colors: bool,
    uses_depth_texture: bool,
    primitive_render_state: RenderState,
    terrain_render_state: RenderState,
    point_render_state: RenderState,
    clear_command: ClearCommand<G>,
    clear_pass_state: PassState<G>,
    size: u32,
    destroyed: bool,
}

impl<G: Gl + 'static> ShadowMap<G> {
    /// Constructor. `options.context` is a typed renderer context; callers with
    /// a differently typed context can use [`ShadowMap::with_context`].
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        context: Context<G>,
        light_camera: ShadowMapLightCamera,
        enabled: Option<bool>,
        is_point_light: Option<bool>,
        point_light_radius: Option<f64>,
        cascades_enabled: Option<bool>,
        number_of_cascades: Option<u32>,
        maximum_distance: Option<f64>,
        size: Option<u32>,
        soft_shadows: Option<bool>,
        darkness: Option<f64>,
        normal_offset: Option<bool>,
        fading_enabled: Option<bool>,
    ) -> Result<Self> {
        if let Some(cascades) = number_of_cascades {
            if cascades != 1 && cascades != 4 {
                return Err(RuntimeError::new("Only one or four cascades are supported."));
            }
        }
        let is_point_light = is_point_light.unwrap_or(false);
        let cascades_enabled = if is_point_light { false } else { cascades_enabled.unwrap_or(true) };
        let number_of_cascades = if cascades_enabled { number_of_cascades.unwrap_or(4) } else { 0 };
        let capabilities = context.capabilities().unwrap_or_default();
        let uses_depth_texture = capabilities.depth_texture && !is_point_light;
        let normal_offset = normal_offset.unwrap_or(true);
        let polygon_offset_supported = polygon_offset_supported(&capabilities);
        let mut shadow = Self {
            context: context.clone(),
            enabled: enabled.unwrap_or(true),
            soft_shadows: soft_shadows.unwrap_or(false),
            normal_offset,
            dirty: true,
            from_light_source: true,
            darkness: darkness.unwrap_or(0.3),
            darkness_current: darkness.unwrap_or(0.3),
            fading_enabled: fading_enabled.unwrap_or(true),
            maximum_distance: maximum_distance.unwrap_or(5000.0),
            out_of_view: false,
            out_of_view_previous: false,
            needs_update: true,
            polygon_offset_supported,
            terrain_bias: ShadowBias {
                polygon_offset: polygon_offset_supported,
                polygon_offset_factor: 1.1,
                polygon_offset_units: 4.0,
                normal_offset,
                normal_offset_scale: 0.5,
                normal_shading: true,
                normal_shading_smooth: 0.3,
                depth_bias: 0.0001,
            },
            primitive_bias: ShadowBias {
                polygon_offset: polygon_offset_supported,
                polygon_offset_factor: 1.1,
                polygon_offset_units: 4.0,
                normal_offset,
                normal_offset_scale: 0.1,
                normal_shading: true,
                normal_shading_smooth: 0.05,
                depth_bias: 0.00002,
            },
            point_bias: ShadowBias {
                polygon_offset: false,
                polygon_offset_factor: 1.1,
                polygon_offset_units: 4.0,
                normal_offset,
                normal_offset_scale: 0.0,
                normal_shading: true,
                normal_shading_smooth: 0.1,
                depth_bias: 0.0005,
            },
            depth_attachment: None,
            color_attachment: None,
            shadow_map_matrix: Matrix4::IDENTITY,
            shadow_map_texture: None,
            light_direction_ec: Cartesian3::ZERO,
            light_position_ec: Cartesian4::new(0.0, 0.0, 0.0, 0.0),
            distance: 0.0,
            light_camera,
            shadow_map_camera: ShadowMapCamera::new(if cascades_enabled {
                ShadowMapFrustum::OrthographicOffCenter(OrthographicOffCenterFrustum::new(
                    0.0, 1.0, 1.0, 0.0, 1.0, 5000.0,
                ))
            } else {
                light_camera.frustum
            }),
            shadow_map_culling_volume: None,
            scene_camera: None,
            bounding_sphere: BoundingSphere::new(Cartesian3::ZERO, 0.0),
            is_point_light,
            point_light_radius: point_light_radius.unwrap_or(100.0),
            cascades_enabled,
            number_of_cascades,
            fit_near_far: true,
            maximum_cascade_distances: [25.0, 150.0, 700.0, f64::MAX],
            texture_size: Cartesian2::new(0.0, 0.0),
            is_spot_light: !cascades_enabled
                && !is_point_light
                && light_camera.frustum.near().is_finite()
                && matches!(light_camera.frustum, ShadowMapFrustum::Perspective(f) if f.fov.is_finite()),
            cascade_splits: [Cartesian4::ZERO, Cartesian4::ZERO],
            cascade_matrices: [Matrix4::IDENTITY; 4],
            cascade_distances: Cartesian4::ZERO,
            passes: Vec::new(),
            debug_show: false,
            debug_freeze_frame: false,
            debug_freeze_frame_current: false,
            debug_cascade_colors: false,
            uses_depth_texture,
            primitive_render_state: RenderState::default(),
            terrain_render_state: RenderState::default(),
            point_render_state: RenderState::default(),
            clear_command: ClearCommand { depth: Some(1.0), color: Some([0.0, 0.0, 0.0, 0.0]), ..Default::default() },
            clear_pass_state: PassState::default(),
            size: 2048,
            destroyed: false,
        };
        let number_of_passes = if is_point_light {
            6
        } else if !cascades_enabled {
            1
        } else {
            number_of_cascades
        };
        for _ in 0..number_of_passes {
            shadow.passes.push(ShadowPass {
                camera: ShadowMapCamera::new(shadow.shadow_map_camera.frustum),
                framebuffer: None,
                texture_offsets: None,
                viewport: Viewport { x: 0, y: 0, width: 1, height: 1 },
                culling_volume: None,
            });
        }
        shadow.recreate_render_states();
        shadow.set_size(size.unwrap_or(2048));
        Ok(shadow)
    }

    /// Typed constructor options.
    pub fn with_options(context: Context<G>, options: ShadowMapOptions) -> Result<Self> {
        let mut shadow = Self::new(
            context,
            options.light_camera,
            options.enabled,
            options.is_point_light,
            options.point_light_radius,
            options.cascades_enabled,
            options.number_of_cascades,
            options.maximum_distance,
            options.size,
            options.soft_shadows,
            options.darkness,
            options.normal_offset,
            options.fading_enabled,
        )?;
        shadow.from_light_source = options.from_light_source.unwrap_or(true);
        Ok(shadow)
    }

    /// Builds a shadow map from a scene [`Camera`] light source.
    pub fn from_camera(context: Context<G>, light_camera: &Camera) -> Result<Self> {
        let light = ShadowMapLightCamera::from_camera(light_camera)?;
        Self::new(context, light, None, None, None, None, None, None, None, None, None, None, None)
    }

    fn recreate_render_states(&mut self) {
        let color_mask = !self.uses_depth_texture;
        self.primitive_render_state = create_render_state(color_mask, &self.primitive_bias);
        self.terrain_render_state = create_render_state(color_mask, &self.terrain_bias);
        self.point_render_state = create_render_state(color_mask, &self.point_bias);
    }

    pub fn set_size(&mut self, size: u32) {
        self.size = size;
        let number_of_passes = self.passes.len();
        let limits = self.context.limits().unwrap_or_default();
        let maximum_cube = limits.maximum_cube_map_size.max(1);
        let maximum_texture = limits.maximum_texture_size.max(1);
        if self.is_point_light {
            let size = size.min(maximum_cube);
            self.texture_size = Cartesian2::new(f64::from(size), f64::from(size));
            for pass in &mut self.passes {
                pass.viewport = Viewport { x: 0, y: 0, width: size, height: size };
            }
        } else if number_of_passes == 1 {
            let size = size.min(maximum_texture);
            self.texture_size = Cartesian2::new(f64::from(size), f64::from(size));
            if let Some(pass) = self.passes.first_mut() {
                pass.viewport = Viewport { x: 0, y: 0, width: size, height: size };
            }
        } else if number_of_passes == 4 {
            let size = if maximum_texture >= size.saturating_mul(2) { size } else { maximum_texture / 2 };
            self.texture_size = Cartesian2::new(f64::from(size * 2), f64::from(size * 2));
            let viewports = [
                Viewport { x: 0, y: 0, width: size, height: size },
                Viewport { x: size as i32, y: 0, width: size, height: size },
                Viewport { x: 0, y: size as i32, width: size, height: size },
                Viewport { x: size as i32, y: size as i32, width: size, height: size },
            ];
            for (pass, viewport) in self.passes.iter_mut().zip(viewports) {
                pass.viewport = viewport;
            }
        }
        self.clear_pass_state.viewport =
            Some(Viewport { x: 0, y: 0, width: self.texture_size.x as u32, height: self.texture_size.y as u32 });
        for pass in &mut self.passes {
            let viewport = pass.viewport;
            let bias_x = f64::from(viewport.x) / self.texture_size.x;
            let bias_y = f64::from(viewport.y) / self.texture_size.y;
            let scale_x = f64::from(viewport.width) / self.texture_size.x;
            let scale_y = f64::from(viewport.height) / self.texture_size.y;
            pass.texture_offsets = Some(Matrix4::new(
                scale_x, 0.0, 0.0, bias_x, 0.0, scale_y, 0.0, bias_y, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
            ));
        }
    }

    pub fn size(&self) -> u32 {
        self.size
    }
    pub fn enabled(&self) -> bool {
        self.enabled
    }
    pub fn set_enabled(&mut self, value: bool) {
        self.dirty = self.enabled != value;
        self.enabled = value;
    }
    pub fn soft_shadows(&self) -> bool {
        self.soft_shadows
    }
    pub fn set_soft_shadows(&mut self, value: bool) {
        self.dirty = self.soft_shadows != value;
        self.soft_shadows = value;
    }
    pub fn normal_offset(&self) -> bool {
        self.normal_offset
    }
    pub fn set_normal_offset(&mut self, value: bool) {
        self.dirty = self.normal_offset != value;
        self.normal_offset = value;
        self.terrain_bias.normal_offset = value;
        self.primitive_bias.normal_offset = value;
        self.point_bias.normal_offset = value;
    }
    pub fn out_of_view(&self) -> bool {
        self.out_of_view
    }
    pub fn debug_cascade_colors(&self) -> bool {
        self.debug_cascade_colors
    }
    pub fn set_debug_cascade_colors(&mut self, value: bool) {
        self.dirty = self.debug_cascade_colors != value;
        self.debug_cascade_colors = value;
    }
    pub fn passes(&self) -> &[ShadowPass<G>] {
        &self.passes
    }
    pub fn is_point_light(&self) -> bool {
        self.is_point_light
    }
    pub fn shadow_map_camera(&self) -> &ShadowMapCamera {
        &self.shadow_map_camera
    }
    pub fn cascade_splits(&self) -> [Cartesian4; 2] {
        self.cascade_splits
    }
    pub fn cascade_matrices(&self) -> [Matrix4; 4] {
        self.cascade_matrices
    }
    pub fn cascade_distances(&self) -> Cartesian4 {
        self.cascade_distances
    }
    pub fn texture_size(&self) -> Cartesian2 {
        self.texture_size
    }
    pub fn texture_offsets(&self) -> Vec<Option<Matrix4>> {
        self.passes.iter().map(|pass| pass.texture_offsets).collect()
    }
    pub fn light_direction_ec(&self) -> Cartesian3 {
        self.light_direction_ec
    }
    pub fn light_position_ec(&self) -> Cartesian4 {
        self.light_position_ec
    }
    pub fn distance(&self) -> f64 {
        self.distance
    }
    pub fn darkness_current(&self) -> f64 {
        self.darkness_current
    }
    pub fn shadow_map_culling_volume(&self) -> Option<&CullingVolume> {
        self.shadow_map_culling_volume.as_ref()
    }
    pub fn terrain_bias(&self) -> ShadowBias {
        self.terrain_bias
    }
    pub fn primitive_bias(&self) -> ShadowBias {
        self.primitive_bias
    }
    pub fn point_bias(&self) -> ShadowBias {
        self.point_bias
    }
    pub fn shadow_map_matrix(&self) -> Matrix4 {
        self.shadow_map_matrix
    }
    pub fn primitive_render_state(&self) -> &RenderState {
        &self.primitive_render_state
    }
    pub fn terrain_render_state(&self) -> &RenderState {
        &self.terrain_render_state
    }
    pub fn point_render_state(&self) -> &RenderState {
        &self.point_render_state
    }
    pub fn texture(&self) -> Option<&TextureBinding<G>> {
        self.shadow_map_texture.as_ref()
    }

    fn update_framebuffer(&mut self) -> Result<()> {
        let needs_create = self.passes.first().and_then(|pass| pass.framebuffer.as_ref()).is_none()
            || self.shadow_map_texture.as_ref().is_some_and(|texture| match texture {
                TextureBinding::Texture(texture) => f64::from(texture.width()) != self.texture_size.x,
                TextureBinding::CubeMap(texture) => f64::from(texture.width()) != self.texture_size.x,
                TextureBinding::Volume(_) => true,
            });
        if needs_create {
            self.destroy_framebuffer()?;
            self.create_framebuffer()?;
            self.check_framebuffer()?;
            self.clear_framebuffer(0)?;
        }
        Ok(())
    }

    fn destroy_framebuffer(&mut self) -> Result<()> {
        let framebuffers: Vec<_> = self.passes.iter_mut().filter_map(|pass| pass.framebuffer.take()).collect();
        for framebuffer in framebuffers {
            if !framebuffer.is_destroyed() {
                framebuffer.destroy()?;
            }
        }
        if let Some(Attachment::Texture(texture)) = self.depth_attachment.take() {
            if !texture.is_destroyed() {
                let _ = texture.destroy();
            }
        }
        if let Some(Attachment::Renderbuffer(renderbuffer)) = self.depth_attachment.take() {
            let _ = renderbuffer.destroy();
        }
        if let Some(Attachment::Texture(texture)) = self.color_attachment.take() {
            if !texture.is_destroyed() {
                let _ = texture.destroy();
            }
        }
        self.shadow_map_texture = None;
        Ok(())
    }

    fn create_framebuffer(&mut self) -> Result<()> {
        let width = self.texture_size.x as u32;
        let height = self.texture_size.y as u32;
        let depth_renderbuffer = Rc::new(Renderbuffer::new(
            &self.context,
            RenderbufferOptions {
                format: RenderbufferFormat::DepthComponent16,
                width: Some(width),
                height: Some(height),
                num_samples: 1,
            },
        )?);
        if self.is_point_light {
            let mut options = TextureOptions::rgba(width, height);
            options.pixel_format = PixelFormat::Rgba;
            options.pixel_datatype = PixelDatatype::UnsignedByte;
            options.sampler = Sampler::NEAREST;
            let cube_map = Rc::new(CubeMap::new(&self.context, options, None)?);
            let faces = [
                CubeFace::NegativeX,
                CubeFace::NegativeY,
                CubeFace::NegativeZ,
                CubeFace::PositiveX,
                CubeFace::PositiveY,
                CubeFace::PositiveZ,
            ];
            for (pass, face) in self.passes.iter_mut().zip(faces) {
                let framebuffer = Rc::new(Framebuffer::new(
                    &self.context,
                    FramebufferOptions {
                        colors: vec![Attachment::CubeFace(cube_map.face(face).clone())],
                        depth: Some(Attachment::Renderbuffer(depth_renderbuffer.clone())),
                        destroy_attachments: false,
                        ..Default::default()
                    },
                )?);
                pass.framebuffer = Some(framebuffer);
            }
            self.shadow_map_texture = Some(TextureBinding::CubeMap(cube_map));
            self.depth_attachment = Some(Attachment::Renderbuffer(depth_renderbuffer));
            return Ok(());
        }
        if self.uses_depth_texture {
            let mut options = TextureOptions::rgba(width, height);
            options.pixel_format = PixelFormat::DepthStencil;
            options.pixel_datatype = PixelDatatype::UnsignedInt24_8;
            options.sampler = Sampler::NEAREST;
            let texture = Rc::new(Texture::new(&self.context, options, None)?);
            let framebuffer = Rc::new(Framebuffer::new(
                &self.context,
                FramebufferOptions {
                    depth_stencil: Some(Attachment::Texture(texture.clone())),
                    destroy_attachments: false,
                    ..Default::default()
                },
            )?);
            for pass in &mut self.passes {
                pass.framebuffer = Some(framebuffer.clone());
            }
            self.shadow_map_texture = Some(TextureBinding::Texture(texture.clone()));
            self.depth_attachment = Some(Attachment::Texture(texture));
            return Ok(());
        }
        let mut options = TextureOptions::rgba(width, height);
        options.pixel_format = PixelFormat::Rgba;
        options.pixel_datatype = PixelDatatype::UnsignedByte;
        options.sampler = Sampler::NEAREST;
        let color_texture = Rc::new(Texture::new(&self.context, options, None)?);
        let framebuffer = Rc::new(Framebuffer::new(
            &self.context,
            FramebufferOptions {
                colors: vec![Attachment::Texture(color_texture.clone())],
                depth: Some(Attachment::Renderbuffer(depth_renderbuffer.clone())),
                destroy_attachments: false,
                ..Default::default()
            },
        )?);
        for pass in &mut self.passes {
            pass.framebuffer = Some(framebuffer.clone());
        }
        self.shadow_map_texture = Some(TextureBinding::Texture(color_texture.clone()));
        self.depth_attachment = Some(Attachment::Renderbuffer(depth_renderbuffer));
        self.color_attachment = Some(Attachment::Texture(color_texture));
        Ok(())
    }

    fn check_framebuffer(&mut self) -> Result<()> {
        if self.uses_depth_texture {
            let status = self
                .passes
                .first()
                .and_then(|pass| pass.framebuffer.as_ref())
                .map_or(Ok(gl::FRAMEBUFFER_COMPLETE), |framebuffer| framebuffer.status())?;
            if status != gl::FRAMEBUFFER_COMPLETE {
                self.uses_depth_texture = false;
                self.recreate_render_states();
                self.destroy_framebuffer()?;
                self.create_framebuffer()?;
            }
        }
        Ok(())
    }

    fn clear_framebuffer(&mut self, pass: usize) -> Result<()> {
        if self.is_point_light || pass == 0 {
            let framebuffer = self.passes.get(pass).and_then(|pass| pass.framebuffer.clone());
            self.clear_command.framebuffer = framebuffer;
            let pass_state = self.clear_pass_state.clone();
            self.clear_command.execute(&self.context, &pass_state)?;
        }
        Ok(())
    }

    /// `updatePass`: clears the shadow map's pass before rendering casters.
    pub fn update_pass(&mut self, pass: usize) -> Result<()> {
        self.clear_framebuffer(pass)
    }

    fn update_cameras(&mut self, frame: &FrameState) -> Result<()> {
        let light = self.light_camera;
        if self.cascades_enabled {
            self.shadow_map_camera.direction_wc = light.direction_wc;
        } else if self.is_point_light {
            self.shadow_map_camera.position_wc = light.position_wc;
        } else {
            self.shadow_map_camera.clone_from(&ShadowMapCamera {
                view_matrix: light.view_matrix,
                inverse_view_matrix: light.inverse_view_matrix,
                frustum: light.frustum,
                position_cartographic: light.position_cartographic,
                position_wc: light.position_wc,
                direction_wc: light.direction_wc,
                up_wc: light.up_wc,
                right_wc: light.right_wc,
                view_projection_matrix: Matrix4::IDENTITY,
            });
        }
        let camera_view = frame.uniform.camera.view;
        let mut light_direction =
            camera_view.multiply_by_point_as_vector(self.shadow_map_camera.direction_wc).normalize();
        light_direction = -light_direction;
        self.light_direction_ec = light_direction;
        let light_position = camera_view.multiply_by_point(self.shadow_map_camera.position_wc);
        self.light_position_ec =
            Cartesian4::new(light_position.x, light_position.y, light_position.z, self.point_light_radius);

        let (mut near, mut far) = if self.fit_near_far {
            let near = frame.shadow_state.near_plane.min(self.maximum_distance);
            let far = frame.shadow_state.far_plane.min(self.maximum_distance).max(near + 1.0);
            (near, far)
        } else {
            (frame.uniform.camera.frustum.parameters().2[0], self.maximum_distance)
        };
        near = near.max(0.0);
        far = far.max(near + 1.0);
        let mut scene_camera = ShadowMapCamera::new(ShadowMapFrustum::from_uniform(&frame.uniform.camera.frustum));
        scene_camera.view_matrix = frame.uniform.camera.view;
        scene_camera.inverse_view_matrix = frame.uniform.camera.inverse_view;
        scene_camera.position_wc = frame.uniform.camera.position;
        scene_camera.direction_wc = frame.uniform.camera.direction;
        scene_camera.up_wc = frame.uniform.camera.up;
        scene_camera.right_wc = frame.uniform.camera.right;
        scene_camera.frustum.set_near_far(near, far);
        self.scene_camera = Some(scene_camera);
        self.distance = far - near;

        self.check_visibility(frame);

        if !self.out_of_view_previous && self.out_of_view {
            self.needs_update = true;
        }
        self.out_of_view_previous = self.out_of_view;
        Ok(())
    }

    fn check_visibility(&mut self, frame: &FrameState) {
        let Some(scene_camera) = self.scene_camera else { return };
        let mut sphere = BoundingSphere::new(Cartesian3::ZERO, 0.0);
        if self.cascades_enabled {
            if scene_camera.frustum.near() >= self.maximum_distance {
                self.out_of_view = true;
                self.needs_update = false;
                return;
            }
            let ellipsoid = frame.uniform.map_projection.ellipsoid();
            let Some(surface_normal) = ellipsoid.geodetic_surface_normal(scene_camera.position_wc) else {
                self.out_of_view = true;
                self.needs_update = false;
                return;
            };
            let light_direction = -self.shadow_map_camera.direction_wc;
            let dot = surface_normal.dot(light_direction);
            self.darkness_current = if self.fading_enabled {
                let amount = (dot / 0.1).clamp(0.0, 1.0);
                1.0 + (self.darkness - 1.0) * amount
            } else {
                self.darkness
            };
            if dot < 0.0 {
                self.out_of_view = true;
                self.needs_update = false;
                return;
            }
            self.needs_update = true;
            self.out_of_view = false;
        } else if self.is_point_light {
            sphere.center = self.shadow_map_camera.position_wc;
            sphere.radius = self.point_light_radius;
            self.out_of_view = frame.culling_volume.compute_visibility(&sphere) == Intersect::Outside;
            self.needs_update = !self.out_of_view && !self.bounding_sphere.equals(&sphere);
            self.bounding_sphere = sphere;
        } else {
            let frustum_radius = self.shadow_map_camera.frustum.far() / 2.0;
            sphere.center = self.shadow_map_camera.position_wc
                + self.shadow_map_camera.direction_wc.multiply_by_scalar(frustum_radius);
            sphere.radius = frustum_radius;
            self.out_of_view = frame.culling_volume.compute_visibility(&sphere) == Intersect::Outside;
            self.needs_update = !self.out_of_view && !self.bounding_sphere.equals(&sphere);
            self.bounding_sphere = sphere;
        }
    }

    fn compute_omnidirectional(&mut self, frame: &FrameState) {
        let frustum = ShadowMapFrustum::Perspective(PerspectiveFrustum {
            fov: crate::core::math::PI_OVER_TWO,
            aspect_ratio: 1.0,
            near: 1.0,
            far: self.point_light_radius,
            x_offset: 0.0,
            y_offset: 0.0,
        });
        let directions = [
            Cartesian3::new(-1.0, 0.0, 0.0),
            Cartesian3::new(0.0, -1.0, 0.0),
            Cartesian3::new(0.0, 0.0, -1.0),
            Cartesian3::new(1.0, 0.0, 0.0),
            Cartesian3::new(0.0, 1.0, 0.0),
            Cartesian3::new(0.0, 0.0, 1.0),
        ];
        let ups = [
            Cartesian3::new(0.0, -1.0, 0.0),
            Cartesian3::new(0.0, 0.0, -1.0),
            Cartesian3::new(0.0, -1.0, 0.0),
            Cartesian3::new(0.0, -1.0, 0.0),
            Cartesian3::new(0.0, 0.0, 1.0),
            Cartesian3::new(0.0, -1.0, 0.0),
        ];
        let rights = [
            Cartesian3::new(0.0, 0.0, 1.0),
            Cartesian3::new(1.0, 0.0, 0.0),
            Cartesian3::new(-1.0, 0.0, 0.0),
            Cartesian3::new(0.0, 0.0, -1.0),
            Cartesian3::new(1.0, 0.0, 0.0),
            Cartesian3::new(1.0, 0.0, 0.0),
        ];
        for (index, pass) in self.passes.iter_mut().enumerate() {
            let camera = &mut pass.camera;
            camera.position_wc = self.shadow_map_camera.position_wc;
            camera.position_cartographic = frame
                .uniform
                .map_projection
                .ellipsoid()
                .cartesian_to_cartographic(camera.position_wc)
                .unwrap_or_default();
            camera.direction_wc = directions[index];
            camera.up_wc = ups[index];
            camera.right_wc = rights[index];
            camera.view_matrix =
                Matrix4::compute_view(camera.position_wc, camera.direction_wc, camera.up_wc, camera.right_wc);
            camera.inverse_view_matrix = camera.view_matrix.inverse().unwrap_or(Matrix4::IDENTITY);
            camera.frustum = frustum;
        }
    }

    fn fit_shadow_map_to_scene(&mut self, frame: &FrameState) {
        let Some(scene_camera) = self.scene_camera else { return };
        let view_projection = scene_camera.frustum.projection_matrix().multiply(&scene_camera.view_matrix);
        let Ok(inverse_view_projection) = view_projection.inverse() else { return };
        let light_dir = self.shadow_map_camera.direction_wc;
        let mut light_up = scene_camera.direction_wc;
        if light_dir.equals_epsilon_with(light_up, crate::core::math::EPSILON10, crate::core::math::EPSILON10) {
            light_up = scene_camera.up_wc;
        }
        let mut light_right = light_dir.cross(light_up);
        light_up = light_right.cross(light_dir).normalize();
        light_right = light_right.normalize();
        let light_view = Matrix4::compute_view(Cartesian3::ZERO, light_dir, light_up, light_right);
        let camera_to_light = light_view.multiply(&inverse_view_projection);
        let (mut min, mut max) =
            (Cartesian3::new(f64::MAX, f64::MAX, f64::MAX), Cartesian3::new(-f64::MAX, -f64::MAX, -f64::MAX));
        for corner in frustum_corners_ndc() {
            let mut corner = camera_to_light.multiply_by_vector(corner);
            if corner.w == 0.0 {
                continue;
            }
            corner = Cartesian4::new(corner.x / corner.w, corner.y / corner.w, corner.z / corner.w, corner.w);
            min = Cartesian3::new(min.x.min(corner.x), min.y.min(corner.y), min.z.min(corner.z));
            max = Cartesian3::new(max.x.max(corner.x), max.y.max(corner.y), max.z.max(corner.z));
        }
        max.z += 1000.0;
        min.z -= 10.0;
        let translation = Cartesian3::new(-0.5 * (min.x + max.x), -0.5 * (min.y + max.y), -max.z);
        let light_view = Matrix4::from_translation(translation).multiply(&light_view);
        let half_width = 0.5 * (max.x - min.x);
        let half_height = 0.5 * (max.y - min.y);
        let depth = max.z - min.z;
        self.shadow_map_camera.frustum = ShadowMapFrustum::OrthographicOffCenter(OrthographicOffCenterFrustum::new(
            -half_width,
            half_width,
            half_height,
            -half_height,
            0.01,
            depth.max(0.01),
        ));
        self.shadow_map_camera.view_matrix = light_view;
        self.shadow_map_camera.inverse_view_matrix = light_view.inverse().unwrap_or(Matrix4::IDENTITY);
        self.shadow_map_camera.position_wc = self.shadow_map_camera.inverse_view_matrix.get_translation();
        self.shadow_map_camera.position_cartographic = frame
            .uniform
            .map_projection
            .ellipsoid()
            .cartesian_to_cartographic(self.shadow_map_camera.position_wc)
            .unwrap_or_default();
        self.shadow_map_camera.direction_wc = light_dir;
        self.shadow_map_camera.up_wc = light_up;
        self.shadow_map_camera.right_wc = light_right;
    }

    fn compute_cascades(&mut self, frame: &FrameState) {
        let Some(scene_camera) = self.scene_camera else { return };
        let camera_near = scene_camera.frustum.near();
        let camera_far = scene_camera.frustum.far();
        let number_of_cascades = self.number_of_cascades as usize;
        let range = camera_far - camera_near;
        let ratio = camera_far / camera_near;
        let (lambda, clamp_cascade_distances) =
            if frame.shadow_state.closest_object_size < 200.0 { (0.9, true) } else { (0.9, false) };
        let mut splits = [0.0f64; 5];
        let mut cascade_distances = [0.0f64; 4];
        splits[0] = camera_near;
        splits[number_of_cascades] = camera_far;
        for i in 0..number_of_cascades {
            let p = (i + 1) as f64 / number_of_cascades as f64;
            let log_scale = camera_near * ratio.powf(p);
            let uniform_scale = camera_near + range * p;
            let split = uniform_scale + (log_scale - uniform_scale) * lambda;
            splits[i + 1] = split;
            cascade_distances[i] = split - splits[i];
        }
        if clamp_cascade_distances {
            let maximum = self.maximum_cascade_distances;
            for (distance, limit) in cascade_distances.iter_mut().take(number_of_cascades).zip(maximum.iter()) {
                *distance = distance.min(*limit);
            }
            let mut distance = splits[0];
            for (next_split, cascade_distance) in splits
                .iter_mut()
                .skip(1)
                .take(number_of_cascades - 1)
                .zip(cascade_distances.iter())
            {
                distance += *cascade_distance;
                *next_split = distance;
            }
        }
        self.cascade_splits[0] = Cartesian4::new(splits[0], splits[1], splits[2], splits[3]);
        self.cascade_splits[1] = Cartesian4::new(splits[1], splits[2], splits[3], splits[4]);
        self.cascade_distances =
            Cartesian4::new(cascade_distances[0], cascade_distances[1], cascade_distances[2], cascade_distances[3]);

        let shadow_frustum = self.shadow_map_camera.frustum;
        let (left, right, bottom, top) = match shadow_frustum {
            ShadowMapFrustum::OrthographicOffCenter(f) => (f.left, f.right, f.bottom, f.top),
            ShadowMapFrustum::Perspective(f) => {
                let o = f.off_center_frustum();
                (o.left, o.right, o.bottom, o.top)
            }
            ShadowMapFrustum::PerspectiveOffCenter(f) => (f.left, f.right, f.bottom, f.top),
            ShadowMapFrustum::Orthographic(f) => {
                let o = f.off_center_frustum();
                (o.left, o.right, o.bottom, o.top)
            }
        };
        let (near, far) = (shadow_frustum.near(), shadow_frustum.far());
        let position = self.shadow_map_camera.position_wc;
        let direction = self.shadow_map_camera.direction_wc;
        let up = self.shadow_map_camera.up_wc;
        let mut shadow_camera = self.shadow_map_camera;
        let shadow_view_projection = shadow_camera.get_view_projection();

        for i in 0..number_of_cascades {
            let mut sub_frustum = scene_camera.frustum;
            sub_frustum.set_near_far(splits[i], splits[i + 1]);
            let view_projection = sub_frustum.projection_matrix().multiply(&scene_camera.view_matrix);
            let Ok(inverse_view_projection) = view_projection.inverse() else { continue };
            let shadow_map_matrix = shadow_view_projection.multiply(&inverse_view_projection);
            let (mut min, mut max) =
                (Cartesian3::new(f64::MAX, f64::MAX, f64::MAX), Cartesian3::new(-f64::MAX, -f64::MAX, -f64::MAX));
            for corner in frustum_corners_ndc() {
                let mut corner = shadow_map_matrix.multiply_by_vector(corner);
                if corner.w == 0.0 {
                    continue;
                }
                corner = Cartesian4::new(corner.x / corner.w, corner.y / corner.w, corner.z / corner.w, corner.w);
                min = Cartesian3::new(min.x.min(corner.x), min.y.min(corner.y), min.z.min(corner.z));
                max = Cartesian3::new(max.x.max(corner.x), max.y.max(corner.y), max.z.max(corner.z));
            }
            min.x = min.x.max(0.0);
            min.y = min.y.max(0.0);
            min.z = 0.0;
            max.x = max.x.min(1.0);
            max.y = max.y.min(1.0);
            max.z = max.z.min(1.0);
            let offsets = self.passes[i].texture_offsets;
            let mut cascade_camera = shadow_camera;
            cascade_camera.frustum = ShadowMapFrustum::OrthographicOffCenter(OrthographicOffCenterFrustum::new(
                left + min.x * (right - left),
                left + max.x * (right - left),
                bottom + max.y * (top - bottom),
                bottom + min.y * (top - bottom),
                near + min.z * (far - near),
                near + max.z * (far - near),
            ));
            let culling_volume = cascade_camera.frustum.compute_culling_volume(position, direction, up);
            let mut cascade_matrix = cascade_camera.get_view_projection().multiply(&scene_camera.inverse_view_matrix);
            if let Some(offsets) = offsets {
                cascade_matrix = offsets.multiply(&cascade_matrix);
            }
            self.passes[i].camera = cascade_camera;
            self.passes[i].culling_volume = Some(culling_volume);
            self.cascade_matrices[i] = cascade_matrix;
        }
    }

    /// `ShadowMap.prototype.update`.
    pub fn update(&mut self, frame: &FrameState) -> Result<()> {
        if self.destroyed {
            return Err(RuntimeError::new("This object was destroyed, i.e., destroy() was called."));
        }
        self.update_cameras(frame)?;
        if self.needs_update {
            self.update_framebuffer()?;
            if self.is_point_light {
                self.compute_omnidirectional(frame);
            }
            if self.cascades_enabled {
                self.fit_shadow_map_to_scene(frame);
                if self.number_of_cascades > 1 {
                    self.compute_cascades(frame);
                }
            }
            if !self.is_point_light {
                self.shadow_map_culling_volume = Some(self.shadow_map_camera.frustum.compute_culling_volume(
                    self.shadow_map_camera.position_wc,
                    self.shadow_map_camera.direction_wc,
                    self.shadow_map_camera.up_wc,
                ));
                if self.passes.len() == 1 {
                    self.passes[0].camera.clone_from(&self.shadow_map_camera);
                }
            } else {
                self.shadow_map_culling_volume = Some(CullingVolume::from_bounding_sphere(&self.bounding_sphere));
            }
        }
        if self.passes.len() == 1 {
            let Some(scene_camera) = self.scene_camera else { return Ok(()) };
            self.shadow_map_matrix =
                self.shadow_map_camera.get_view_projection().multiply(&scene_camera.inverse_view_matrix);
        }
        if self.debug_show {
            self.debug_freeze_frame_current = self.debug_freeze_frame;
        }
        Ok(())
    }

    fn receive_options(&self, terrain: bool, terrain_normal: bool, cast_shadows: bool) -> ReceiveOptions {
        ReceiveOptions {
            uses_depth_texture: self.uses_depth_texture,
            polygon_offset_supported: self.polygon_offset_supported,
            point_light: self.is_point_light,
            spot_light: self.is_spot_light,
            number_of_cascades: self.number_of_cascades,
            debug_cascade_colors: self.debug_cascade_colors,
            soft_shadows: self.soft_shadows,
            cast_shadows,
            terrain,
            terrain_normal,
            point_bias: self.point_bias.receive(),
            terrain_bias: self.terrain_bias.receive(),
            primitive_bias: self.primitive_bias.receive(),
        }
    }

    /// Uniform values added to cast or receive shaders.
    pub fn uniform_values(&self, is_terrain: bool) -> Vec<(String, UniformValue)> {
        let bias = if self.is_point_light {
            self.point_bias
        } else if is_terrain {
            self.terrain_bias
        } else {
            self.primitive_bias
        };
        vec![
            (
                "shadowMap_matrix".into(),
                UniformValue::Matrix { dimension: 4, values: self.shadow_map_matrix.0.map(|v| v as f32).to_vec() },
            ),
            (
                "shadowMap_cascadeSplits".into(),
                UniformValue::Float(
                    self.cascade_splits.iter().flat_map(|s| [s.x as f32, s.y as f32, s.z as f32, s.w as f32]).collect(),
                ),
            ),
            (
                "shadowMap_cascadeMatrices".into(),
                UniformValue::Float(self.cascade_matrices.iter().flat_map(|m| m.0.map(|v| v as f32)).collect()),
            ),
            (
                "shadowMap_lightDirectionEC".into(),
                UniformValue::Float(vec![
                    self.light_direction_ec.x as f32,
                    self.light_direction_ec.y as f32,
                    self.light_direction_ec.z as f32,
                ]),
            ),
            (
                "shadowMap_lightPositionEC".into(),
                UniformValue::Float(vec![
                    self.light_position_ec.x as f32,
                    self.light_position_ec.y as f32,
                    self.light_position_ec.z as f32,
                    self.light_position_ec.w as f32,
                ]),
            ),
            (
                "shadowMap_cascadeDistances".into(),
                UniformValue::Float(vec![
                    self.cascade_distances.x as f32,
                    self.cascade_distances.y as f32,
                    self.cascade_distances.z as f32,
                    self.cascade_distances.w as f32,
                ]),
            ),
            (
                "shadowMap_texelSizeDepthBiasAndNormalShadingSmooth".into(),
                UniformValue::Float(vec![
                    1.0 / self.texture_size.x as f32,
                    1.0 / self.texture_size.y as f32,
                    bias.depth_bias as f32,
                    bias.normal_shading_smooth as f32,
                ]),
            ),
            (
                "shadowMap_normalOffsetScaleDistanceMaxDistanceAndDarkness".into(),
                UniformValue::Float(vec![
                    bias.normal_offset_scale as f32,
                    self.distance as f32,
                    self.maximum_distance as f32,
                    self.darkness_current as f32,
                ]),
            ),
        ]
    }

    /// Derives a cast draw from an existing program/render state/uniforms.
    #[allow(clippy::too_many_arguments)]
    pub fn create_cast_draw(
        &self,
        context: &Context<G>,
        options: &SceneDrawOptions<G>,
        is_terrain: bool,
        pass: Pass,
    ) -> Result<SceneDrawOptions<G>> {
        let source = options
            .program
            .source_options()
            .ok_or_else(|| RuntimeError::new("Shadow cast derivation requires ShaderProgram::from_sources"))?;
        let is_point_light = self.is_point_light;
        let is_opaque = pass != Pass::Translucent;
        let vertex = shadow_map_shader::cast_vertex(&source.vertex_shader_source, is_point_light, is_terrain);
        let fragment = shadow_map_shader::cast_fragment(
            &source.fragment_shader_source,
            is_point_light,
            self.uses_depth_texture,
            is_opaque,
        );
        let program = Rc::new(ShaderProgram::from_sources(
            context,
            &ShaderProgramOptions {
                vertex_shader_source: vertex,
                fragment_shader_source: fragment,
                attribute_locations: source.attribute_locations.clone(),
            },
        )?);
        let mut state = if is_point_light {
            self.point_render_state.clone()
        } else if is_terrain {
            self.terrain_render_state.clone()
        } else {
            self.primitive_render_state.clone()
        };
        if !options.state.cull.enabled {
            state.cull.enabled = false;
        }
        let derived = SceneDrawOptions {
            vertices: options.vertices.clone(),
            program,
            state,
            primitive_type: options.primitive_type,
            offset: options.offset,
            count: options.count,
            instance_count: options.instance_count,
            uniforms: combine_uniforms(self.uniform_values(is_terrain), options.uniforms.clone()),
            uniform_map: options.uniform_map.clone(),
            textures: with_shadow_texture(self.shadow_map_texture.clone(), options.textures.clone()),
            framebuffer: options.framebuffer.clone(),
            pick_metadata: options.pick_metadata.clone(),
        };
        Ok(derived)
    }

    /// Builds an executable cast command.
    pub fn create_cast_command(
        &self,
        id: impl Into<String>,
        context: Context<G>,
        pass: Pass,
        metadata: DrawMetadata,
        options: &SceneDrawOptions<G>,
        is_terrain: bool,
    ) -> Result<SceneCommand> {
        let derived = self.create_cast_draw(&context, options, is_terrain, pass)?;
        Ok(SceneCommand::from_draw(id, context, pass, metadata, derived))
    }

    /// Derives a receive draw for a command whose source is known.
    #[allow(clippy::too_many_arguments)]
    pub fn create_receive_draw(
        &self,
        context: &Context<G>,
        options: &SceneDrawOptions<G>,
        is_terrain: bool,
        has_terrain_normal: bool,
        cast_shadows: bool,
    ) -> Result<SceneDrawOptions<G>> {
        let source = options
            .program
            .source_options()
            .ok_or_else(|| RuntimeError::new("Shadow receive derivation requires ShaderProgram::from_sources"))?;
        let vertex = shadow_map_shader::receive_vertex(&source.vertex_shader_source, is_terrain, has_terrain_normal);
        let receive_options = self.receive_options(is_terrain, has_terrain_normal, cast_shadows);
        let fragment = shadow_map_shader::receive_fragment(&source.fragment_shader_source, &receive_options);
        let program = Rc::new(ShaderProgram::from_sources(
            context,
            &ShaderProgramOptions {
                vertex_shader_source: vertex,
                fragment_shader_source: fragment,
                attribute_locations: source.attribute_locations.clone(),
            },
        )?);
        Ok(SceneDrawOptions {
            vertices: options.vertices.clone(),
            program,
            state: options.state.clone(),
            primitive_type: options.primitive_type,
            offset: options.offset,
            count: options.count,
            instance_count: options.instance_count,
            uniforms: combine_uniforms(self.uniform_values(is_terrain), options.uniforms.clone()),
            uniform_map: options.uniform_map.clone(),
            textures: with_shadow_texture(self.shadow_map_texture.clone(), options.textures.clone()),
            framebuffer: options.framebuffer.clone(),
            pick_metadata: options.pick_metadata.clone(),
        })
    }

    /// `createReceiveDerivedCommand` for a scene command.
    #[allow(clippy::too_many_arguments)]
    pub fn create_receive_command(
        &self,
        id: impl Into<String>,
        context: Context<G>,
        metadata: DrawMetadata,
        options: &SceneDrawOptions<G>,
        is_terrain: bool,
        has_terrain_normal: bool,
        cast_shadows: bool,
    ) -> Result<Option<SceneCommand>> {
        if !metadata.receive_shadows {
            return Ok(None);
        }
        let derived = self.create_receive_draw(&context, options, is_terrain, has_terrain_normal, cast_shadows)?;
        Ok(Some(SceneCommand::from_draw(id, context, Pass::Opaque, metadata, derived)))
    }

    pub fn is_destroyed(&self) -> bool {
        self.destroyed
    }

    pub fn destroy(&mut self) -> Result<()> {
        self.destroy_framebuffer()?;
        self.destroyed = true;
        Ok(())
    }
}

impl<G: Gl + 'static> SceneEffect for ShadowMap<G> {
    fn update(&mut self, frame: &mut FrameState) -> Result<()> {
        ShadowMap::update(self, frame)
    }
    fn render(&mut self, _frame: &mut FrameState) -> Result<()> {
        Ok(())
    }
    fn destroy(&mut self) -> Result<()> {
        ShadowMap::destroy(self)
    }
}

fn polygon_offset_supported(capabilities: &ContextCapabilities) -> bool {
    // Cesium disables polygon offset on Windows browsers without depth
    // textures. The portable port has no user-agent, so follow the extension
    // rule directly.
    capabilities.depth_texture
}

fn create_render_state(color_mask: bool, bias: &ShadowBias) -> RenderState {
    RenderState {
        cull: Cull { enabled: true, face: CullFace::Back.value() },
        depth_test: DepthTest { enabled: true, ..Default::default() },
        color_mask: ColorMask { red: color_mask, green: color_mask, blue: color_mask, alpha: color_mask },
        depth_mask: true,
        polygon_offset: PolygonOffset {
            enabled: bias.polygon_offset,
            factor: bias.polygon_offset_factor,
            units: bias.polygon_offset_units,
        },
        ..Default::default()
    }
}

fn frustum_corners_ndc() -> [Cartesian4; 8] {
    [
        Cartesian4::new(-1.0, -1.0, -1.0, 1.0),
        Cartesian4::new(1.0, -1.0, -1.0, 1.0),
        Cartesian4::new(1.0, 1.0, -1.0, 1.0),
        Cartesian4::new(-1.0, 1.0, -1.0, 1.0),
        Cartesian4::new(-1.0, -1.0, 1.0, 1.0),
        Cartesian4::new(1.0, -1.0, 1.0, 1.0),
        Cartesian4::new(1.0, 1.0, 1.0, 1.0),
        Cartesian4::new(-1.0, 1.0, 1.0, 1.0),
    ]
}

fn combine_uniforms(
    shadow: Vec<(String, UniformValue)>,
    source: Vec<(String, UniformValue)>,
) -> Vec<(String, UniformValue)> {
    let mut result = source;
    for (name, value) in shadow {
        if let Some(slot) = result.iter_mut().find(|(existing, _)| *existing == name) {
            slot.1 = value;
        } else {
            result.push((name, value));
        }
    }
    result
}

fn with_shadow_texture<G: Gl + 'static>(
    shadow: Option<TextureBinding<G>>,
    mut textures: Vec<(String, Rc<dyn crate::renderer::SampledTexture<G>>)>,
) -> Vec<(String, Rc<dyn crate::renderer::SampledTexture<G>>)> {
    if let Some(binding) = shadow {
        textures.push((
            "shadowMap_texture".into(),
            match binding {
                TextureBinding::Texture(t) => t as Rc<dyn crate::renderer::SampledTexture<G>>,
                TextureBinding::CubeMap(t) => t as Rc<dyn crate::renderer::SampledTexture<G>>,
                TextureBinding::Volume(t) => t as Rc<dyn crate::renderer::SampledTexture<G>>,
            },
        ));
    }
    textures
}

impl<G: Gl + 'static> Drop for ShadowMap<G> {
    fn drop(&mut self) {
        if !self.destroyed {
            let _ = self.destroy();
        }
    }
}
