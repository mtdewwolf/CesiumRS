//! `Source/Shaders/Materials`.
//!
//! Each shader is one `shaders.push` line in [`register`]. Its name is the
//! file stem, which is how Scene imports it
//! (`import GridMaterial from "../Shaders/Materials/GridMaterial.js"`).
//! Upstream gives these modules no `@alias`. The source sits next to this
//! file as `<stem>.glsl`.

use super::ShaderModule;

/// `AspectRampMaterial`, the exact text of `Materials/AspectRampMaterial.js`.
pub static ASPECT_RAMP_MATERIAL: ShaderModule = ShaderModule {
    id: "Shaders/Materials/AspectRampMaterial",
    name: "AspectRampMaterial",
    source: include_str!("glsl/AspectRampMaterial.glsl"),
};

/// `BumpMapMaterial`, the exact text of `Materials/BumpMapMaterial.js`.
pub static BUMP_MAP_MATERIAL: ShaderModule = ShaderModule {
    id: "Shaders/Materials/BumpMapMaterial",
    name: "BumpMapMaterial",
    source: include_str!("glsl/BumpMapMaterial.glsl"),
};

/// `CheckerboardMaterial`, the exact text of `Materials/CheckerboardMaterial.js`.
pub static CHECKERBOARD_MATERIAL: ShaderModule = ShaderModule {
    id: "Shaders/Materials/CheckerboardMaterial",
    name: "CheckerboardMaterial",
    source: include_str!("glsl/CheckerboardMaterial.glsl"),
};

/// `DotMaterial`, the exact text of `Materials/DotMaterial.js`.
pub static DOT_MATERIAL: ShaderModule = ShaderModule {
    id: "Shaders/Materials/DotMaterial",
    name: "DotMaterial",
    source: include_str!("glsl/DotMaterial.glsl"),
};

/// `ElevationBandMaterial`, the exact text of `Materials/ElevationBandMaterial.js`.
pub static ELEVATION_BAND_MATERIAL: ShaderModule = ShaderModule {
    id: "Shaders/Materials/ElevationBandMaterial",
    name: "ElevationBandMaterial",
    source: include_str!("glsl/ElevationBandMaterial.glsl"),
};

/// `ElevationContourMaterial`, the exact text of `Materials/ElevationContourMaterial.js`.
pub static ELEVATION_CONTOUR_MATERIAL: ShaderModule = ShaderModule {
    id: "Shaders/Materials/ElevationContourMaterial",
    name: "ElevationContourMaterial",
    source: include_str!("glsl/ElevationContourMaterial.glsl"),
};

/// `ElevationRampMaterial`, the exact text of `Materials/ElevationRampMaterial.js`.
pub static ELEVATION_RAMP_MATERIAL: ShaderModule = ShaderModule {
    id: "Shaders/Materials/ElevationRampMaterial",
    name: "ElevationRampMaterial",
    source: include_str!("glsl/ElevationRampMaterial.glsl"),
};

/// `FadeMaterial`, the exact text of `Materials/FadeMaterial.js`.
pub static FADE_MATERIAL: ShaderModule = ShaderModule {
    id: "Shaders/Materials/FadeMaterial",
    name: "FadeMaterial",
    source: include_str!("glsl/FadeMaterial.glsl"),
};

/// `GridMaterial`, the exact text of `Materials/GridMaterial.js`.
pub static GRID_MATERIAL: ShaderModule = ShaderModule {
    id: "Shaders/Materials/GridMaterial",
    name: "GridMaterial",
    source: include_str!("glsl/GridMaterial.glsl"),
};

/// `NormalMapMaterial`, the exact text of `Materials/NormalMapMaterial.js`.
pub static NORMAL_MAP_MATERIAL: ShaderModule = ShaderModule {
    id: "Shaders/Materials/NormalMapMaterial",
    name: "NormalMapMaterial",
    source: include_str!("glsl/NormalMapMaterial.glsl"),
};

/// `PolylineArrowMaterial`, the exact text of `Materials/PolylineArrowMaterial.js`.
pub static POLYLINE_ARROW_MATERIAL: ShaderModule = ShaderModule {
    id: "Shaders/Materials/PolylineArrowMaterial",
    name: "PolylineArrowMaterial",
    source: include_str!("glsl/PolylineArrowMaterial.glsl"),
};

