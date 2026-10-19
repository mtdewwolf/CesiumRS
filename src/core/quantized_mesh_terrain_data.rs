//! CesiumJS QuantizedMeshTerrainData, with native ready-future processing.
//! Generic credits preserve caller-owned credit types without a browser host.
use super::bounding_sphere::BoundingSphere;
use super::cartesian2::Cartesian2;
use super::cartesian3::Cartesian3;
use super::index_datatype::IndexDatatype;
use super::intersections_2d::compute_barycentric_coordinates;
use super::math::{clamp, js_min, lerp};
use super::oriented_bounding_box::OrientedBoundingBox;
use super::quantized_terrain_processor::{self, QuantizedTerrainEdges, QuantizedTerrainMeshRequest};
use super::quantized_terrain_upsample::{self, QuantizedTerrainUpsampleRequest};
use super::rectangle::Rectangle;
use super::runtime_error::RuntimeError;
use super::terrain_data::{TerrainData, TerrainMeshRequest, TerrainTile};
use super::terrain_mesh::{TerrainMesh, TerrainMeshOptions};
use super::tiling_scheme::TilingScheme;
use super::typed_array::TypedArray;
use std::future::{ready, Ready};
use std::sync::Arc;

#[derive(Clone, Debug)]
pub struct QuantizedMeshTerrainDataOptions<C = String> {
    pub quantized_vertices: Vec<u16>,
    pub indices: TypedArray,
    pub encoded_normals: Option<Vec<u8>>,
    pub minimum_height: f64,
    pub maximum_height: f64,
    pub bounding_sphere: BoundingSphere,
    pub oriented_bounding_box: Option<OrientedBoundingBox>,
    pub horizon_occlusion_point: Cartesian3,
    pub edges: QuantizedTerrainEdges,
    pub skirt_heights: [f64; 4],
    pub child_tile_mask: u8,
    pub created_by_upsampling: bool,
    pub water_mask: Option<Vec<u8>>,
    pub credits: Option<Vec<C>>,
}
impl<C> QuantizedMeshTerrainDataOptions<C> {
    pub fn new(
        quantized_vertices: Vec<u16>,
        indices: TypedArray,
        heights: [f64; 2],
        bounding_sphere: BoundingSphere,
        horizon_occlusion_point: Cartesian3,
    ) -> Self {
        Self {
            quantized_vertices,
            indices,
            encoded_normals: None,
            minimum_height: heights[0],
            maximum_height: heights[1],
            bounding_sphere,
            oriented_bounding_box: None,
            horizon_occlusion_point,
            edges: QuantizedTerrainEdges::default(),
            skirt_heights: [0.0; 4],
            child_tile_mask: 15,
            created_by_upsampling: false,
            water_mask: None,
            credits: None,
        }
    }
}
#[derive(Clone, Debug)]
pub struct QuantizedMeshTerrainData<C = String> {
    quantized_vertices: Option<Vec<u16>>,
    indices: Option<TypedArray>,
    encoded_normals: Option<Vec<u8>>,
    edges: Option<QuantizedTerrainEdges>,
    minimum_height: f64,
    maximum_height: f64,
    bounding_sphere: BoundingSphere,
    oriented_bounding_box: Option<OrientedBoundingBox>,
    horizon_occlusion_point: Cartesian3,
    skirt_heights: [f64; 4],
    child_tile_mask: u8,
    created_by_upsampling: bool,
    water_mask: Option<Vec<u8>>,
    credits: Option<Vec<C>>,
    mesh: Option<Arc<TerrainMesh>>,
}
impl<C: Clone> QuantizedMeshTerrainData<C> {
    pub fn new(mut o: QuantizedMeshTerrainDataOptions<C>) -> Option<Self> {
        if o.quantized_vertices.is_empty() || !o.quantized_vertices.len().is_multiple_of(3) {
            return None;
        }
        let count = o.quantized_vertices.len() / 3;
        IndexDatatype::from_typed_array(&o.indices)?;
        if !o.indices.len().is_multiple_of(3) || o.indices.to_f64_vec().iter().any(|&i| i as usize >= count) {
            return None;
        }
        if o.encoded_normals.as_ref().is_some_and(|n| n.len() < count * 2)
            || o.edges.all().iter().any(|e| e.iter().any(|&i| i as usize >= count))
        {
            return None;
        }
        o.edges.west.sort_by_key(|&i| o.quantized_vertices[count + i as usize]);
        o.edges.east.sort_by_key(|&i| o.quantized_vertices[count + i as usize]);
        o.edges.south.sort_by_key(|&i| o.quantized_vertices[i as usize]);
        o.edges.north.sort_by_key(|&i| o.quantized_vertices[i as usize]);
        Some(Self {
            quantized_vertices: Some(o.quantized_vertices),
            indices: Some(o.indices),
            encoded_normals: o.encoded_normals,
            edges: Some(o.edges),
            minimum_height: o.minimum_height,
            maximum_height: o.maximum_height,
            bounding_sphere: o.bounding_sphere,
            oriented_bounding_box: o.oriented_bounding_box,
            horizon_occlusion_point: o.horizon_occlusion_point,
            skirt_heights: o.skirt_heights,
            child_tile_mask: o.child_tile_mask,
            created_by_upsampling: o.created_by_upsampling,
            water_mask: o.water_mask,
            credits: o.credits,
            mesh: None,
        })
    }
    pub fn quantized_vertices(&self) -> Option<&[u16]> {
        self.quantized_vertices.as_deref()
    }
    pub fn indices(&self) -> Option<&TypedArray> {
        self.indices.as_ref()
    }
    pub fn encoded_normals(&self) -> Option<&[u8]> {
        self.encoded_normals.as_deref()
    }
    pub fn edges(&self) -> Option<&QuantizedTerrainEdges> {
        self.edges.as_ref()
    }
    pub fn minimum_height(&self) -> f64 {
        self.minimum_height
    }
    pub fn maximum_height(&self) -> f64 {
        self.maximum_height
    }
    pub fn bounding_sphere(&self) -> BoundingSphere {
        self.bounding_sphere
    }
    pub fn oriented_bounding_box(&self) -> Option<OrientedBoundingBox> {
        self.oriented_bounding_box
    }
    pub fn horizon_occlusion_point(&self) -> Cartesian3 {
        self.horizon_occlusion_point
    }
    pub fn skirt_heights(&self) -> [f64; 4] {
        self.skirt_heights
    }
    pub fn child_tile_mask(&self) -> u8 {
        self.child_tile_mask
    }
    pub fn credits(&self) -> Option<&[C]> {
        self.credits.as_deref()
    }
    pub fn water_mask(&self) -> Option<&[u8]> {
        self.water_mask.as_deref()
    }
    pub fn was_created_by_upsampling(&self) -> bool {
        self.created_by_upsampling
    }
    pub fn can_upsample(&self) -> bool {
        self.mesh.is_some()
    }
    pub fn mesh(&self) -> Option<&TerrainMesh> {
        self.mesh.as_deref()
    }
    pub fn mesh_mut(&mut self) -> Option<&mut TerrainMesh> {
        self.mesh.as_mut().map(Arc::make_mut)
    }

