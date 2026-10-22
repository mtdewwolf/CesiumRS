//! CesiumJS `QuadtreeTileLoadState`: the load state of a quadtree tile.

numeric_enum!(QuadtreeTileLoadState: i32 {
    Start = 0 => "START",
    Loading = 1 => "LOADING",
    Done = 2 => "DONE",
    Failed = 3 => "FAILED",
});
