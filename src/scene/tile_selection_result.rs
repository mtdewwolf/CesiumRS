//! CesiumJS `TileSelectionResult`: what quadtree selection did with a tile in
//! the last frame. Kicked results set bit 4 and keep the original in bits 0-1.

numeric_enum!(TileSelectionResult: i32 {
    None = 0 => "NONE",
    Culled = 1 => "CULLED",
    Rendered = 2 => "RENDERED",
    Refined = 3 => "REFINED",
    RenderedAndKicked = 2 | 4 => "RENDERED_AND_KICKED",
    RefinedAndKicked = 3 | 4 => "REFINED_AND_KICKED",
    CulledButNeeded = 1 | 8 => "CULLED_BUT_NEEDED",
});

impl TileSelectionResult {
    /// Cesium compares numerically, so `CulledButNeeded` (9) also counts.
    pub fn was_kicked(value: i32) -> bool {
        value >= Self::RenderedAndKicked.value()
    }

    pub fn original_result(value: i32) -> i32 {
        value & 3
    }

    pub fn kick(value: i32) -> i32 {
        value | 4
    }
}