    pub fn create_mesh_sync(&mut self, request: TerrainMeshRequest<'_>) -> Result<Arc<TerrainMesh>, RuntimeError> {
        let vertices = self
            .quantized_vertices
            .as_ref()
            .ok_or_else(|| RuntimeError::new("Quantized terrain buffers were released"))?;
        let indices = self.indices.as_ref().unwrap();
        let edges = self.edges.as_ref().unwrap();
        let TerrainTile { x, y, level } = request.tile;
        let rectangle = request.tiling_scheme.tile_xy_to_rectangle(x as f64, y as f64, level);
        let result =
            quantized_terrain_processor::create_vertices_from_quantized_terrain_mesh(QuantizedTerrainMeshRequest {
                quantized_vertices: vertices,
                indices,
                encoded_normals: self.encoded_normals.as_deref(),
                edges,
                skirt_heights: self.skirt_heights,
                rectangle,
                center: self.bounding_sphere.center,
                ellipsoid: *request.tiling_scheme.ellipsoid(),
                minimum_height: self.minimum_height,
                maximum_height: self.maximum_height,
                include_web_mercator_t: true,
                exaggeration: request.exaggeration,
                exaggeration_relative_height: request.exaggeration_relative_height,
            })
            .ok_or_else(|| RuntimeError::new("Invalid quantized terrain mesh"))?;
        let mesh = Arc::new(TerrainMesh::new(TerrainMeshOptions {
            center: self.bounding_sphere.center,
            vertices: result.vertices,
            indices: result.indices,
            index_count_without_skirts: Some(result.index_count_without_skirts),
            vertex_count_without_skirts: vertices.len() / 3,
            minimum_height: self.minimum_height,
            maximum_height: self.maximum_height,
            rectangle,
            bounding_sphere_3d: self.bounding_sphere,
            occludee_point_in_scaled_space: Some(
                result.occludee_point_in_scaled_space.unwrap_or(self.horizon_occlusion_point),
            ),
            vertex_stride: Some(result.encoding.stride),
            oriented_bounding_box: self.oriented_bounding_box,
            encoding: result.encoding,
            edges: result.edges,
        }));
        self.quantized_vertices = None;
        self.indices = None;
        self.encoded_normals = None;
        self.edges = None;
        self.mesh = Some(mesh.clone());
        Ok(mesh)
    }

