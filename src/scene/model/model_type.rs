//! CesiumJS `ModelType`: the format a `Model` was loaded from.

string_enum!(ModelType {
    Gltf = "GLTF" => "GLTF",
    TileGltf = "TILE_GLTF" => "TILE_GLTF",
    TileB3dm = "B3DM" => "TILE_B3DM",
    TileI3dm = "I3DM" => "TILE_I3DM",
    TilePnts = "PNTS" => "TILE_PNTS",
    TileGeojson = "TILE_GEOJSON" => "TILE_GEOJSON",
});

impl ModelType {
    /// Whether the model is 3D Tiles content rather than a standalone glTF.
    pub fn is_3d_tiles(self) -> bool {
        self != Self::Gltf
    }
}
