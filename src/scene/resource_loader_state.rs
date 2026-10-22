//! CesiumJS `ResourceLoaderState`: the state of a glTF or 3D Tiles resource loader.

numeric_enum!(ResourceLoaderState: i32 {
    Unloaded = 0 => "UNLOADED",
    Loading = 1 => "LOADING",
    Loaded = 2 => "LOADED",
    Processing = 3 => "PROCESSING",
    Ready = 4 => "READY",
    Failed = 5 => "FAILED",
});
