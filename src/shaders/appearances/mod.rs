//! `Source/Shaders/Appearances`.
//!
//! Each shader is one `shaders.push` line in [`register`]. Its name is the
//! file stem, which is how Scene imports it
//! (`import PerInstanceColorAppearanceFS from "../Shaders/Appearances/PerInstanceColorAppearanceFS.js"`).
//! Upstream gives these modules no `@alias`. The source sits next to this
//! file as `<stem>.glsl`.

use super::ShaderModule;

/// `AllMaterialAppearanceFS`, the exact text of `Appearances/AllMaterialAppearanceFS.js`.
pub static ALL_MATERIAL_APPEARANCE_FS: ShaderModule = ShaderModule {
    id: "Shaders/Appearances/AllMaterialAppearanceFS",
    name: "AllMaterialAppearanceFS",
    source: include_str!("glsl/AllMaterialAppearanceFS.glsl"),
};

/// `AllMaterialAppearanceVS`, the exact text of `Appearances/AllMaterialAppearanceVS.js`.
pub static ALL_MATERIAL_APPEARANCE_VS: ShaderModule = ShaderModule {
    id: "Shaders/Appearances/AllMaterialAppearanceVS",
    name: "AllMaterialAppearanceVS",
    source: include_str!("glsl/AllMaterialAppearanceVS.glsl"),
};

/// `BasicMaterialAppearanceFS`, the exact text of `Appearances/BasicMaterialAppearanceFS.js`.
pub static BASIC_MATERIAL_APPEARANCE_FS: ShaderModule = ShaderModule {
    id: "Shaders/Appearances/BasicMaterialAppearanceFS",
    name: "BasicMaterialAppearanceFS",
    source: include_str!("glsl/BasicMaterialAppearanceFS.glsl"),
};

/// `BasicMaterialAppearanceVS`, the exact text of `Appearances/BasicMaterialAppearanceVS.js`.
pub static BASIC_MATERIAL_APPEARANCE_VS: ShaderModule = ShaderModule {
    id: "Shaders/Appearances/BasicMaterialAppearanceVS",
    name: "BasicMaterialAppearanceVS",
    source: include_str!("glsl/BasicMaterialAppearanceVS.glsl"),
};

/// `EllipsoidSurfaceAppearanceFS`, the exact text of `Appearances/EllipsoidSurfaceAppearanceFS.js`.
pub static ELLIPSOID_SURFACE_APPEARANCE_FS: ShaderModule = ShaderModule {
    id: "Shaders/Appearances/EllipsoidSurfaceAppearanceFS",
    name: "EllipsoidSurfaceAppearanceFS",
    source: include_str!("glsl/EllipsoidSurfaceAppearanceFS.glsl"),
};

/// `EllipsoidSurfaceAppearanceVS`, the exact text of `Appearances/EllipsoidSurfaceAppearanceVS.js`.
pub static ELLIPSOID_SURFACE_APPEARANCE_VS: ShaderModule = ShaderModule {
    id: "Shaders/Appearances/EllipsoidSurfaceAppearanceVS",
    name: "EllipsoidSurfaceAppearanceVS",
    source: include_str!("glsl/EllipsoidSurfaceAppearanceVS.glsl"),
};

/// `PerInstanceColorAppearanceFS`, the exact text of `Appearances/PerInstanceColorAppearanceFS.js`.
pub static PER_INSTANCE_COLOR_APPEARANCE_FS: ShaderModule = ShaderModule {
    id: "Shaders/Appearances/PerInstanceColorAppearanceFS",
    name: "PerInstanceColorAppearanceFS",
    source: include_str!("glsl/PerInstanceColorAppearanceFS.glsl"),
};

/// `PerInstanceColorAppearanceVS`, the exact text of `Appearances/PerInstanceColorAppearanceVS.js`.
pub static PER_INSTANCE_COLOR_APPEARANCE_VS: ShaderModule = ShaderModule {
    id: "Shaders/Appearances/PerInstanceColorAppearanceVS",
    name: "PerInstanceColorAppearanceVS",
    source: include_str!("glsl/PerInstanceColorAppearanceVS.glsl"),
};

