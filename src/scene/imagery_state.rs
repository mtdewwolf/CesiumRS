//! CesiumJS `ImageryState`: the load state of an imagery tile.

numeric_enum!(ImageryState: i32 {
    Unloaded = 0 => "UNLOADED",
    Transitioning = 1 => "TRANSITIONING",
    Received = 2 => "RECEIVED",
    TextureLoaded = 3 => "TEXTURE_LOADED",
    Ready = 4 => "READY",
    Failed = 5 => "FAILED",
    Invalid = 6 => "INVALID",
    Placeholder = 7 => "PLACEHOLDER",
});
