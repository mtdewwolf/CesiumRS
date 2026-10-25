//! CesiumJS `EdgeDetectionPipelineStage`: appends the fragment shader that
//! decides which edge-pass samples are visible in the final frame.
use super::PrimitiveRenderResources;
use crate::{renderer::Gl, shaders};

pub fn process<G: Gl>(resources: &mut PrimitiveRenderResources<G>) {
    resources.shader_builder.add_fragment_lines(&[shaders::model::EDGE_DETECTION_STAGE_FS.source]);
}