/// `PerInstanceFlatColorAppearanceFS`, the exact text of `Appearances/PerInstanceFlatColorAppearanceFS.js`.
pub static PER_INSTANCE_FLAT_COLOR_APPEARANCE_FS: ShaderModule = ShaderModule {
    id: "Shaders/Appearances/PerInstanceFlatColorAppearanceFS",
    name: "PerInstanceFlatColorAppearanceFS",
    source: include_str!("glsl/PerInstanceFlatColorAppearanceFS.glsl"),
};

/// `PerInstanceFlatColorAppearanceVS`, the exact text of `Appearances/PerInstanceFlatColorAppearanceVS.js`.
pub static PER_INSTANCE_FLAT_COLOR_APPEARANCE_VS: ShaderModule = ShaderModule {
    id: "Shaders/Appearances/PerInstanceFlatColorAppearanceVS",
    name: "PerInstanceFlatColorAppearanceVS",
    source: include_str!("glsl/PerInstanceFlatColorAppearanceVS.glsl"),
};

/// `PolylineColorAppearanceVS`, the exact text of `Appearances/PolylineColorAppearanceVS.js`.
pub static POLYLINE_COLOR_APPEARANCE_VS: ShaderModule = ShaderModule {
    id: "Shaders/Appearances/PolylineColorAppearanceVS",
    name: "PolylineColorAppearanceVS",
    source: include_str!("glsl/PolylineColorAppearanceVS.glsl"),
};

/// `PolylineMaterialAppearanceVS`, the exact text of `Appearances/PolylineMaterialAppearanceVS.js`.
pub static POLYLINE_MATERIAL_APPEARANCE_VS: ShaderModule = ShaderModule {
    id: "Shaders/Appearances/PolylineMaterialAppearanceVS",
    name: "PolylineMaterialAppearanceVS",
    source: include_str!("glsl/PolylineMaterialAppearanceVS.glsl"),
};

/// `TexturedMaterialAppearanceFS`, the exact text of `Appearances/TexturedMaterialAppearanceFS.js`.
pub static TEXTURED_MATERIAL_APPEARANCE_FS: ShaderModule = ShaderModule {
    id: "Shaders/Appearances/TexturedMaterialAppearanceFS",
    name: "TexturedMaterialAppearanceFS",
    source: include_str!("glsl/TexturedMaterialAppearanceFS.glsl"),
};

/// `TexturedMaterialAppearanceVS`, the exact text of `Appearances/TexturedMaterialAppearanceVS.js`.
pub static TEXTURED_MATERIAL_APPEARANCE_VS: ShaderModule = ShaderModule {
    id: "Shaders/Appearances/TexturedMaterialAppearanceVS",
    name: "TexturedMaterialAppearanceVS",
    source: include_str!("glsl/TexturedMaterialAppearanceVS.glsl"),
};

pub fn register(shaders: &mut Vec<&'static ShaderModule>) {
    shaders.push(&ALL_MATERIAL_APPEARANCE_FS);
    shaders.push(&ALL_MATERIAL_APPEARANCE_VS);
    shaders.push(&BASIC_MATERIAL_APPEARANCE_FS);
    shaders.push(&BASIC_MATERIAL_APPEARANCE_VS);
    shaders.push(&ELLIPSOID_SURFACE_APPEARANCE_FS);
    shaders.push(&ELLIPSOID_SURFACE_APPEARANCE_VS);
    shaders.push(&PER_INSTANCE_COLOR_APPEARANCE_FS);
    shaders.push(&PER_INSTANCE_COLOR_APPEARANCE_VS);
    shaders.push(&PER_INSTANCE_FLAT_COLOR_APPEARANCE_FS);
    shaders.push(&PER_INSTANCE_FLAT_COLOR_APPEARANCE_VS);
    shaders.push(&POLYLINE_COLOR_APPEARANCE_VS);
    shaders.push(&POLYLINE_MATERIAL_APPEARANCE_VS);
    shaders.push(&TEXTURED_MATERIAL_APPEARANCE_FS);
    shaders.push(&TEXTURED_MATERIAL_APPEARANCE_VS);
}
