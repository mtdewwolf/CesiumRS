//! `Source/Shaders/Model`.
//!
//! Each shader is one `shaders.push` line in [`register`]. Its name is the
//! file stem, which is how Scene imports it
//! (`import ModelFS from "../../Shaders/Model/ModelFS.js"`).
//! Upstream gives these modules no `@alias`. The source sits next to this
//! file as `glsl/<stem>.glsl`.

use super::ShaderModule;

/// `AtmosphereStageFS`, the exact text of `Model/AtmosphereStageFS.js`.
pub static ATMOSPHERE_STAGE_FS: ShaderModule = ShaderModule {
    id: "Shaders/Model/AtmosphereStageFS",
    name: "AtmosphereStageFS",
    source: include_str!("glsl/AtmosphereStageFS.glsl"),
};

/// `AtmosphereStageVS`, the exact text of `Model/AtmosphereStageVS.js`.
pub static ATMOSPHERE_STAGE_VS: ShaderModule = ShaderModule {
    id: "Shaders/Model/AtmosphereStageVS",
    name: "AtmosphereStageVS",
    source: include_str!("glsl/AtmosphereStageVS.glsl"),
};

/// `CPUStylingStageFS`, the exact text of `Model/CPUStylingStageFS.js`.
pub static CPU_STYLING_STAGE_FS: ShaderModule = ShaderModule {
    id: "Shaders/Model/CPUStylingStageFS",
    name: "CPUStylingStageFS",
    source: include_str!("glsl/CPUStylingStageFS.glsl"),
};

/// `CPUStylingStageVS`, the exact text of `Model/CPUStylingStageVS.js`.
pub static CPU_STYLING_STAGE_VS: ShaderModule = ShaderModule {
    id: "Shaders/Model/CPUStylingStageVS",
    name: "CPUStylingStageVS",
    source: include_str!("glsl/CPUStylingStageVS.glsl"),
};

/// `CustomShaderStageFS`, the exact text of `Model/CustomShaderStageFS.js`.
pub static CUSTOM_SHADER_STAGE_FS: ShaderModule = ShaderModule {
    id: "Shaders/Model/CustomShaderStageFS",
    name: "CustomShaderStageFS",
    source: include_str!("glsl/CustomShaderStageFS.glsl"),
};

/// `CustomShaderStageVS`, the exact text of `Model/CustomShaderStageVS.js`.
pub static CUSTOM_SHADER_STAGE_VS: ShaderModule = ShaderModule {
    id: "Shaders/Model/CustomShaderStageVS",
    name: "CustomShaderStageVS",
    source: include_str!("glsl/CustomShaderStageVS.glsl"),
};

/// `EdgeDetectionStageFS`, the exact text of `Model/EdgeDetectionStageFS.js`.
pub static EDGE_DETECTION_STAGE_FS: ShaderModule = ShaderModule {
    id: "Shaders/Model/EdgeDetectionStageFS",
    name: "EdgeDetectionStageFS",
    source: include_str!("glsl/EdgeDetectionStageFS.glsl"),
};

/// `EdgeVisibilityStageFS`, the exact text of `Model/EdgeVisibilityStageFS.js`.
pub static EDGE_VISIBILITY_STAGE_FS: ShaderModule = ShaderModule {
    id: "Shaders/Model/EdgeVisibilityStageFS",
    name: "EdgeVisibilityStageFS",
    source: include_str!("glsl/EdgeVisibilityStageFS.glsl"),
};

/// `FeatureIdStageFS`, the exact text of `Model/FeatureIdStageFS.js`.
pub static FEATURE_ID_STAGE_FS: ShaderModule = ShaderModule {
    id: "Shaders/Model/FeatureIdStageFS",
    name: "FeatureIdStageFS",
    source: include_str!("glsl/FeatureIdStageFS.glsl"),
};

/// `FeatureIdStageVS`, the exact text of `Model/FeatureIdStageVS.js`.
pub static FEATURE_ID_STAGE_VS: ShaderModule = ShaderModule {
    id: "Shaders/Model/FeatureIdStageVS",
    name: "FeatureIdStageVS",
    source: include_str!("glsl/FeatureIdStageVS.glsl"),
};

