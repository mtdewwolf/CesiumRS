//! CesiumJS ModelColorPipelineStage.
use super::PipelineResources;
use crate::{
    renderer::{render_state::ColorMask, Pass, ShaderDestination, UniformValue},
    shaders,
};
use std::rc::Rc;
pub fn process(resources: &mut PipelineResources, has_silhouette: bool) {
    resources.shader_builder.add_define("HAS_MODEL_COLOR", None, ShaderDestination::FRAGMENT);
    resources.shader_builder.add_fragment_lines(&[shaders::model::MODEL_COLOR_STAGE_FS.source]);
    let color = resources.model.borrow().color;
    if color.alpha == 0. && !has_silhouette {
        resources.render_state_options.color_mask = ColorMask { red: false, green: false, blue: false, alpha: false };
    }
    if color.alpha < 1. {
        resources.alpha_options.borrow_mut().pass = Some(Pass::Translucent);
    }
    resources.shader_builder.add_uniform("vec4", "model_color", ShaderDestination::FRAGMENT);
    resources.shader_builder.add_uniform("float", "model_colorBlend", ShaderDestination::FRAGMENT);
    let model = resources.model.clone();
    resources.uniform_map.insert(
        "model_color".into(),
        Rc::new(move || {
            let c = model.borrow().color;
            UniformValue::Float(vec![c.red as f32, c.green as f32, c.blue as f32, c.alpha as f32])
        }),
    );
    let model = resources.model.clone();
    resources.uniform_map.insert(
        "model_colorBlend".into(),
        Rc::new(move || {
            let m = model.borrow();
            UniformValue::Float(vec![m.color_blend_mode.get_color_blend(m.color_blend_amount) as f32])
        }),
    );
}
