//! CesiumJS `TerrainState`: the load state of a globe tile's terrain.

numeric_enum!(TerrainState: i32 {
    Failed = 0 => "FAILED",
    Unloaded = 1 => "UNLOADED",
    Receiving = 2 => "RECEIVING",
    Received = 3 => "RECEIVED",
    Transforming = 4 => "TRANSFORMING",
    Transformed = 5 => "TRANSFORMED",
    Ready = 6 => "READY",
});
