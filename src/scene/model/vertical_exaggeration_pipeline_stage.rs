//! CesiumJS `VerticalExaggerationPipelineStage`: transforms vertex positions
//! with the scene's vertical exaggeration and relative height.
use super::PrimitiveRenderResources;
use crate::{
    renderer::{Gl, ShaderDestination, UniformValue},
    scene::FrameState,
    shaders,
};
use std::rc::Rc;

/// Add the exaggeration shader and its live uniform.
pub fn process<G: Gl>(
    resources: &mut PrimitiveRenderResources<G>,
    _primitive: &crate::scene::model_components::Primitive<G>,
    frame_state: &FrameState,
) {
    resources.shader_builder.add_vertex_lines(&[shaders::model::VERTICAL_EXAGGERATION_STAGE_VS.source]);
    resources.shader_builder.add_define("HAS_VERTICAL_EXAGGERATION", None, ShaderDestination::VERTEX);
    resources.shader_builder.add_uniform("vec2", "u_verticalExaggerationAndRelativeHeight", ShaderDestination::VERTEX);
    let exaggeration = frame_state.vertical_exaggeration;
    let relative_height = frame_state.vertical_exaggeration_relative_height;
    resources.uniform_map.insert(
        "u_verticalExaggerationAndRelativeHeight".into(),
        Rc::new(move || UniformValue::Float(vec![exaggeration as f32, relative_height as f32])),
    );
}