/// `GeometryStageFS`, the exact text of `Model/GeometryStageFS.js`.
pub static GEOMETRY_STAGE_FS: ShaderModule = ShaderModule {
    id: "Shaders/Model/GeometryStageFS",
    name: "GeometryStageFS",
    source: include_str!("glsl/GeometryStageFS.glsl"),
};

/// `GeometryStageVS`, the exact text of `Model/GeometryStageVS.js`.
pub static GEOMETRY_STAGE_VS: ShaderModule = ShaderModule {
    id: "Shaders/Model/GeometryStageVS",
    name: "GeometryStageVS",
    source: include_str!("glsl/GeometryStageVS.glsl"),
};

/// `ImageBasedLightingStageFS`, the exact text of `Model/ImageBasedLightingStageFS.js`.
pub static IMAGE_BASED_LIGHTING_STAGE_FS: ShaderModule = ShaderModule {
    id: "Shaders/Model/ImageBasedLightingStageFS",
    name: "ImageBasedLightingStageFS",
    source: include_str!("glsl/ImageBasedLightingStageFS.glsl"),
};

/// `InstancingStageCommon`, the exact text of `Model/InstancingStageCommon.js`.
pub static INSTANCING_STAGE_COMMON: ShaderModule = ShaderModule {
    id: "Shaders/Model/InstancingStageCommon",
    name: "InstancingStageCommon",
    source: include_str!("glsl/InstancingStageCommon.glsl"),
};

/// `InstancingStageVS`, the exact text of `Model/InstancingStageVS.js`.
pub static INSTANCING_STAGE_VS: ShaderModule = ShaderModule {
    id: "Shaders/Model/InstancingStageVS",
    name: "InstancingStageVS",
    source: include_str!("glsl/InstancingStageVS.glsl"),
};

/// `LegacyInstancingStageVS`, the exact text of `Model/LegacyInstancingStageVS.js`.
pub static LEGACY_INSTANCING_STAGE_VS: ShaderModule = ShaderModule {
    id: "Shaders/Model/LegacyInstancingStageVS",
    name: "LegacyInstancingStageVS",
    source: include_str!("glsl/LegacyInstancingStageVS.glsl"),
};

/// `LightingStageFS`, the exact text of `Model/LightingStageFS.js`.
pub static LIGHTING_STAGE_FS: ShaderModule = ShaderModule {
    id: "Shaders/Model/LightingStageFS",
    name: "LightingStageFS",
    source: include_str!("glsl/LightingStageFS.glsl"),
};

/// `MaterialStageFS`, the exact text of `Model/MaterialStageFS.js`.
pub static MATERIAL_STAGE_FS: ShaderModule = ShaderModule {
    id: "Shaders/Model/MaterialStageFS",
    name: "MaterialStageFS",
    source: include_str!("glsl/MaterialStageFS.glsl"),
};

/// `MetadataStageFS`, the exact text of `Model/MetadataStageFS.js`.
pub static METADATA_STAGE_FS: ShaderModule = ShaderModule {
    id: "Shaders/Model/MetadataStageFS",
    name: "MetadataStageFS",
    source: include_str!("glsl/MetadataStageFS.glsl"),
};

/// `MetadataStageVS`, the exact text of `Model/MetadataStageVS.js`.
pub static METADATA_STAGE_VS: ShaderModule = ShaderModule {
    id: "Shaders/Model/MetadataStageVS",
    name: "MetadataStageVS",
    source: include_str!("glsl/MetadataStageVS.glsl"),
};

/// `ModelClippingPlanesStageFS`, the exact text of `Model/ModelClippingPlanesStageFS.js`.
pub static MODEL_CLIPPING_PLANES_STAGE_FS: ShaderModule = ShaderModule {
    id: "Shaders/Model/ModelClippingPlanesStageFS",
    name: "ModelClippingPlanesStageFS",
    source: include_str!("glsl/ModelClippingPlanesStageFS.glsl"),
};

