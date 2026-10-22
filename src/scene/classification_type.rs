//! CesiumJS `ClassificationType`: what a ground or 3D Tiles classification
//! primitive classifies.

numeric_enum!(ClassificationType: i32 {
    Terrain = 0 => "TERRAIN",
    Cesium3DTile = 1 => "CESIUM_3D_TILE",
    Both = 2 => "BOTH",
});

impl ClassificationType {
    pub const NUMBER_OF_CLASSIFICATION_TYPES: i32 = 3;
}
