//! `Source/Shaders/Builtin/Structs`.
//!
//! Each struct is one `shaders.push` line in [`register`]. Its name is the
//! `czm_` key `CzmBuiltins.js` gives it, which is also the GLSL identifier.
//! The source sits next to this file in `structs/<stem>.glsl`.

use super::super::ShaderModule;

/// `czm_depthRangeStruct`, the exact text of `Structs/depthRangeStruct.js`.
pub static DEPTH_RANGE_STRUCT: ShaderModule = ShaderModule {
    id: "Shaders/Builtin/Structs/depthRangeStruct",
    name: "czm_depthRangeStruct",
    source: include_str!("structs/depthRangeStruct.glsl"),
};

/// `czm_material`, the exact text of `Structs/material.js`.
pub static MATERIAL: ShaderModule = ShaderModule {
    id: "Shaders/Builtin/Structs/material",
    name: "czm_material",
    source: include_str!("structs/material.glsl"),
};

/// `czm_materialInput`, the exact text of `Structs/materialInput.js`.
pub static MATERIAL_INPUT: ShaderModule = ShaderModule {
    id: "Shaders/Builtin/Structs/materialInput",
    name: "czm_materialInput",
    source: include_str!("structs/materialInput.glsl"),
};

/// `czm_modelMaterial`, the exact text of `Structs/modelMaterial.js`.
pub static MODEL_MATERIAL: ShaderModule = ShaderModule {
    id: "Shaders/Builtin/Structs/modelMaterial",
    name: "czm_modelMaterial",
    source: include_str!("structs/modelMaterial.glsl"),
};

/// `czm_modelVertexOutput`, the exact text of `Structs/modelVertexOutput.js`.
pub static MODEL_VERTEX_OUTPUT: ShaderModule = ShaderModule {
    id: "Shaders/Builtin/Structs/modelVertexOutput",
    name: "czm_modelVertexOutput",
    source: include_str!("structs/modelVertexOutput.glsl"),
};

/// `czm_ray`, the exact text of `Structs/ray.js`.
pub static RAY: ShaderModule =
    ShaderModule { id: "Shaders/Builtin/Structs/ray", name: "czm_ray", source: include_str!("structs/ray.glsl") };

/// `czm_raySegment`, the exact text of `Structs/raySegment.js`.
pub static RAY_SEGMENT: ShaderModule = ShaderModule {
    id: "Shaders/Builtin/Structs/raySegment",
    name: "czm_raySegment",
    source: include_str!("structs/raySegment.glsl"),
};

/// `czm_shadowParameters`, the exact text of `Structs/shadowParameters.js`.
pub static SHADOW_PARAMETERS: ShaderModule = ShaderModule {
    id: "Shaders/Builtin/Structs/shadowParameters",
    name: "czm_shadowParameters",
    source: include_str!("structs/shadowParameters.glsl"),
};

pub fn register(shaders: &mut Vec<&'static ShaderModule>) {
    shaders.push(&DEPTH_RANGE_STRUCT);
    shaders.push(&MATERIAL);
    shaders.push(&MATERIAL_INPUT);
    shaders.push(&MODEL_MATERIAL);
    shaders.push(&MODEL_VERTEX_OUTPUT);
    shaders.push(&RAY);
    shaders.push(&RAY_SEGMENT);
    shaders.push(&SHADOW_PARAMETERS);
}
