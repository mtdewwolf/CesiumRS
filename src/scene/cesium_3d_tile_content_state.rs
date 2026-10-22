//! CesiumJS `Cesium3DTileContentState`: the load state of a 3D Tiles content.

numeric_enum!(Cesium3DTileContentState: i32 {
    Unloaded = 0 => "UNLOADED",
    Loading = 1 => "LOADING",
    Processing = 2 => "PROCESSING",
    Ready = 3 => "READY",
    Expired = 4 => "EXPIRED",
    Failed = 5 => "FAILED",
});