/// `PolylineDashMaterial`, the exact text of `Materials/PolylineDashMaterial.js`.
pub static POLYLINE_DASH_MATERIAL: ShaderModule = ShaderModule {
    id: "Shaders/Materials/PolylineDashMaterial",
    name: "PolylineDashMaterial",
    source: include_str!("glsl/PolylineDashMaterial.glsl"),
};

/// `PolylineGlowMaterial`, the exact text of `Materials/PolylineGlowMaterial.js`.
pub static POLYLINE_GLOW_MATERIAL: ShaderModule = ShaderModule {
    id: "Shaders/Materials/PolylineGlowMaterial",
    name: "PolylineGlowMaterial",
    source: include_str!("glsl/PolylineGlowMaterial.glsl"),
};

/// `PolylineOutlineMaterial`, the exact text of `Materials/PolylineOutlineMaterial.js`.
pub static POLYLINE_OUTLINE_MATERIAL: ShaderModule = ShaderModule {
    id: "Shaders/Materials/PolylineOutlineMaterial",
    name: "PolylineOutlineMaterial",
    source: include_str!("glsl/PolylineOutlineMaterial.glsl"),
};

/// `RimLightingMaterial`, the exact text of `Materials/RimLightingMaterial.js`.
pub static RIM_LIGHTING_MATERIAL: ShaderModule = ShaderModule {
    id: "Shaders/Materials/RimLightingMaterial",
    name: "RimLightingMaterial",
    source: include_str!("glsl/RimLightingMaterial.glsl"),
};

/// `SlopeRampMaterial`, the exact text of `Materials/SlopeRampMaterial.js`.
pub static SLOPE_RAMP_MATERIAL: ShaderModule = ShaderModule {
    id: "Shaders/Materials/SlopeRampMaterial",
    name: "SlopeRampMaterial",
    source: include_str!("glsl/SlopeRampMaterial.glsl"),
};

/// `StripeMaterial`, the exact text of `Materials/StripeMaterial.js`.
pub static STRIPE_MATERIAL: ShaderModule = ShaderModule {
    id: "Shaders/Materials/StripeMaterial",
    name: "StripeMaterial",
    source: include_str!("glsl/StripeMaterial.glsl"),
};

/// `Water`, the exact text of `Materials/Water.js`.
pub static WATER: ShaderModule =
    ShaderModule { id: "Shaders/Materials/Water", name: "Water", source: include_str!("glsl/Water.glsl") };

/// `WaterMaskMaterial`, the exact text of `Materials/WaterMaskMaterial.js`.
pub static WATER_MASK_MATERIAL: ShaderModule = ShaderModule {
    id: "Shaders/Materials/WaterMaskMaterial",
    name: "WaterMaskMaterial",
    source: include_str!("glsl/WaterMaskMaterial.glsl"),
};

pub fn register(shaders: &mut Vec<&'static ShaderModule>) {
    shaders.push(&ASPECT_RAMP_MATERIAL);
    shaders.push(&BUMP_MAP_MATERIAL);
    shaders.push(&CHECKERBOARD_MATERIAL);
    shaders.push(&DOT_MATERIAL);
    shaders.push(&ELEVATION_BAND_MATERIAL);
    shaders.push(&ELEVATION_CONTOUR_MATERIAL);
    shaders.push(&ELEVATION_RAMP_MATERIAL);
    shaders.push(&FADE_MATERIAL);
    shaders.push(&GRID_MATERIAL);
    shaders.push(&NORMAL_MAP_MATERIAL);
    shaders.push(&POLYLINE_ARROW_MATERIAL);
    shaders.push(&POLYLINE_DASH_MATERIAL);
    shaders.push(&POLYLINE_GLOW_MATERIAL);
    shaders.push(&POLYLINE_OUTLINE_MATERIAL);
    shaders.push(&RIM_LIGHTING_MATERIAL);
    shaders.push(&SLOPE_RAMP_MATERIAL);
    shaders.push(&STRIPE_MATERIAL);
    shaders.push(&WATER);
    shaders.push(&WATER_MASK_MATERIAL);
}
