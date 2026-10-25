//! CesiumJS AlphaPipelineStage.
use super::PipelineResources;
use crate::{
    renderer::{Pass, ShaderDestination, UniformValue},
    scene::blending_state,
};
use std::rc::Rc;
pub fn process(resources: &mut PipelineResources) {
    let pass = *resources.alpha_options.borrow_mut().pass.get_or_insert(resources.model.borrow().opaque_pass);
    if pass == Pass::Translucent {
        resources.render_state_options.cull.enabled = false;
        resources.render_state_options.depth_mask = false;
        resources.render_state_options.blending = blending_state::alpha_blend();
    }
    if resources.alpha_options.borrow().alpha_cutoff.is_some() {
        resources.shader_builder.add_define("ALPHA_MODE_MASK", None, ShaderDestination::FRAGMENT);
        resources.shader_builder.add_uniform("float", "u_alphaCutoff", ShaderDestination::FRAGMENT);
        let options = resources.alpha_options.clone();
        resources.uniform_map.insert(
            "u_alphaCutoff".into(),
            Rc::new(move || UniformValue::Float(vec![options.borrow().alpha_cutoff.unwrap_or(f64::NAN) as f32])),
        );
    }
}
