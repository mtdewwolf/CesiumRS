//! `Source/Shaders/PostProcessStages`.
//!
//! Each shader is one `shaders.push` line in [`register`]. Its name is the
//! file stem, which is how Scene imports it
//! (`import FXAA from "../Shaders/PostProcessStages/FXAA.js"`).
//! Upstream gives these modules no `@alias`. The source sits next to this
//! file as `glsl/<stem>.glsl`.

use super::ShaderModule;

/// `AcesTonemappingStage`, the exact text of `PostProcessStages/AcesTonemappingStage.js`.
pub static ACES_TONEMAPPING_STAGE: ShaderModule = ShaderModule {
    id: "Shaders/PostProcessStages/AcesTonemappingStage",
    name: "AcesTonemappingStage",
    source: include_str!("glsl/AcesTonemappingStage.glsl"),
};

/// `AdditiveBlend`, the exact text of `PostProcessStages/AdditiveBlend.js`.
pub static ADDITIVE_BLEND: ShaderModule = ShaderModule {
    id: "Shaders/PostProcessStages/AdditiveBlend",
    name: "AdditiveBlend",
    source: include_str!("glsl/AdditiveBlend.glsl"),
};

/// `AmbientOcclusionGenerate`, the exact text of `PostProcessStages/AmbientOcclusionGenerate.js`.
pub static AMBIENT_OCCLUSION_GENERATE: ShaderModule = ShaderModule {
    id: "Shaders/PostProcessStages/AmbientOcclusionGenerate",
    name: "AmbientOcclusionGenerate",
    source: include_str!("glsl/AmbientOcclusionGenerate.glsl"),
};

/// `AmbientOcclusionModulate`, the exact text of `PostProcessStages/AmbientOcclusionModulate.js`.
pub static AMBIENT_OCCLUSION_MODULATE: ShaderModule = ShaderModule {
    id: "Shaders/PostProcessStages/AmbientOcclusionModulate",
    name: "AmbientOcclusionModulate",
    source: include_str!("glsl/AmbientOcclusionModulate.glsl"),
};

/// `BlackAndWhite`, the exact text of `PostProcessStages/BlackAndWhite.js`.
pub static BLACK_AND_WHITE: ShaderModule = ShaderModule {
    id: "Shaders/PostProcessStages/BlackAndWhite",
    name: "BlackAndWhite",
    source: include_str!("glsl/BlackAndWhite.glsl"),
};

/// `BloomComposite`, the exact text of `PostProcessStages/BloomComposite.js`.
pub static BLOOM_COMPOSITE: ShaderModule = ShaderModule {
    id: "Shaders/PostProcessStages/BloomComposite",
    name: "BloomComposite",
    source: include_str!("glsl/BloomComposite.glsl"),
};

/// `BrightPass`, the exact text of `PostProcessStages/BrightPass.js`.
pub static BRIGHT_PASS: ShaderModule = ShaderModule {
    id: "Shaders/PostProcessStages/BrightPass",
    name: "BrightPass",
    source: include_str!("glsl/BrightPass.glsl"),
};

/// `Brightness`, the exact text of `PostProcessStages/Brightness.js`.
pub static BRIGHTNESS: ShaderModule = ShaderModule {
    id: "Shaders/PostProcessStages/Brightness",
    name: "Brightness",
    source: include_str!("glsl/Brightness.glsl"),
};

/// `CompositeTranslucentClassification`, the exact text of `PostProcessStages/CompositeTranslucentClassification.js`.
pub static COMPOSITE_TRANSLUCENT_CLASSIFICATION: ShaderModule = ShaderModule {
    id: "Shaders/PostProcessStages/CompositeTranslucentClassification",
    name: "CompositeTranslucentClassification",
    source: include_str!("glsl/CompositeTranslucentClassification.glsl"),
};

/// `ContrastBias`, the exact text of `PostProcessStages/ContrastBias.js`.
pub static CONTRAST_BIAS: ShaderModule = ShaderModule {
    id: "Shaders/PostProcessStages/ContrastBias",
    name: "ContrastBias",
    source: include_str!("glsl/ContrastBias.glsl"),
};

/// `DepthOfField`, the exact text of `PostProcessStages/DepthOfField.js`.
pub static DEPTH_OF_FIELD: ShaderModule = ShaderModule {
    id: "Shaders/PostProcessStages/DepthOfField",
    name: "DepthOfField",
    source: include_str!("glsl/DepthOfField.glsl"),
};

/// `DepthView`, the exact text of `PostProcessStages/DepthView.js`.
pub static DEPTH_VIEW: ShaderModule = ShaderModule {
    id: "Shaders/PostProcessStages/DepthView",
    name: "DepthView",
    source: include_str!("glsl/DepthView.glsl"),
};

/// `DepthViewPacked`, the exact text of `PostProcessStages/DepthViewPacked.js`.
pub static DEPTH_VIEW_PACKED: ShaderModule = ShaderModule {
    id: "Shaders/PostProcessStages/DepthViewPacked",
    name: "DepthViewPacked",
    source: include_str!("glsl/DepthViewPacked.glsl"),
};

/// `EdgeDetection`, the exact text of `PostProcessStages/EdgeDetection.js`.
pub static EDGE_DETECTION: ShaderModule = ShaderModule {
    id: "Shaders/PostProcessStages/EdgeDetection",
    name: "EdgeDetection",
    source: include_str!("glsl/EdgeDetection.glsl"),
};

/// `FXAA`, the exact text of `PostProcessStages/FXAA.js`.
pub static FXAA: ShaderModule =
    ShaderModule { id: "Shaders/PostProcessStages/FXAA", name: "FXAA", source: include_str!("glsl/FXAA.glsl") };

