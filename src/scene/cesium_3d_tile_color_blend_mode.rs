//! CesiumJS `Cesium3DTileColorBlendMode`: how a 3D Tiles feature color mixes with the source color.

numeric_enum!(Cesium3DTileColorBlendMode: i32 {
    Highlight = 0 => "HIGHLIGHT",
    Replace = 1 => "REPLACE",
    Mix = 2 => "MIX",
});