/// `ModelClippingPolygonsStageFS`, the exact text of `Model/ModelClippingPolygonsStageFS.js`.
pub static MODEL_CLIPPING_POLYGONS_STAGE_FS: ShaderModule = ShaderModule {
    id: "Shaders/Model/ModelClippingPolygonsStageFS",
    name: "ModelClippingPolygonsStageFS",
    source: include_str!("glsl/ModelClippingPolygonsStageFS.glsl"),
};

/// `ModelClippingPolygonsStageVS`, the exact text of `Model/ModelClippingPolygonsStageVS.js`.
pub static MODEL_CLIPPING_POLYGONS_STAGE_VS: ShaderModule = ShaderModule {
    id: "Shaders/Model/ModelClippingPolygonsStageVS",
    name: "ModelClippingPolygonsStageVS",
    source: include_str!("glsl/ModelClippingPolygonsStageVS.glsl"),
};

/// `ModelColorStageFS`, the exact text of `Model/ModelColorStageFS.js`.
pub static MODEL_COLOR_STAGE_FS: ShaderModule = ShaderModule {
    id: "Shaders/Model/ModelColorStageFS",
    name: "ModelColorStageFS",
    source: include_str!("glsl/ModelColorStageFS.glsl"),
};

/// `ModelFS`, the exact text of `Model/ModelFS.js`.
pub static MODEL_FS: ShaderModule =
    ShaderModule { id: "Shaders/Model/ModelFS", name: "ModelFS", source: include_str!("glsl/ModelFS.glsl") };

/// `ModelSilhouetteStageFS`, the exact text of `Model/ModelSilhouetteStageFS.js`.
pub static MODEL_SILHOUETTE_STAGE_FS: ShaderModule = ShaderModule {
    id: "Shaders/Model/ModelSilhouetteStageFS",
    name: "ModelSilhouetteStageFS",
    source: include_str!("glsl/ModelSilhouetteStageFS.glsl"),
};

/// `ModelSilhouetteStageVS`, the exact text of `Model/ModelSilhouetteStageVS.js`.
pub static MODEL_SILHOUETTE_STAGE_VS: ShaderModule = ShaderModule {
    id: "Shaders/Model/ModelSilhouetteStageVS",
    name: "ModelSilhouetteStageVS",
    source: include_str!("glsl/ModelSilhouetteStageVS.glsl"),
};

/// `ModelSplitterStageFS`, the exact text of `Model/ModelSplitterStageFS.js`.
pub static MODEL_SPLITTER_STAGE_FS: ShaderModule = ShaderModule {
    id: "Shaders/Model/ModelSplitterStageFS",
    name: "ModelSplitterStageFS",
    source: include_str!("glsl/ModelSplitterStageFS.glsl"),
};

/// `ModelVS`, the exact text of `Model/ModelVS.js`.
pub static MODEL_VS: ShaderModule =
    ShaderModule { id: "Shaders/Model/ModelVS", name: "ModelVS", source: include_str!("glsl/ModelVS.glsl") };

/// `MorphTargetsStageVS`, the exact text of `Model/MorphTargetsStageVS.js`.
pub static MORPH_TARGETS_STAGE_VS: ShaderModule = ShaderModule {
    id: "Shaders/Model/MorphTargetsStageVS",
    name: "MorphTargetsStageVS",
    source: include_str!("glsl/MorphTargetsStageVS.glsl"),
};

/// `PointCloudStylingStageVS`, the exact text of `Model/PointCloudStylingStageVS.js`.
pub static POINT_CLOUD_STYLING_STAGE_VS: ShaderModule = ShaderModule {
    id: "Shaders/Model/PointCloudStylingStageVS",
    name: "PointCloudStylingStageVS",
    source: include_str!("glsl/PointCloudStylingStageVS.glsl"),
};

/// `PrimitiveOutlineStageFS`, the exact text of `Model/PrimitiveOutlineStageFS.js`.
pub static PRIMITIVE_OUTLINE_STAGE_FS: ShaderModule = ShaderModule {
    id: "Shaders/Model/PrimitiveOutlineStageFS",
    name: "PrimitiveOutlineStageFS",
    source: include_str!("glsl/PrimitiveOutlineStageFS.glsl"),
};

