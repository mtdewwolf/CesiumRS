//! CesiumJS `Cesium3DTileOptimizationHint`: whether a tile's children are worth the child-union culling optimization.

numeric_enum!(Cesium3DTileOptimizationHint: i32 {
    NotComputed = -1 => "NOT_COMPUTED",
    UseOptimization = 1 => "USE_OPTIMIZATION",
    SkipOptimization = 0 => "SKIP_OPTIMIZATION",
});
