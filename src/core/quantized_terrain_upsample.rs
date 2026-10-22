//! Native worker calculation for upsampleQuantizedTerrainMesh. Vertex order,
//! recursive clipping keys and quantized normal interpolation follow Cesium.
use super::attribute_compression::AttributeCompression;
use super::bounding_sphere::BoundingSphere;
use super::cartesian2::Cartesian2;
use super::cartesian3::Cartesian3;
use super::cartographic::Cartographic;
use super::ellipsoid::Ellipsoid;
use super::ellipsoidal_occluder::EllipsoidalOccluder;
use super::index_datatype::IndexDatatype;
use super::intersections_2d::clip_triangle_at_axis_aligned_threshold as clip;
use super::math::{clamp, js_round, lerp, to_int32, TWO_PI};
use super::oriented_bounding_box::OrientedBoundingBox;
use super::quantized_terrain_processor::QuantizedTerrainEdges;
use super::rectangle::Rectangle;
use super::terrain_mesh::TerrainMesh;
use super::typed_array::TypedArray;
use std::collections::HashMap;

#[derive(Clone, Copy)]
pub struct QuantizedTerrainUpsampleRequest<'a> {
    pub mesh: &'a TerrainMesh,
    pub minimum_height: f64,
    pub maximum_height: f64,
    pub is_east_child: bool,
    pub is_north_child: bool,
    pub child_rectangle: Rectangle,
    pub ellipsoid: Ellipsoid,
}
#[derive(Clone, Debug, PartialEq)]
pub struct QuantizedTerrainUpsampleResult {
    pub vertices: Vec<u16>,
    pub indices: TypedArray,
    pub encoded_normals: Option<Vec<u8>>,
    pub minimum_height: f64,
    pub maximum_height: f64,
    pub edges: QuantizedTerrainEdges,
    pub bounding_sphere: BoundingSphere,
    pub oriented_bounding_box: OrientedBoundingBox,
    pub horizon_occlusion_point: Option<Cartesian3>,
}

#[derive(Clone)]
enum Vertex {
    Indexed(usize),
    Interpolated(Box<Vertex>, Box<Vertex>, f64),
}
#[derive(Clone, PartialEq, Eq, Hash)]
enum VertexKey {
    Indexed(usize),
    Interpolated(Box<VertexKey>, Box<VertexKey>, u64),
}
struct Parent {
    uvh: Vec<[f64; 3]>,
    normals: Option<Vec<Cartesian2>>,
}
impl Vertex {
    fn values(&self, parent: &Parent) -> [f64; 3] {
        match self {
            Self::Indexed(i) => parent.uvh[*i],
            Self::Interpolated(a, b, t) => {
                let a = a.values(parent);
                let b = b.values(parent);
                [lerp(a[0], b[0], *t), lerp(a[1], b[1], *t), lerp(a[2], b[2], *t)]
            }
        }
    }
    fn key(&self) -> VertexKey {
        match self {
            Self::Indexed(i) => VertexKey::Indexed(*i),
            Self::Interpolated(a, b, t) => {
                VertexKey::Interpolated(Box::new(a.key()), Box::new(b.key()), if *t == 0.0 { 0 } else { t.to_bits() })
            }
        }
    }
    fn normal(&self, parent: &Parent) -> Option<Cartesian2> {
        match self {
            Self::Indexed(i) => parent.normals.as_ref().and_then(|normals| normals.get(*i)).copied(),
            Self::Interpolated(a, b, t) => {
                let a = a.normal(parent)?;
                let b = b.normal(parent)?;
                let a = AttributeCompression::oct_decode(a.x, a.y);
                let b = AttributeCompression::oct_decode(b.x, b.y);
                Some(AttributeCompression::oct_encode(a.lerp(b, *t).normalize()))
            }
        }
    }
}
fn clipped_vertices(result: &[f64], vertices: &[Vertex]) -> Option<Vec<Vertex>> {
    let mut polygon = Vec::new();
    let mut i = 0;
    while i < result.len() {
        if result[i] != -1.0 {
            polygon.push(vertices.get(result[i] as usize)?.clone());
            i += 1;
        } else {
            polygon.push(Vertex::Interpolated(
                Box::new(vertices.get(*result.get(i + 1)? as usize)?.clone()),
                Box::new(vertices.get(*result.get(i + 2)? as usize)?.clone()),
                *result.get(i + 3)?,
            ));
            i += 4;
        }
    }
    Some(polygon)
}
fn add_polygon(
    polygon: Vec<Vertex>,
    parent: &Parent,
    map: &mut HashMap<VertexKey, u32>,
    values: &mut Vec<[f64; 3]>,
    normals: &mut Vec<Cartesian2>,
    indices: &mut Vec<u32>,
) -> Option<()> {
    let mut polygon_indices = Vec::new();
    for vertex in polygon {
        let key = vertex.key();
        let index = if let Some(index) = map.get(&key) {
            *index
        } else {
            // An indexed vertex inside both clipping boundaries was added in
            // the initial pass. Only interpolated vertices are created here.
            if matches!(vertex, Vertex::Indexed(_)) {
                return None;
            }
            let index = u32::try_from(values.len()).ok()?;
            values.push(vertex.values(parent));
            if parent.normals.is_some() {
                normals.push(vertex.normal(parent)?);
            }
            map.insert(key, index);
            index
        };
        polygon_indices.push(index);
    }
    if polygon_indices.len() == 3 {
        indices.extend(&polygon_indices);
    } else if polygon_indices.len() == 4 {
        indices.extend([
            polygon_indices[0],
            polygon_indices[1],
            polygon_indices[2],
            polygon_indices[0],
            polygon_indices[2],
            polygon_indices[3],
        ]);
    }
    Some(())
}

