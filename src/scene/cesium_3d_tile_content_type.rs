//! CesiumJS `Cesium3DTileContentType`: the content formats a 3D Tiles tile
//! can reference.

string_enum!(Cesium3DTileContentType {
    Batched3DModel = "b3dm" => "BATCHED_3D_MODEL",
    Instanced3DModel = "i3dm" => "INSTANCED_3D_MODEL",
    Composite = "cmpt" => "COMPOSITE",
    PointCloud = "pnts" => "POINT_CLOUD",
    Vector = "vctr" => "VECTOR",
    Geometry = "geom" => "GEOMETRY",
    Gltf = "gltf" => "GLTF",
    GltfBinary = "glb" => "GLTF_BINARY",
    ImplicitSubtree = "subt" => "IMPLICIT_SUBTREE",
    ImplicitSubtreeJson = "subtreeJson" => "IMPLICIT_SUBTREE_JSON",
    ExternalTileset = "externalTileset" => "EXTERNAL_TILESET",
    MultipleContent = "multipleContent" => "MULTIPLE_CONTENT",
    GeoJson = "geoJson" => "GEOJSON",
    VoxelBinary = "voxl" => "VOXEL_BINARY",
    VoxelJson = "voxelJson" => "VOXEL_JSON",
});

impl Cesium3DTileContentType {
    /// Whether the content is a binary payload rather than JSON.
    pub fn is_binary_format(self) -> bool {
        matches!(
            self,
            Self::Batched3DModel
                | Self::Instanced3DModel
                | Self::Composite
                | Self::PointCloud
                | Self::Vector
                | Self::Geometry
                | Self::ImplicitSubtree
                | Self::VoxelBinary
                | Self::GltfBinary
        )
    }
}
