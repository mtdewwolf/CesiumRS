//! CesiumJS `AtmospherePipelineStage`: applies earth atmosphere effects and fog
//! to models. Fog is only sampled when the camera is close enough for the
//! effect to be non-negligible, so that condition is a uniform.
use super::ModelRenderResources;
use crate::{
    core::math,
    renderer::{Gl, ShaderDestination, UniformValue},
    scene::FrameState,
    shaders,
};
use std::rc::Rc;

/// Add the atmosphere shader and the live fog visibility uniform.
pub fn process<G: Gl>(resources: &mut ModelRenderResources<G>, frame_state: &FrameState) {
    resources.shader_builder.add_define("HAS_ATMOSPHERE", None, ShaderDestination::BOTH);
    resources.shader_builder.add_define("COMPUTE_POSITION_WC_ATMOSPHERE", None, ShaderDestination::BOTH);
    resources.shader_builder.add_varying("vec3", "v_atmosphereRayleighColor", None);
    resources.shader_builder.add_varying("vec3", "v_atmosphereMieColor", None);
    resources.shader_builder.add_varying("float", "v_atmosphereOpacity", None);
    resources.shader_builder.add_vertex_lines(&[shaders::model::ATMOSPHERE_STAGE_VS.source]);
    resources.shader_builder.add_fragment_lines(&[shaders::model::ATMOSPHERE_STAGE_FS.source]);
    resources.shader_builder.add_uniform("bool", "u_isInFog", ShaderDestination::FRAGMENT);

    // We only need a rough measure of distance to the model, so measure from
    // the camera to the bounding sphere center. The frame state is not
    // retained by the uniform provider, matching the per-frame snapshot
    // Cesium takes through its closure.
    let distance = frame_state.uniform.camera.position.distance(resources.model.borrow().bounding_sphere.center);
    let in_fog = math::fog(distance, frame_state.fog.density) > math::EPSILON3;
    resources.uniform_map.insert("u_isInFog".into(), Rc::new(move || UniformValue::Int(vec![i32::from(in_fog)])));
}
