//! Native worker calculation for createVerticesFromQuantizedTerrainMesh.
use super::axis_aligned_bounding_box::AxisAlignedBoundingBox;
use super::cartesian2::Cartesian2;
use super::cartesian3::Cartesian3;
use super::cartographic::Cartographic;
use super::ellipsoid::Ellipsoid;
use super::ellipsoidal_occluder::EllipsoidalOccluder;
use super::index_datatype::IndexDatatype;
use super::math::{js_max, js_min, lerp, TWO_PI};
use super::rectangle::Rectangle;
use super::terrain_encoding::{TerrainEncoding, TerrainEncodingOptions, TerrainVertex};
use super::terrain_provider::{self, TerrainEdgeIndices};
use super::transforms::Transforms;
use super::typed_array::TypedArray;
use super::web_mercator_projection::WebMercatorProjection;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct QuantizedTerrainEdges {
    pub west: Vec<u32>,
    pub south: Vec<u32>,
    pub east: Vec<u32>,
    pub north: Vec<u32>,
}
impl QuantizedTerrainEdges {
    pub(crate) fn all(&self) -> [&[u32]; 4] {
        [&self.west, &self.south, &self.east, &self.north]
    }
}

#[derive(Clone, Copy)]
pub struct QuantizedTerrainMeshRequest<'a> {
    pub quantized_vertices: &'a [u16],
    pub indices: &'a TypedArray,
    pub encoded_normals: Option<&'a [u8]>,
    pub edges: &'a QuantizedTerrainEdges,
    pub skirt_heights: [f64; 4],
    pub rectangle: Rectangle,
    pub center: Cartesian3,
    pub ellipsoid: Ellipsoid,
    pub minimum_height: f64,
    pub maximum_height: f64,
    pub include_web_mercator_t: bool,
    pub exaggeration: f64,
    pub exaggeration_relative_height: f64,
}
#[derive(Clone, Debug, PartialEq)]
pub struct QuantizedTerrainMeshResult {
    pub vertices: Vec<f32>,
    pub indices: TypedArray,
    pub edges: TerrainEdgeIndices,
    pub encoding: TerrainEncoding,
    pub occludee_point_in_scaled_space: Option<Cartesian3>,
    pub index_count_without_skirts: usize,
}

