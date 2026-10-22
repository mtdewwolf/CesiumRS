//! CesiumJS `MetadataSemantic`: the built-in 3D Metadata semantics.

string_enum!(MetadataSemantic {
    Id = "ID" => "ID",
    Name = "NAME" => "NAME",
    Description = "DESCRIPTION" => "DESCRIPTION",
    TilesetTileCount = "TILESET_TILE_COUNT" => "TILESET_TILE_COUNT",
    TileBoundingBox = "TILE_BOUNDING_BOX" => "TILE_BOUNDING_BOX",
    TileBoundingRegion = "TILE_BOUNDING_REGION" => "TILE_BOUNDING_REGION",
    TileBoundingSphere = "TILE_BOUNDING_SPHERE" => "TILE_BOUNDING_SPHERE",
    TileMinimumHeight = "TILE_MINIMUM_HEIGHT" => "TILE_MINIMUM_HEIGHT",
    TileMaximumHeight = "TILE_MAXIMUM_HEIGHT" => "TILE_MAXIMUM_HEIGHT",
    TileHorizonOcclusionPoint = "TILE_HORIZON_OCCLUSION_POINT" => "TILE_HORIZON_OCCLUSION_POINT",
    TileGeometricError = "TILE_GEOMETRIC_ERROR" => "TILE_GEOMETRIC_ERROR",
    ContentBoundingBox = "CONTENT_BOUNDING_BOX" => "CONTENT_BOUNDING_BOX",
    ContentBoundingRegion = "CONTENT_BOUNDING_REGION" => "CONTENT_BOUNDING_REGION",
    ContentBoundingSphere = "CONTENT_BOUNDING_SPHERE" => "CONTENT_BOUNDING_SPHERE",
    ContentMinimumHeight = "CONTENT_MINIMUM_HEIGHT" => "CONTENT_MINIMUM_HEIGHT",
    ContentMaximumHeight = "CONTENT_MAXIMUM_HEIGHT" => "CONTENT_MAXIMUM_HEIGHT",
    ContentHorizonOcclusionPoint = "CONTENT_HORIZON_OCCLUSION_POINT" => "CONTENT_HORIZON_OCCLUSION_POINT",
});
