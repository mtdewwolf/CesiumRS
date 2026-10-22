//! CesiumJS `TileState`: the load state of a terrain tile's data.

numeric_enum!(TileState: i32 {
    Start = 0 => "START",
    Loading = 1 => "LOADING",
    Ready = 2 => "READY",
    UpsampledOnly = 3 => "UPSAMPLED_ONLY",
});