pub fn create_vertices_from_quantized_terrain_mesh(
    p: QuantizedTerrainMeshRequest<'_>,
) -> Option<QuantizedTerrainMeshResult> {
    if p.quantized_vertices.is_empty() || !p.quantized_vertices.len().is_multiple_of(3) {
        return None;
    }
    IndexDatatype::from_typed_array(p.indices)?;
    let count = p.quantized_vertices.len() / 3;
    if p.encoded_normals.is_some_and(|n| n.len() < count.saturating_mul(2)) {
        return None;
    }
    let edge_count = p.edges.all().iter().try_fold(0usize, |s, e| s.checked_add(e.len()))?;
    if p.edges.all().iter().any(|e| e.iter().any(|&i| i as usize >= count)) {
        return None;
    }
    let from_enu = Transforms::east_north_up_to_fixed_frame(p.center, &p.ellipsoid);
    let to_enu = from_enu.inverse_transformation();
    let r = p.rectangle;
    let south_merc = WebMercatorProjection::geodetic_latitude_to_mercator_angle(r.south);
    let inv_merc_height = 1.0 / (WebMercatorProjection::geodetic_latitude_to_mercator_angle(r.north) - south_merc);
    let mut positions = Vec::with_capacity(count);
    let mut heights = Vec::with_capacity(count);
    let mut uvs = Vec::with_capacity(count);
    let (mut min, mut max) = (
        Cartesian3::new(f64::INFINITY, f64::INFINITY, f64::INFINITY),
        Cartesian3::new(f64::NEG_INFINITY, f64::NEG_INFINITY, f64::NEG_INFINITY),
    );
    let (mut min_lon, mut max_lon, mut min_lat, mut max_lat) =
        (f64::INFINITY, f64::NEG_INFINITY, f64::INFINITY, f64::NEG_INFINITY);
    let mut samples = Vec::with_capacity(count);
    let sample = |c: Cartographic, uv: Cartesian2, index: usize| -> Option<TerrainVertex> {
        let position = p.ellipsoid.cartographic_to_cartesian(&c);
        let mut vertex = TerrainVertex::new(position, uv, c.height);
        if let Some(normals) = p.encoded_normals {
            vertex.normal_to_pack = Some(Cartesian2::new(normals[index * 2] as f64, normals[index * 2 + 1] as f64));
        }
        if p.include_web_mercator_t {
            vertex.web_mercator_t = Some(
                (WebMercatorProjection::geodetic_latitude_to_mercator_angle(c.latitude) - south_merc) * inv_merc_height,
            );
        }
        if p.exaggeration != 1.0 {
            vertex.geodetic_surface_normal = Some(p.ellipsoid.geodetic_surface_normal(position)?);
        }
        Some(vertex)
    };
    for i in 0..count {
        let uv =
            Cartesian2::new(p.quantized_vertices[i] as f64 / 32767.0, p.quantized_vertices[i + count] as f64 / 32767.0);
        let height = lerp(p.minimum_height, p.maximum_height, p.quantized_vertices[i + 2 * count] as f64 / 32767.0);
        let c = Cartographic::new(lerp(r.west, r.east, uv.x), lerp(r.south, r.north, uv.y), height);
        min_lon = js_min(c.longitude, min_lon);
        max_lon = js_max(c.longitude, max_lon);
        min_lat = js_min(c.latitude, min_lat);
        max_lat = js_max(c.latitude, max_lat);
        let v = sample(c, uv, i)?;
        let enu = to_enu.multiply_by_point(v.position);
        min = min.minimum_by_component(enu);
        max = max.maximum_by_component(enu);
        positions.push(v.position);
        heights.push(height);
        uvs.push(uv);
        samples.push(v);
    }
    let sort = |indices: &[u32], axis: usize, reverse: bool| {
        let mut result = indices.to_vec();
        result.sort_by(|&a, &b| {
            let a = if axis == 0 { uvs[a as usize].x } else { uvs[a as usize].y };
            let b = if axis == 0 { uvs[b as usize].x } else { uvs[b as usize].y };
            if reverse {
                b.total_cmp(&a)
            } else {
                a.total_cmp(&b)
            }
        });
        result
    };
    let edges = TerrainEdgeIndices {
        west_indices_south_to_north: sort(&p.edges.west, 1, false),
        south_indices_east_to_west: sort(&p.edges.south, 0, true),
        east_indices_north_to_south: sort(&p.edges.east, 1, true),
        north_indices_west_to_east: sort(&p.edges.north, 0, false),
    };
    let ordered = [
        edges.west_indices_south_to_north.as_slice(),
        edges.south_indices_east_to_west.as_slice(),
        edges.east_indices_north_to_south.as_slice(),
        edges.north_indices_west_to_east.as_slice(),
    ];
    let east = if r.east < r.west { r.east + TWO_PI } else { r.east };
    let mut min_height = p.minimum_height;
    for (k, indices) in p.edges.all().iter().enumerate() {
        for &i in *indices {
            let i = i as usize;
            let uv = uvs[i];
            let height = heights[i] - p.skirt_heights[k];
            let position = p.ellipsoid.cartographic_to_cartesian(&Cartographic::new(
                lerp(r.west, east, uv.x),
                lerp(r.south, r.north, uv.y),
                height,
            ));
            let enu = to_enu.multiply_by_point(position);
            min = min.minimum_by_component(enu);
            max = max.maximum_by_component(enu);
            min_height = js_min(min_height, height);
        }
    }
    let encoding = TerrainEncoding::new(TerrainEncodingOptions {
        center: Some(p.center),
        axis_aligned_bounding_box: Some(AxisAlignedBoundingBox::new(min, max, Some(p.center))),
        minimum_height: Some(min_height),
        maximum_height: Some(p.maximum_height),
        from_enu: Some(from_enu),
        has_vertex_normals: p.encoded_normals.is_some(),
        has_web_mercator_t: p.include_web_mercator_t,
        has_geodetic_surface_normals: p.exaggeration != 1.0,
        exaggeration: p.exaggeration,
        exaggeration_relative_height: p.exaggeration_relative_height,
    })?;
    let mut vertices = vec![0.0; count.checked_add(edge_count)?.checked_mul(encoding.stride)?];
    let mut offset = 0;
    for v in &samples {
        offset = encoding.encode(&mut vertices, offset, v)?;
    }
    let lon_offset = (max_lon - min_lon) * 0.0001;
    let lat_offset = (max_lat - min_lat) * 0.0001;
    for (k, indices) in ordered.iter().enumerate() {
        for &i in *indices {
            let i = i as usize;
            let uv = uvs[i];
            let lon = lerp(r.west, east, uv.x)
                + if k == 0 {
                    -lon_offset
                } else if k == 2 {
                    lon_offset
                } else {
                    0.0
                };
            let lat = lerp(r.south, r.north, uv.y)
                + if k == 1 {
                    -lat_offset
                } else if k == 3 {
                    lat_offset
                } else {
                    0.0
                };
            offset = encoding.encode(
                &mut vertices,
                offset,
                &sample(Cartographic::new(lon, lat, heights[i] - p.skirt_heights[k]), uv, i)?,
            )?;
        }
    }
    let index_count = p.indices.len();
    let skirt_count = edge_count.saturating_sub(4).checked_mul(6)?;
    // Empty edges can make Cesium allocate fewer slots than addSkirtIndices
    // writes. Its typed-array writes past the end are ignored.
    let written_skirt_count = p
        .edges
        .all()
        .iter()
        .try_fold(0usize, |total, e| total.checked_add(e.len().saturating_sub(1).checked_mul(6)?))?;
    let mut values = p.indices.to_f64_vec();
    values.resize(index_count.checked_add(skirt_count.max(written_skirt_count))?, 0.0);
    let mut indices =
        IndexDatatype::create_typed_array(count + edge_count, &values.iter().map(|&v| v as u32).collect::<Vec<_>>());
    terrain_provider::add_skirt_indices(&edges, u32::try_from(count).ok()?, &mut indices, index_count)?;
    match &mut indices {
        TypedArray::Uint16(v) => v.truncate(index_count + skirt_count),
        TypedArray::Uint32(v) => v.truncate(index_count + skirt_count),
        _ => unreachable!(),
    }
    let occludee_point_in_scaled_space = if p.minimum_height < 0.0 {
        EllipsoidalOccluder::new(&p.ellipsoid, None).compute_horizon_culling_point_possibly_under_ellipsoid(
            p.center,
            &positions,
            Some(p.minimum_height),
        )
    } else {
        None
    };
    Some(QuantizedTerrainMeshResult {
        vertices,
        indices,
        edges,
        encoding,
        occludee_point_in_scaled_space,
        index_count_without_skirts: index_count,
    })
}
