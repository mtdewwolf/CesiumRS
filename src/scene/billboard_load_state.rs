//! CesiumJS `BillboardLoadState`: the load state of a billboard image.

numeric_enum!(BillboardLoadState: i32 {
    None = 0 => "NONE",
    Loading = 2 => "LOADING",
    Loaded = 3 => "LOADED",
    Error = 4 => "ERROR",
    Failed = 5 => "FAILED",
});