    pub fn upsample_sync(
        &self,
        scheme: &dyn TilingScheme,
        source: TerrainTile,
        destination: TerrainTile,
    ) -> Option<Self> {
        let mesh = self.mesh.as_ref()?;
        let east = source.x as u64 * 2 != destination.x as u64;
        let north = source.y as u64 * 2 == destination.y as u64;
        let rectangle = scheme.tile_xy_to_rectangle(destination.x as f64, destination.y as f64, destination.level);
        let r = quantized_terrain_upsample::upsample_quantized_terrain_mesh(QuantizedTerrainUpsampleRequest {
            mesh,
            minimum_height: self.minimum_height,
            maximum_height: self.maximum_height,
            is_east_child: east,
            is_north_child: north,
            child_rectangle: rectangle,
            ellipsoid: *scheme.ellipsoid(),
        })?;
        let shortest = js_min(
            js_min(js_min(self.skirt_heights[0], self.skirt_heights[2]), self.skirt_heights[1]),
            self.skirt_heights[3],
        );
        Self::new(QuantizedMeshTerrainDataOptions {
            quantized_vertices: r.vertices,
            indices: r.indices,
            encoded_normals: r.encoded_normals,
            minimum_height: r.minimum_height,
            maximum_height: r.maximum_height,
            bounding_sphere: r.bounding_sphere,
            oriented_bounding_box: Some(r.oriented_bounding_box),
            horizon_occlusion_point: r.horizon_occlusion_point?,
            edges: r.edges,
            skirt_heights: [
                if east { shortest * 0.5 } else { self.skirt_heights[0] },
                if north { shortest * 0.5 } else { self.skirt_heights[1] },
                if east { self.skirt_heights[2] } else { shortest * 0.5 },
                if north { self.skirt_heights[3] } else { shortest * 0.5 },
            ],
            child_tile_mask: 0,
            created_by_upsampling: true,
            water_mask: None,
            credits: self.credits.clone(),
        })
    }

    pub fn interpolate_height(&self, r: &Rectangle, longitude: f64, latitude: f64) -> Option<f64> {
        let u = clamp((longitude - r.west) / r.width(), 0.0, 1.0) * 32767.0;
        let v = clamp((latitude - r.south) / r.height(), 0.0, 1.0) * 32767.0;
        let indices = if let Some(mesh) = self.mesh.as_ref() { &mesh.indices } else { self.indices.as_ref()? };
        let vertex = |i: usize| -> Option<(Cartesian2, f64)> {
            if let Some(mesh) = self.mesh.as_ref() {
                Some((
                    mesh.encoding.decode_texture_coordinates(&mesh.vertices, i)?,
                    mesh.encoding.decode_height(&mesh.vertices, i)?,
                ))
            } else {
                let vertices = self.quantized_vertices.as_ref()?;
                let count = vertices.len() / 3;
                Some((
                    Cartesian2::new(*vertices.get(i)? as f64, *vertices.get(i + count)? as f64),
                    *vertices.get(i + count * 2)? as f64,
                ))
            }
        };
        for start in (0..indices.len()).step_by(3) {
            let (a, ha) = vertex(indices.get(start)? as usize)?;
            let (b, hb) = vertex(indices.get(start + 1)? as usize)?;
            let (c, hc) = vertex(indices.get(start + 2)? as usize)?;
            if u < a.x.min(b.x).min(c.x)
                || u > a.x.max(b.x).max(c.x)
                || v < a.y.min(b.y).min(c.y)
                || v > a.y.max(b.y).max(c.y)
            {
                continue;
            }
            let bary = compute_barycentric_coordinates(u, v, a.x, a.y, b.x, b.y, c.x, c.y);
            if bary.x >= -1e-15 && bary.y >= -1e-15 && bary.z >= -1e-15 {
                let height = bary.x * ha + bary.y * hb + bary.z * hc;
                return Some(if self.mesh.is_some() {
                    height
                } else {
                    lerp(self.minimum_height, self.maximum_height, height / 32767.0)
                });
            }
        }
        None
    }
    pub fn is_child_available(&self, x: u32, y: u32, cx: u32, cy: u32) -> bool {
        let bit = 2 + u8::from(cx as u64 != x as u64 * 2) - 2 * u8::from(cy as u64 != y as u64 * 2);
        self.child_tile_mask & (1 << bit) != 0
    }
}

impl<C: Clone> TerrainData for QuantizedMeshTerrainData<C> {
    type Credit = C;
    type WaterMask = [u8];
    type MeshFuture<'a>
        = Ready<Result<Arc<TerrainMesh>, RuntimeError>>
    where
        C: 'a;
    type UpsampleFuture<'a>
        = Ready<Result<Self, RuntimeError>>
    where
        C: 'a;
    fn credits(&self) -> Option<&[C]> {
        self.credits()
    }
    fn water_mask(&self) -> Option<&[u8]> {
        self.water_mask()
    }
    fn interpolate_height(&self, r: &Rectangle, lon: f64, lat: f64) -> Option<f64> {
        self.interpolate_height(r, lon, lat)
    }
    fn is_child_available(&self, x: u32, y: u32, cx: u32, cy: u32) -> bool {
        self.is_child_available(x, y, cx, cy)
    }
    fn create_mesh<'a>(&'a mut self, request: TerrainMeshRequest<'a>) -> Option<Self::MeshFuture<'a>> {
        Some(ready(self.create_mesh_sync(request)))
    }
    fn upsample<'a>(
        &'a self,
        scheme: &'a dyn TilingScheme,
        source: TerrainTile,
        destination: TerrainTile,
    ) -> Option<Self::UpsampleFuture<'a>> {
        self.upsample_sync(scheme, source, destination).map(|data| ready(Ok(data)))
    }
    fn was_created_by_upsampling(&self) -> bool {
        self.created_by_upsampling
    }
}
