//! CesiumJS `Scene/Model/CPUStylingPipelineStage`: adds the CPU styling shaders,
//! defines, and `model_colorBlend` / `model_commandTranslucent` uniforms.

use super::PipelineResources;
use crate::renderer::{Pass, ShaderDestination, UniformValue};
use crate::shaders;
use std::rc::Rc;

pub fn process(resources: &mut PipelineResources) {
    let shader = &mut resources.shader_builder;

    shader.add_vertex_lines(&[shaders::model::CPU_STYLING_STAGE_VS.source]);
    shader.add_fragment_lines(&[shaders::model::CPU_STYLING_STAGE_FS.source]);
    shader.add_define("USE_CPU_STYLING", None, ShaderDestination::BOTH);

    let has_color = resources.model.borrow().has_color;
    // Needed when the model isn't statically colored (if it is, the uniform will
    // already have been added in the ModelColorStage).
    if !has_color {
        shader.add_uniform("float", "model_colorBlend", ShaderDestination::FRAGMENT);
        let model = resources.model.clone();
        resources.uniform_map.insert(
            "model_colorBlend".into(),
            Rc::new(move || {
                let m = model.borrow();
                UniformValue::Float(vec![m.color_blend_mode.get_color_blend(m.color_blend_amount) as f32])
            }),
        );
    }

    // Check if the command will berendered in the translucent pass. This uniform value
    // changes between the two commands generated when styleCommandsNeeded is
    // ALL_COMMANDS.
    shader.add_uniform("bool", "model_commandTranslucent", ShaderDestination::BOTH);
    let alpha_options = resources.alpha_options.clone();
    resources.uniform_map.insert(
        "model_commandTranslucent".into(),
        Rc::new(move || {
            let translucent = alpha_options.borrow().pass == Some(Pass::Translucent);
            UniformValue::Int(vec![i32::from(translucent)])
        }),
    );
}
