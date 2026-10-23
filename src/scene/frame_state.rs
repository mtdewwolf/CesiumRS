//! Owned frame data exchanged by Scene, primitives and tileset passes.
use super::{
    scene::{SceneCommand, SceneSubmission},
    Atmosphere, FrameFog, SceneMode,
};
use crate::{
    core::{Cartesian3, Color, Credit, CullingVolume, JulianDate, Occluder},
    renderer::{UniformFrame, UniformLight},
};
use std::{any::Any, rc::Rc};

/// A shared subsystem or GPU resource. FrameState does not perform operations
/// on these objects; their concrete owners retain the corresponding type.
#[derive(Clone)]
pub struct FrameResource(Rc<dyn Any>);
impl FrameResource {
    pub fn new<T: Any>(resource: T) -> Self {
        Self(Rc::new(resource))
    }
    pub fn get<T: Any>(&self) -> Option<&T> {
        self.0.downcast_ref()
    }
    pub fn ptr_eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
}
impl std::fmt::Debug for FrameResource {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("FrameResource").field(&self.0.type_id()).finish()
    }
}
impl PartialEq for FrameResource {
    fn eq(&self, other: &Self) -> bool {
        self.ptr_eq(other)
    }
}

#[derive(Clone, Debug)]
pub struct FrameShadowState {
    pub shadows_enabled: bool,
    pub light_shadows_enabled: Option<bool>,
    pub shadow_maps: Vec<FrameResource>,
    pub light_shadow_maps: Vec<FrameResource>,
    pub near_plane: f64,
    pub far_plane: f64,
    pub closest_object_size: f64,
    pub last_dirty_time: f64,
    pub out_of_view: bool,
}
impl Default for FrameShadowState {
    fn default() -> Self {
        Self {
            shadows_enabled: true,
            light_shadows_enabled: None,
            shadow_maps: Vec::new(),
            light_shadow_maps: Vec::new(),
            near_plane: 1.,
            far_plane: 5000.,
            closest_object_size: 1000.,
            last_dirty_time: 0.,
            out_of_view: true,
        }
    }
}
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct FramePasses {
    pub render: bool,
    pub pick: bool,
    pub pick_voxel: bool,
    pub depth: bool,
    pub post_process: bool,
    pub offscreen: bool,
}
pub type AfterRender = Box<dyn FnOnce() -> Result<bool, crate::core::RuntimeError>>;
pub struct FrameState {
    pub context: Option<FrameResource>,
    pub context_capabilities: crate::renderer::ContextCapabilities,
    pub credit_display: Option<FrameResource>,
    pub job_scheduler: Option<FrameResource>,
    pub render_request: Option<super::SceneRenderRequest>,
    pub camera_position_local: Cartesian3,
    pub camera_position_delta: f64,
    pub camera_position_delta_last_frame: f64,
    pub camera_time_since_moved: f64,
    pub drawing_buffer_width: u32,
    pub drawing_buffer_height: u32,
    pub frame_number: u32,
    pub time: JulianDate,
    pub new_frame: bool,
    pub mode: SceneMode,
    pub morph_time: f64,
    pub uniform: UniformFrame,
    pub culling_volume: CullingVolume,
    pub passes: FramePasses,
    pub command_list: Vec<SceneCommand>,
    pub panorama_command_list: Vec<SceneCommand>,
    pub shadow_maps: Vec<FrameResource>,
    pub brdf_lut_generator: Option<FrameResource>,
    pub environment_map: Option<FrameResource>,
    pub spherical_harmonic_coefficients: Option<Vec<Cartesian3>>,
    pub specular_environment_maps: Option<FrameResource>,
    pub specular_environment_maps_maximum_lod: Option<f64>,
    pub camera_underground: bool,
    pub globe_translucency_state: Option<FrameResource>,
    pub occluder: Option<Occluder>,
    pub pixel_ratio: f64,
    pub scene_3d_only: bool,
    pub fog: FrameFog,
    pub atmosphere: Option<Atmosphere>,
    pub shadow_state: FrameShadowState,
    pub split_position: f64,
    pub frustum_splits: Vec<f64>,
    pub background_color: Option<Color>,
    pub light: Option<UniformLight>,
    pub minimum_disable_depth_test_distance: Option<f64>,
    pub invert_classification: bool,
    pub invert_classification_color: Option<Color>,
    pub tileset_pass_state: Option<super::TilesetPassState>,
    pub picking_metadata: bool,
    pub picked_metadata_info: Option<serde_json::Value>,
    pub edge_visibility_requested: bool,
    /// Credits submitted this frame, in encounter order. The display owner
    /// consumes these after rendering; duplicate identities count as repeats.
    pub credits: Vec<Credit>,
    pub after_render: Vec<AfterRender>,
    pub executed_commands: Vec<SceneSubmission>,
    pub minimum_terrain_height: f64,
    pub maximum_screen_space_error: f64,
    pub use_log_depth: bool,
    pub terrain_exaggeration_changed: bool,
    pub vertical_exaggeration: f64,
    pub vertical_exaggeration_relative_height: f64,
}
impl FrameState {
    pub fn new(uniform: UniformFrame, culling_volume: CullingVolume, dimensions: [u32; 2]) -> Self {
        Self {
            context: None,
            context_capabilities: crate::renderer::ContextCapabilities::default(),
            credit_display: None,
            job_scheduler: None,
            render_request: None,
            camera_position_local: uniform.camera.position,
            camera_position_delta: 0.,
            camera_position_delta_last_frame: 0.,
            camera_time_since_moved: 0.,
            drawing_buffer_width: dimensions[0],
            drawing_buffer_height: dimensions[1],
            frame_number: 0,
            time: uniform.time,
            new_frame: false,
            mode: SceneMode::Scene3D,
            morph_time: 1.,
            uniform,
            culling_volume,
            passes: FramePasses::default(),
            command_list: Vec::new(),
            panorama_command_list: Vec::new(),
            shadow_maps: Vec::new(),
            brdf_lut_generator: None,
            environment_map: None,
            spherical_harmonic_coefficients: None,
            specular_environment_maps: None,
            specular_environment_maps_maximum_lod: None,
            camera_underground: false,
            globe_translucency_state: None,
            occluder: None,
            pixel_ratio: 1.,
            scene_3d_only: false,
            fog: FrameFog {
                density: f64::NAN,
                visual_density_scalar: f64::NAN,
                sse: f64::NAN,
                minimum_brightness: f64::NAN,
                ..FrameFog::default()
            },
            atmosphere: None,
            shadow_state: FrameShadowState::default(),
            split_position: 0.,
            frustum_splits: Vec::new(),
            background_color: None,
            light: None,
            minimum_disable_depth_test_distance: None,
            invert_classification: false,
            invert_classification_color: None,
            tileset_pass_state: None,
            picking_metadata: false,
            picked_metadata_info: None,
            edge_visibility_requested: false,
            credits: Vec::new(),
            after_render: Vec::new(),
            executed_commands: Vec::new(),
            minimum_terrain_height: 0.,
            maximum_screen_space_error: f64::NAN,
            use_log_depth: false,
            terrain_exaggeration_changed: false,
            vertical_exaggeration: 1.,
            vertical_exaggeration_relative_height: 0.,
        }
    }
}
