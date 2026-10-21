//! Google Earth Enterprise four-quadrant terrain decoding, skirts and upsampling.
use super::network_terrain::{Reader, TerrainFuture};
use super::quantized_mesh_terrain_data::QuantizedMeshTerrainDataOptions;
use super::quantized_terrain_upsample::{upsample_quantized_terrain_mesh, QuantizedTerrainUpsampleRequest};
use super::terrain_data::{TerrainMeshRequest, TerrainTile};
use super::terrain_mesh::TerrainMeshOptions;
use super::*;
use std::sync::Arc;
#[derive(Clone, Debug)]
pub struct GoogleEarthEnterpriseTerrainData {
    buffer: Option<Vec<u8>>,
    pub credits: Option<Vec<Credit>>,
    pub negative_altitude_exponent_bias: f64,
    pub negative_elevation_threshold: f64,
    child_tile_mask: u8,
    mesh: Option<Arc<TerrainMesh>>,
    skirt_height: f64,
    child: Option<QuantizedMeshTerrainData<Credit>>,
}
struct Quad {
    step: Cartesian2,
    points: Vec<(u8, u8, f64)>,
    indices: Vec<u32>,
}
fn quads(bytes: &[u8]) -> Result<Vec<Quad>, RuntimeError> {
    let mut r = Reader::new(bytes);
    let mut out = vec![];
    for _ in 0..4 {
        let len = r.u32()? as usize;
        let mut q = Reader::new(r.take(len)?);
        q.take(16)?; // origin
        let step = Cartesian2::new(
            (q.f64()? * 180.) * (std::f64::consts::PI / 180.),
            (q.f64()? * 180.) * (std::f64::consts::PI / 180.),
        );
        let count = q.u32()? as usize;
        let faces = q.u32()? as usize;
        q.u32()?;
        if count > len / 6 || faces > len / 6 {
            return Err(RuntimeError::new("Invalid terrain tile."));
        }
        let points = (0..count).map(|_| Ok((q.u8()?, q.u8()?, q.f32()?))).collect::<Result<Vec<_>, RuntimeError>>()?;
        let indices = q.indices(faces * 3, false)?;
        if q.pos != len || indices.iter().any(|&i| i as usize >= count) {
            return Err(RuntimeError::new("Invalid terrain tile."));
        }
        out.push(Quad { step, points, indices });
    }
    Ok(out)
}
fn interpolate(u: f64, v: f64, points: &[(Cartesian2, f64)], indices: &[u32]) -> Option<f64> {
    for triangle in indices.chunks_exact(3) {
        let (a, ha) = points[triangle[0] as usize];
        let (b, hb) = points[triangle[1] as usize];
        let (c, hc) = points[triangle[2] as usize];
        let w = super::intersections_2d::compute_barycentric_coordinates(u, v, a.x, a.y, b.x, b.y, c.x, c.y);
        if w.x >= -1e-15 && w.y >= -1e-15 && w.z >= -1e-15 {
            return Some(w.x * ha + w.y * hb + w.z * hc);
        }
    }
    None
}
pub(crate) fn interpolate_mesh(mesh: &TerrainMesh, u: f64, v: f64) -> Option<f64> {
    let count = mesh.vertices.len() / mesh.stride;
    let points = (0..count)
        .map(|i| {
            Some((
                mesh.encoding.decode_texture_coordinates(&mesh.vertices, i)?,
                mesh.encoding.decode_height(&mesh.vertices, i)?,
            ))
        })
        .collect::<Option<Vec<_>>>()?;
    let indices = (0..mesh.indices.len()).map(|i| mesh.indices.get(i).map(|i| i as u32)).collect::<Option<Vec<_>>>()?;
    interpolate(u, v, &points, &indices)
}
impl GoogleEarthEnterpriseTerrainData {
    pub fn new(
        buffer: Vec<u8>,
        negative_altitude_exponent_bias: f64,
        negative_elevation_threshold: f64,
        google_child_mask: u8,
        credits: Option<Vec<Credit>>,
    ) -> Self {
        Self {
            buffer: Some(buffer),
            credits,
            negative_altitude_exponent_bias,
            negative_elevation_threshold,
            child_tile_mask: (google_child_mask & 3)
                | if google_child_mask & 4 != 0 { 8 } else { 0 }
                | if google_child_mask & 8 != 0 { 4 } else { 0 },
            mesh: None,
            skirt_height: 0.,
            child: None,
        }
    }
    pub fn buffer(&self) -> Option<&[u8]> {
        self.buffer.as_deref()
    }
    pub fn child_tile_mask(&self) -> u8 {
        self.child.as_ref().map_or(self.child_tile_mask, |c| c.child_tile_mask())
    }
    pub fn mesh(&self) -> Option<&TerrainMesh> {
        self.child.as_ref().and_then(|c| c.mesh()).or(self.mesh.as_deref())
    }
    pub fn interpolate_height(&self, r: &Rectangle, lon: f64, lat: f64) -> Option<f64> {
        if let Some(c) = &self.child {
            return c.interpolate_height(r, lon, lat);
        }
        let u = ((lon - r.west) / r.width()).clamp(0., 1.);
        let v = ((lat - r.south) / r.height()).clamp(0., 1.);
        if let Some(m) = &self.mesh {
            return interpolate_mesh(m, u, v);
        }
        let quads = quads(self.buffer.as_deref()?).ok()?;
        let (index, us, vs) = if v > 0.5 {
            if u > 0.5 {
                (2, 0.5, 0.5)
            } else {
                (3, 0., 0.5)
            }
        } else if u > 0.5 {
            (1, 0.5, 0.)
        } else {
            (0, 0., 0.)
        };
        let q = &quads[index];
        let sx = r.width() / q.step.x / 2.;
        let sy = r.height() / q.step.y / 2.;
        let pts = q
            .points
            .iter()
            .map(|&(x, y, h)| (Cartesian2::new(us + x as f64 * sx, vs + y as f64 * sy), h * 6371010.))
            .collect::<Vec<_>>();
        interpolate(u, v, &pts, &q.indices)
    }
    pub fn create_mesh_sync(&mut self, request: TerrainMeshRequest<'_>) -> Result<Arc<TerrainMesh>, RuntimeError> {
        if let Some(c) = &mut self.child {
            return c.create_mesh_sync(request);
        }
        if let Some(m) = &self.mesh {
            return Ok(m.clone());
        }
        let buffer = self.buffer.as_deref().ok_or_else(|| RuntimeError::new("Invalid terrain tile."))?;
        let r = request.tiling_scheme.tile_xy_to_rectangle(
            request.tile.x as f64,
            request.tile.y as f64,
            request.tile.level,
        );
        let e = *request.tiling_scheme.ellipsoid();
        let center = e.cartographic_to_cartesian(&r.center());
        self.skirt_height = (40075.16 / 1i32.wrapping_shl(request.tile.level) as f64 * 8.).min(1000.);
        let result = create_vertices_from_google_earth_enterprise_buffer(GoogleEarthEnterpriseBufferRequest {
            buffer,
            relative_to_center: center,
            ellipsoid: e,
            rectangle: r,
            exaggeration: request.exaggeration,
            exaggeration_relative_height: request.exaggeration_relative_height,
            skirt_height: self.skirt_height,
            include_web_mercator_t: true,
            negative_altitude_exponent_bias: self.negative_altitude_exponent_bias,
            negative_elevation_threshold: self.negative_elevation_threshold,
        })?;
        let mesh = Arc::new(TerrainMesh::new(TerrainMeshOptions {
            center,
            vertices: result.vertices,
            indices: TypedArray::Uint16(result.indices),
            index_count_without_skirts: Some(result.index_count_without_skirts),
            vertex_count_without_skirts: result.vertex_count_without_skirts,
            minimum_height: result.minimum_height,
            maximum_height: result.maximum_height,
            rectangle: r,
            bounding_sphere_3d: result.bounding_sphere_3d,
            occludee_point_in_scaled_space: result.occludee_point_in_scaled_space,
            vertex_stride: Some(result.number_of_attributes),
            oriented_bounding_box: result.oriented_bounding_box,
            encoding: result.encoding,
            edges: result.edges,
        }));
        self.buffer = None;
        self.mesh = Some(mesh.clone());
        Ok(mesh)
    }
    pub fn upsample_quantized_sync(
        &self,
        scheme: &dyn TilingScheme,
        source: TerrainTile,
        dest: TerrainTile,
    ) -> Option<QuantizedMeshTerrainData<Credit>> {
        if let Some(c) = &self.child {
            return c.upsample_sync(scheme, source, dest);
        }
        if dest.level > source.level + 1 {
            return None;
        }
        let mesh = self.mesh.as_ref()?;
        let r = upsample_quantized_terrain_mesh(QuantizedTerrainUpsampleRequest {
            mesh,
            minimum_height: mesh.minimum_height,
            maximum_height: mesh.maximum_height,
            is_east_child: source.x * 2 != dest.x,
            is_north_child: source.y * 2 == dest.y,
            child_rectangle: scheme.tile_xy_to_rectangle(dest.x as f64, dest.y as f64, dest.level),
            ellipsoid: *scheme.ellipsoid(),
        })?;
        let mut options = QuantizedMeshTerrainDataOptions::new(
            r.vertices,
            r.indices,
            [r.minimum_height, r.maximum_height],
            r.bounding_sphere,
            r.horizon_occlusion_point?,
        );
        options.oriented_bounding_box = Some(r.oriented_bounding_box);
        options.edges = r.edges;
        options.skirt_heights = [self.skirt_height; 4];
        options.child_tile_mask = 0;
        options.created_by_upsampling = true;
        options.credits = self.credits.clone();
        QuantizedMeshTerrainData::new(options)
    }
}
impl TerrainData for GoogleEarthEnterpriseTerrainData {
    type Credit = Credit;
    type WaterMask = [u8];
    type MeshFuture<'a> = TerrainFuture<'a, Arc<TerrainMesh>>;
    type UpsampleFuture<'a> = TerrainFuture<'a, Self>;
    fn credits(&self) -> Option<&[Credit]> {
        self.credits.as_deref()
    }
    fn water_mask(&self) -> Option<&[u8]> {
        None
    }
    fn interpolate_height(&self, r: &Rectangle, lon: f64, lat: f64) -> Option<f64> {
        self.interpolate_height(r, lon, lat)
    }
    fn is_child_available(&self, x: u32, y: u32, cx: u32, cy: u32) -> bool {
        let bit = 2 + u8::from(cx != x * 2) - 2 * u8::from(cy != y * 2);
        self.child_tile_mask() & (1 << bit) != 0
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
        let child = self.upsample_quantized_sync(s, a, b)?;
        let mut data = self.clone();
        data.buffer = None;
        data.mesh = None;
        data.child = Some(child);
        Some(Box::pin(async { Ok(data) }))
    }
    fn was_created_by_upsampling(&self) -> bool {
        self.child.is_some()
    }
}