/// `PrimitiveOutlineStageVS`, the exact text of `Model/PrimitiveOutlineStageVS.js`.
pub static PRIMITIVE_OUTLINE_STAGE_VS: ShaderModule = ShaderModule {
    id: "Shaders/Model/PrimitiveOutlineStageVS",
    name: "PrimitiveOutlineStageVS",
    source: include_str!("glsl/PrimitiveOutlineStageVS.glsl"),
};

/// `SelectedFeatureIdStageCommon`, the exact text of `Model/SelectedFeatureIdStageCommon.js`.
pub static SELECTED_FEATURE_ID_STAGE_COMMON: ShaderModule = ShaderModule {
    id: "Shaders/Model/SelectedFeatureIdStageCommon",
    name: "SelectedFeatureIdStageCommon",
    source: include_str!("glsl/SelectedFeatureIdStageCommon.glsl"),
};

/// `SkinningStageVS`, the exact text of `Model/SkinningStageVS.js`.
pub static SKINNING_STAGE_VS: ShaderModule = ShaderModule {
    id: "Shaders/Model/SkinningStageVS",
    name: "SkinningStageVS",
    source: include_str!("glsl/SkinningStageVS.glsl"),
};

/// `VerticalExaggerationStageVS`, the exact text of `Model/VerticalExaggerationStageVS.js`.
pub static VERTICAL_EXAGGERATION_STAGE_VS: ShaderModule = ShaderModule {
    id: "Shaders/Model/VerticalExaggerationStageVS",
    name: "VerticalExaggerationStageVS",
    source: include_str!("glsl/VerticalExaggerationStageVS.glsl"),
};

pub fn register(shaders: &mut Vec<&'static ShaderModule>) {
    shaders.push(&ATMOSPHERE_STAGE_FS);
    shaders.push(&ATMOSPHERE_STAGE_VS);
    shaders.push(&CPU_STYLING_STAGE_FS);
    shaders.push(&CPU_STYLING_STAGE_VS);
    shaders.push(&CUSTOM_SHADER_STAGE_FS);
    shaders.push(&CUSTOM_SHADER_STAGE_VS);
    shaders.push(&EDGE_DETECTION_STAGE_FS);
    shaders.push(&EDGE_VISIBILITY_STAGE_FS);
    shaders.push(&FEATURE_ID_STAGE_FS);
    shaders.push(&FEATURE_ID_STAGE_VS);
    shaders.push(&GEOMETRY_STAGE_FS);
    shaders.push(&GEOMETRY_STAGE_VS);
    shaders.push(&IMAGE_BASED_LIGHTING_STAGE_FS);
    shaders.push(&INSTANCING_STAGE_COMMON);
    shaders.push(&INSTANCING_STAGE_VS);
    shaders.push(&LEGACY_INSTANCING_STAGE_VS);
    shaders.push(&LIGHTING_STAGE_FS);
    shaders.push(&MATERIAL_STAGE_FS);
    shaders.push(&METADATA_STAGE_FS);
    shaders.push(&METADATA_STAGE_VS);
    shaders.push(&MODEL_CLIPPING_PLANES_STAGE_FS);
    shaders.push(&MODEL_CLIPPING_POLYGONS_STAGE_FS);
    shaders.push(&MODEL_CLIPPING_POLYGONS_STAGE_VS);
    shaders.push(&MODEL_COLOR_STAGE_FS);
    shaders.push(&MODEL_FS);
    shaders.push(&MODEL_SILHOUETTE_STAGE_FS);
    shaders.push(&MODEL_SILHOUETTE_STAGE_VS);
    shaders.push(&MODEL_SPLITTER_STAGE_FS);
    shaders.push(&MODEL_VS);
    shaders.push(&MORPH_TARGETS_STAGE_VS);
    shaders.push(&POINT_CLOUD_STYLING_STAGE_VS);
    shaders.push(&PRIMITIVE_OUTLINE_STAGE_FS);
    shaders.push(&PRIMITIVE_OUTLINE_STAGE_VS);
    shaders.push(&SELECTED_FEATURE_ID_STAGE_COMMON);
    shaders.push(&SKINNING_STAGE_VS);
    shaders.push(&VERTICAL_EXAGGERATION_STAGE_VS);
}
