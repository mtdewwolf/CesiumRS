//! CesiumJS `HeightReference`: what a position's height is relative to.

numeric_enum!(HeightReference: i32 {
    None = 0 => "NONE",
    ClampToGround = 1 => "CLAMP_TO_GROUND",
    RelativeToGround = 2 => "RELATIVE_TO_GROUND",
    ClampToTerrain = 3 => "CLAMP_TO_TERRAIN",
    RelativeToTerrain = 4 => "RELATIVE_TO_TERRAIN",
    ClampTo3DTile = 5 => "CLAMP_TO_3D_TILE",
    RelativeTo3DTile = 6 => "RELATIVE_TO_3D_TILE",
});

/// Whether the reference clamps to ground, terrain or 3D Tiles.
pub fn is_height_reference_clamp(height_reference: HeightReference) -> bool {
    use HeightReference::*;
    matches!(height_reference, ClampToGround | ClampTo3DTile | ClampToTerrain)
}

/// Whether the height is relative to ground, terrain or 3D Tiles.
pub fn is_height_reference_relative(height_reference: HeightReference) -> bool {
    use HeightReference::*;
    matches!(height_reference, RelativeToGround | RelativeTo3DTile | RelativeToTerrain)
}
