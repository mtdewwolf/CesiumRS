//! CesiumJS ModelSplitterPipelineStage.
use super::PipelineResources;
use crate::{
    renderer::{ShaderDestination, UniformValue},
    shaders,
};
use std::rc::Rc;
pub const SPLIT_DIRECTION_UNIFORM_NAME: &str = "model_splitDirection";
pub fn process(resources: &mut PipelineResources) {
    resources.shader_builder.add_define("HAS_MODEL_SPLITTER", None, ShaderDestination::FRAGMENT);
    resources.shader_builder.add_fragment_lines(&[shaders::model::MODEL_SPLITTER_STAGE_FS.source]);
    resources.shader_builder.add_uniform("float", SPLIT_DIRECTION_UNIFORM_NAME, ShaderDestination::FRAGMENT);
    let model = resources.model.clone();
    resources.uniform_map.insert(
        SPLIT_DIRECTION_UNIFORM_NAME.into(),
        Rc::new(move || UniformValue::Float(vec![model.borrow().split_direction as f32])),
    );
}
