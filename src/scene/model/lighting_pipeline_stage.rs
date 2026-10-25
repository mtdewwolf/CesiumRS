//! CesiumJS `LightingPipelineStage`: selects unlit or physically based
//! shading and adds the lighting shader and custom light color uniform.
use super::{LightingModel, PrimitiveRenderResources};
use crate::{
    renderer::{Gl, ShaderDestination, UniformValue},
    scene::model_components as mc,
    shaders,
};
use std::rc::Rc;

/// Process a primitive with its material's lighting model.
pub fn process<G: Gl>(resources: &mut PrimitiveRenderResources<G>, _primitive: &mc::Primitive<G>) {
    let light_color = resources.model.borrow().light_color;
    if let Some(color) = light_color {
        resources.shader_builder.add_define("USE_CUSTOM_LIGHT_COLOR", None, ShaderDestination::FRAGMENT);
        resources.shader_builder.add_uniform("vec3", "model_lightColorHdr", ShaderDestination::FRAGMENT);
        resources.uniform_map.insert(
            "model_lightColorHdr".into(),
            Rc::new(move || UniformValue::Float(vec![color.x as f32, color.y as f32, color.z as f32])),
        );
    }

    if resources.lighting_options.lighting_model == LightingModel::Pbr {
        resources.shader_builder.add_define("LIGHTING_PBR", None, ShaderDestination::FRAGMENT);
    } else {
        resources.shader_builder.add_define("LIGHTING_UNLIT", None, ShaderDestination::FRAGMENT);
    }
    resources.shader_builder.add_fragment_lines(&[shaders::model::LIGHTING_STAGE_FS.source]);
}