/// `FilmicTonemapping`, the exact text of `PostProcessStages/FilmicTonemapping.js`.
pub static FILMIC_TONEMAPPING: ShaderModule = ShaderModule {
    id: "Shaders/PostProcessStages/FilmicTonemapping",
    name: "FilmicTonemapping",
    source: include_str!("glsl/FilmicTonemapping.glsl"),
};

/// `GaussianBlur1D`, the exact text of `PostProcessStages/GaussianBlur1D.js`.
pub static GAUSSIAN_BLUR1D: ShaderModule = ShaderModule {
    id: "Shaders/PostProcessStages/GaussianBlur1D",
    name: "GaussianBlur1D",
    source: include_str!("glsl/GaussianBlur1D.glsl"),
};

/// `LensFlare`, the exact text of `PostProcessStages/LensFlare.js`.
pub static LENS_FLARE: ShaderModule = ShaderModule {
    id: "Shaders/PostProcessStages/LensFlare",
    name: "LensFlare",
    source: include_str!("glsl/LensFlare.glsl"),
};

/// `ModifiedReinhardTonemapping`, the exact text of `PostProcessStages/ModifiedReinhardTonemapping.js`.
pub static MODIFIED_REINHARD_TONEMAPPING: ShaderModule = ShaderModule {
    id: "Shaders/PostProcessStages/ModifiedReinhardTonemapping",
    name: "ModifiedReinhardTonemapping",
    source: include_str!("glsl/ModifiedReinhardTonemapping.glsl"),
};

/// `NightVision`, the exact text of `PostProcessStages/NightVision.js`.
pub static NIGHT_VISION: ShaderModule = ShaderModule {
    id: "Shaders/PostProcessStages/NightVision",
    name: "NightVision",
    source: include_str!("glsl/NightVision.glsl"),
};

/// `PassThrough`, the exact text of `PostProcessStages/PassThrough.js`.
pub static PASS_THROUGH: ShaderModule = ShaderModule {
    id: "Shaders/PostProcessStages/PassThrough",
    name: "PassThrough",
    source: include_str!("glsl/PassThrough.glsl"),
};

/// `PassThroughDepth`, the exact text of `PostProcessStages/PassThroughDepth.js`.
pub static PASS_THROUGH_DEPTH: ShaderModule = ShaderModule {
    id: "Shaders/PostProcessStages/PassThroughDepth",
    name: "PassThroughDepth",
    source: include_str!("glsl/PassThroughDepth.glsl"),
};

/// `PbrNeutralTonemapping`, the exact text of `PostProcessStages/PbrNeutralTonemapping.js`.
pub static PBR_NEUTRAL_TONEMAPPING: ShaderModule = ShaderModule {
    id: "Shaders/PostProcessStages/PbrNeutralTonemapping",
    name: "PbrNeutralTonemapping",
    source: include_str!("glsl/PbrNeutralTonemapping.glsl"),
};

/// `PointCloudEyeDomeLighting`, the exact text of `PostProcessStages/PointCloudEyeDomeLighting.js`.
pub static POINT_CLOUD_EYE_DOME_LIGHTING: ShaderModule = ShaderModule {
    id: "Shaders/PostProcessStages/PointCloudEyeDomeLighting",
    name: "PointCloudEyeDomeLighting",
    source: include_str!("glsl/PointCloudEyeDomeLighting.glsl"),
};

/// `ReinhardTonemapping`, the exact text of `PostProcessStages/ReinhardTonemapping.js`.
pub static REINHARD_TONEMAPPING: ShaderModule = ShaderModule {
    id: "Shaders/PostProcessStages/ReinhardTonemapping",
    name: "ReinhardTonemapping",
    source: include_str!("glsl/ReinhardTonemapping.glsl"),
};

/// `Silhouette`, the exact text of `PostProcessStages/Silhouette.js`.
pub static SILHOUETTE: ShaderModule = ShaderModule {
    id: "Shaders/PostProcessStages/Silhouette",
    name: "Silhouette",
    source: include_str!("glsl/Silhouette.glsl"),
};

pub fn register(shaders: &mut Vec<&'static ShaderModule>) {
    shaders.push(&ACES_TONEMAPPING_STAGE);
    shaders.push(&ADDITIVE_BLEND);
    shaders.push(&AMBIENT_OCCLUSION_GENERATE);
    shaders.push(&AMBIENT_OCCLUSION_MODULATE);
    shaders.push(&BLACK_AND_WHITE);
    shaders.push(&BLOOM_COMPOSITE);
    shaders.push(&BRIGHT_PASS);
    shaders.push(&BRIGHTNESS);
    shaders.push(&COMPOSITE_TRANSLUCENT_CLASSIFICATION);
    shaders.push(&CONTRAST_BIAS);
    shaders.push(&DEPTH_OF_FIELD);
    shaders.push(&DEPTH_VIEW);
    shaders.push(&DEPTH_VIEW_PACKED);
    shaders.push(&EDGE_DETECTION);
    shaders.push(&FXAA);
    shaders.push(&FILMIC_TONEMAPPING);
    shaders.push(&GAUSSIAN_BLUR1D);
    shaders.push(&LENS_FLARE);
    shaders.push(&MODIFIED_REINHARD_TONEMAPPING);
    shaders.push(&NIGHT_VISION);
    shaders.push(&PASS_THROUGH);
    shaders.push(&PASS_THROUGH_DEPTH);
    shaders.push(&PBR_NEUTRAL_TONEMAPPING);
    shaders.push(&POINT_CLOUD_EYE_DOME_LIGHTING);
    shaders.push(&REINHARD_TONEMAPPING);
    shaders.push(&SILHOUETTE);
}
