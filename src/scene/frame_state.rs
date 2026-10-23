//! Owned frame data exchanged by Scene, primitives and tileset passes.
use super::{
    scene::{SceneCommand, SceneSubmission},
    SceneMode,
};
use crate::{
    core::{CullingVolume, JulianDate},
    renderer::UniformFrame,
};
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
    pub render_request: Option<super::SceneRenderRequest>,
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
            render_request: None,
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
            after_render: Vec::new(),
            executed_commands: Vec::new(),
            minimum_terrain_height: 0.,
            maximum_screen_space_error: 2.,
            use_log_depth: false,
            terrain_exaggeration_changed: false,
            vertical_exaggeration: 1.,
            vertical_exaggeration_relative_height: 0.,
        }
    }
}
