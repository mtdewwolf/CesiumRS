//! glTF terrain data and mesh-backed children with unquantized barycentric upsampling.
use super::cesium_3d_tiles_terrain_geometry_processor::TilesTerrainMeshOptions;
use super::network_terrain::TerrainFuture;
use super::terrain_data::{TerrainMeshRequest, TerrainTile};
use super::*;
use std::sync::Arc;
#[derive(Clone, Debug)]
pub struct Cesium3DTilesTerrainDataOptions {
    pub gltf: TerrainGltf,
    pub minimum_height: f64,
    pub maximum_height: f64,
    pub bounding_sphere: BoundingSphere,
    pub oriented_bounding_box: OrientedBoundingBox,
    pub horizon_occlusion_point: Cartesian3,
    pub skirt_height: f64,
    pub request_vertex_normals: bool,
    pub child_tile_mask: u8,
    pub credits: Option<Vec<Credit>>,
    pub water_mask: Option<Vec<u8>>,
}
#[derive(Clone, Debug)]
pub struct Cesium3DTilesTerrainData {
    gltf: Option<TerrainGltf>,
    pub minimum_height: f64,
    pub maximum_height: f64,
    pub bounding_sphere: BoundingSphere,
    pub oriented_bounding_box: OrientedBoundingBox,
    pub horizon_occlusion_point: Cartesian3,
    pub skirt_height: f64,
    pub request_vertex_normals: bool,
    pub child_tile_mask: u8,
    pub credits: Option<Vec<Credit>>,
    pub water_mask: Option<Vec<u8>>,
    mesh: Option<Arc<TerrainMesh>>,
    upsampled: bool,
}
impl Cesium3DTilesTerrainData {
    pub fn new(o: Cesium3DTilesTerrainDataOptions) -> Self {
        Self {
            gltf: Some(o.gltf),
            minimum_height: o.minimum_height,
            maximum_height: o.maximum_height,
            bounding_sphere: o.bounding_sphere,
            oriented_bounding_box: o.oriented_bounding_box,
            horizon_occlusion_point: o.horizon_occlusion_point,
            skirt_height: o.skirt_height,
            request_vertex_normals: o.request_vertex_normals,
            child_tile_mask: o.child_tile_mask,
            credits: o.credits,
            water_mask: o.water_mask,
            mesh: None,
            upsampled: false,
        }
    }
    pub fn mesh(&self) -> Option<&TerrainMesh> {
        self.mesh.as_deref()
    }
    pub fn create_mesh_sync(&mut self, r: TerrainMeshRequest<'_>) -> Result<Arc<TerrainMesh>, RuntimeError> {
        if let Some(m) = &self.mesh {
            return Ok(m.clone());
        }
        let gltf = self.gltf.as_ref().ok_or_else(|| RuntimeError::new("Terrain glTF is missing."))?;
        let mesh = Cesium3DTilesTerrainGeometryProcessor::create_mesh(TilesTerrainMeshOptions {
            gltf,
            ellipsoid: *r.tiling_scheme.ellipsoid(),
            rectangle: r.tiling_scheme.tile_xy_to_rectangle(r.tile.x as f64, r.tile.y as f64, r.tile.level),
            has_vertex_normals: self.request_vertex_normals,
            has_web_mercator_t: true,
            minimum_height: self.minimum_height,
            maximum_height: self.maximum_height,
            bounding_sphere: self.bounding_sphere,
            oriented_bounding_box: self.oriented_bounding_box,
            horizon_occlusion_point: self.horizon_occlusion_point,
            skirt_height: self.skirt_height,
            exaggeration: r.exaggeration,
            exaggeration_relative_height: r.exaggeration_relative_height,
        })?;
        let mesh = Arc::new(mesh);
        self.mesh = Some(mesh.clone());
        self.gltf = None;
        Ok(mesh)
    }
    pub fn upsample_sync(
        &self,
        s: &dyn TilingScheme,
        a: TerrainTile,
        b: TerrainTile,
    ) -> Option<Result<Self, RuntimeError>> {
        let parent = self.mesh.as_ref()?;
        if b.level > a.level + 1 {
            return Some(Err(RuntimeError::new(
                "Upsampling through more than one level at a time is not currently supported.",
            )));
        }
        let skirt = self.skirt_height * 0.5;
        let mesh = match Cesium3DTilesTerrainGeometryProcessor::upsample_mesh(
            parent,
            *s.ellipsoid(),
            s.tile_xy_to_rectangle(b.x as f64, b.y as f64, b.level),
            a.x * 2 != b.x,
            a.y * 2 == b.y,
            skirt,
        ) {
            Ok(v) => v,
            Err(e) => return Some(Err(e)),
        };
        let mut result = self.clone();
        result.gltf = None;
        result.minimum_height = mesh.minimum_height;
        result.maximum_height = mesh.maximum_height;
        result.bounding_sphere = mesh.bounding_sphere_3d;
        result.oriented_bounding_box = mesh.oriented_bounding_box?;
        result.horizon_occlusion_point = mesh.occludee_point_in_scaled_space.unwrap_or(Cartesian3::ZERO);
        result.skirt_height = skirt;
        result.child_tile_mask = 0;
        result.water_mask = None;
        result.mesh = Some(Arc::new(mesh));
        result.upsampled = true;
        Some(Ok(result))
    }
}
impl TerrainData for Cesium3DTilesTerrainData {
    type Credit = Credit;
    type WaterMask = [u8];
    type MeshFuture<'a> = TerrainFuture<'a, Arc<TerrainMesh>>;
    type UpsampleFuture<'a> = TerrainFuture<'a, Self>;
    fn credits(&self) -> Option<&[Credit]> {
        self.credits.as_deref()
    }
    fn water_mask(&self) -> Option<&[u8]> {
        self.water_mask.as_deref()
    }
    fn interpolate_height(&self, r: &Rectangle, lon: f64, lat: f64) -> Option<f64> {
        let u = ((lon - r.west) / r.width()).clamp(0., 1.);
        let v = ((lat - r.south) / r.height()).clamp(0., 1.);
        super::google_earth_enterprise_terrain_data::interpolate_mesh(self.mesh.as_deref()?, u, v)
    }
    fn is_child_available(&self, x: u32, y: u32, cx: u32, cy: u32) -> bool {
        let bit = 2 + u8::from(cx != x * 2) - 2 * u8::from(cy != y * 2);
        self.child_tile_mask & (1 << bit) != 0
    }
    fn create_mesh<'a>(&'a mut self, r: TerrainMeshRequest<'a>) -> Option<Self::MeshFuture<'a>> {
        Some(Box::pin(async move { self.create_mesh_sync(r) }))
    }
    fn upsample<'a>(
        &'a self,
        s: &'a dyn TilingScheme,
        a: TerrainTile,
        b: TerrainTile,
    ) -> Option<Self::UpsampleFuture<'a>> {
        let result = self.upsample_sync(s, a, b)?;
        Some(Box::pin(async { result }))
    }
    fn was_created_by_upsampling(&self) -> bool {
        self.upsampled
    }
}