pub fn upsample_quantized_terrain_mesh(
    p: QuantizedTerrainUpsampleRequest<'_>,
) -> Option<QuantizedTerrainUpsampleResult> {
    let mesh = p.mesh;
    let encoding = &mesh.encoding;
    let mut parent = Parent {
        uvh: Vec::with_capacity(mesh.vertex_count_without_skirts),
        normals: encoding.has_vertex_normals.then(Vec::new),
    };
    let mut values = Vec::new();
    let mut normals = Vec::new();
    let mut map = HashMap::new();
    let mut indices = Vec::new();
    for i in 0..mesh.vertex_count_without_skirts {
        let uv = encoding.decode_texture_coordinates(&mesh.vertices, i)?;
        let height = encoding.decode_height(&mesh.vertices, i)?;
        let mut u = clamp(to_int32(uv.x * 32767.0) as f64, 0.0, 32767.0);
        let mut v = clamp(to_int32(uv.y * 32767.0) as f64, 0.0, 32767.0);
        let h = clamp(
            to_int32((height - p.minimum_height) / (p.maximum_height - p.minimum_height) * 32767.0) as f64,
            0.0,
            32767.0,
        );
        if u < 20.0 {
            u = 0.0;
        }
        if v < 20.0 {
            v = 0.0;
        }
        if 32767.0 - u < 20.0 {
            u = 32767.0;
        }
        if 32767.0 - v < 20.0 {
            v = 32767.0;
        }
        parent.uvh.push([u, v, h]);
        let normal =
            if encoding.has_vertex_normals { Some(encoding.get_oct_encoded_normal(&mesh.vertices, i)?) } else { None };
        if let Some(normal) = normal {
            parent.normals.as_mut()?.push(normal);
        }
        if (if p.is_east_child { u >= 16383.0 } else { u <= 16383.0 })
            && (if p.is_north_child { v >= 16383.0 } else { v <= 16383.0 })
        {
            map.insert(VertexKey::Indexed(i), u32::try_from(values.len()).ok()?);
            values.push([u, v, h]);
            if let Some(normal) = normal {
                normals.push(normal);
            }
        }
    }
    let count = mesh.index_count_without_skirts.unwrap_or(mesh.indices.len());
    if count > mesh.indices.len() || !count.is_multiple_of(3) {
        return None;
    }
    for start in (0..count).step_by(3) {
        let triangle: Vec<_> = (0..3)
            .map(|j| mesh.indices.get(start + j).map(|index| Vertex::Indexed(index as usize)))
            .collect::<Option<_>>()?;
        if triangle.iter().any(|v| match v {
            Vertex::Indexed(i) => *i >= parent.uvh.len(),
            _ => false,
        }) {
            return None;
        }
        let a = triangle[0].values(&parent);
        let b = triangle[1].values(&parent);
        let c = triangle[2].values(&parent);
        let result = clip(16383.0, p.is_east_child, a[0], b[0], c[0]);
        let polygon = clipped_vertices(&result, &triangle)?;
        if polygon.len() < 3 {
            continue;
        }
        let mut triangles = vec![vec![polygon[0].clone(), polygon[1].clone(), polygon[2].clone()]];
        if polygon.len() > 3 {
            triangles.push(vec![polygon[0].clone(), polygon[2].clone(), polygon[3].clone()]);
        }
        for triangle in triangles {
            let a = triangle[0].values(&parent);
            let b = triangle[1].values(&parent);
            let c = triangle[2].values(&parent);
            let result = clip(16383.0, p.is_north_child, a[1], b[1], c[1]);
            add_polygon(
                clipped_vertices(&result, &triangle)?,
                &parent,
                &mut map,
                &mut values,
                &mut normals,
                &mut indices,
            )?;
        }
    }
    let mut edges = QuantizedTerrainEdges::default();
    let mut cartesian_vertices = Vec::new();
    let (mut minimum_height, mut maximum_height) = (f64::MAX, -f64::MAX);
    let r = p.child_rectangle;
    let east = if r.east < r.west { r.east + TWO_PI } else { r.east };
    for (i, value) in values.iter_mut().enumerate() {
        let u = js_round(value[0]);
        value[0] = if u <= if p.is_east_child { 16383.0 } else { 0.0 } {
            edges.west.push(i as u32);
            0.0
        } else if u >= if p.is_east_child { 32767.0 } else { 16383.0 } {
            edges.east.push(i as u32);
            32767.0
        } else {
            u * 2.0 + if p.is_east_child { -32767.0 } else { 0.0 }
        };
        let v = js_round(value[1]);
        value[1] = if v <= if p.is_north_child { 16383.0 } else { 0.0 } {
            edges.south.push(i as u32);
            0.0
        } else if v >= if p.is_north_child { 32767.0 } else { 16383.0 } {
            edges.north.push(i as u32);
            32767.0
        } else {
            v * 2.0 + if p.is_north_child { -32767.0 } else { 0.0 }
        };
        let height = lerp(p.minimum_height, p.maximum_height, value[2] / 32767.0);
        value[2] = height;
        if height < minimum_height {
            minimum_height = height;
        }
        if height > maximum_height {
            maximum_height = height;
        }
        let position = p.ellipsoid.cartographic_to_cartesian(&Cartographic::new(
            lerp(r.west, east, value[0] / 32767.0),
            lerp(r.south, r.north, value[1] / 32767.0),
            height,
        ));
        cartesian_vertices.extend([position.x, position.y, position.z]);
    }
    let bounding_sphere = BoundingSphere::from_vertices(&cartesian_vertices, Cartesian3::ZERO, 3);
    let oriented_bounding_box = OrientedBoundingBox::from_rectangle(&r, minimum_height, maximum_height, &p.ellipsoid)?;
    let horizon_occlusion_point = EllipsoidalOccluder::new(&p.ellipsoid, None)
        .compute_horizon_culling_point_from_vertices_possibly_under_ellipsoid(
            bounding_sphere.center,
            &cartesian_vertices,
            3,
            bounding_sphere.center,
            Some(minimum_height),
        );
    let mut vertices = Vec::with_capacity(values.len() * 3);
    vertices.extend(values.iter().map(|v| v[0] as u16));
    vertices.extend(values.iter().map(|v| v[1] as u16));
    vertices
        .extend(values.iter().map(|v| (32767.0 * (v[2] - minimum_height) / (maximum_height - minimum_height)) as u16));
    Some(QuantizedTerrainUpsampleResult {
        vertices,
        indices: IndexDatatype::create_typed_array(values.len(), &indices),
        encoded_normals: encoding
            .has_vertex_normals
            .then(|| normals.iter().flat_map(|v| [v.x as u8, v.y as u8]).collect()),
        minimum_height,
        maximum_height,
        edges,
        bounding_sphere,
        oriented_bounding_box,
        horizon_occlusion_point,
    })
}
