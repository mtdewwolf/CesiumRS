//! CesiumJS `PolylineVolumeGeometry`: a 2D shape extruded along a polyline,
//! with end caps.

use super::array_remove_duplicates::array_remove_duplicates;
use super::bounding_rectangle::BoundingRectangle;
use super::bounding_sphere::BoundingSphere;
use super::cartesian2::Cartesian2;
use super::cartesian3::Cartesian3;
use super::corner_type::CornerType;
use super::ellipsoid::Ellipsoid;
use super::geometry::{Geometry, GeometryAttribute, GeometryAttributes};
use super::geometry_pipeline::{compute_normal, compute_tangent_and_bitangent};
use super::index_datatype::IndexDatatype;
use super::math::RADIANS_PER_DEGREE;
use super::polygon_pipeline::{compute_winding_order_2d, triangulate};
use super::polyline_volume_geometry_library::{compute_positions, remove_duplicates_from_shape};
use super::primitive_type::PrimitiveType;
use super::vertex_format::VertexFormat;
use super::winding_order::WindingOrder;

/// Construction options, with Cesium's defaults.
#[derive(Clone, Debug, PartialEq)]
pub struct PolylineVolumeGeometryOptions {
    pub polyline_positions: Vec<Cartesian3>,
    pub shape_positions: Vec<Cartesian2>,
    pub ellipsoid: Ellipsoid,
    pub corner_type: CornerType,
    pub vertex_format: VertexFormat,
    pub granularity: f64,
}

impl PolylineVolumeGeometryOptions {
    pub fn new(polyline_positions: Vec<Cartesian3>, shape_positions: Vec<Cartesian2>) -> Self {
        Self {
            polyline_positions,
            shape_positions,
            ellipsoid: Ellipsoid::WGS84,
            corner_type: CornerType::Rounded,
            vertex_format: VertexFormat::DEFAULT,
            granularity: RADIANS_PER_DEGREE,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct PolylineVolumeGeometry {
    pub positions: Vec<Cartesian3>,
    pub shape: Vec<Cartesian2>,
    pub ellipsoid: Ellipsoid,
    pub corner_type: CornerType,
    pub vertex_format: VertexFormat,
    pub granularity: f64,
    pub packed_length: usize,
}

pub(crate) fn corner_type_from(value: f64) -> CornerType {
    match value as i32 {
        1 => CornerType::Mitered,
        2 => CornerType::Beveled,
        _ => CornerType::Rounded,
    }
}

/// Packs positions then shape; returns the next index.
pub(crate) fn pack_path(positions: &[Cartesian3], shape: &[Cartesian2], array: &mut [f64], mut i: usize) -> usize {
    array[i] = positions.len() as f64;
    i += 1;
    for p in positions {
        p.pack(array, i);
        i += 3;
    }
    array[i] = shape.len() as f64;
    i += 1;
    for p in shape {
        p.pack(array, i);
        i += 2;
    }
    i
}

pub(crate) fn unpack_path(array: &[f64], mut i: usize) -> (Vec<Cartesian3>, Vec<Cartesian2>, usize) {
    let n = array[i] as usize;
    i += 1;
    let positions = (0..n).map(|k| Cartesian3::unpack(array, i + k * 3)).collect();
    i += n * 3;
    let m = array[i] as usize;
    i += 1;
    let shape = (0..m).map(|k| Cartesian2::unpack(array, i + k * 2)).collect();
    (positions, shape, i + m * 2)
}

/// Clean positions, the counter-clockwise shape and its bounds; `None` for
/// fewer than two positions or three shape points.
pub(crate) fn prepare(positions: &[Cartesian3], shape: &[Cartesian2]) -> Option<(Vec<Cartesian3>, Vec<Cartesian2>, BoundingRectangle)> {
    let clean = array_remove_duplicates(positions, |a, b, e| a.equals_epsilon(*b, e), false, None);
    let mut shape_2d = remove_duplicates_from_shape(shape);
    if clean.len() < 2 || shape_2d.len() < 3 {
        return None;
    }
    if compute_winding_order_2d(&shape_2d) == WindingOrder::Clockwise {
        shape_2d.reverse();
    }
    let br = BoundingRectangle::from_points(&shape_2d);
    Some((clean, shape_2d, br))
}

fn put(values: &mut [u32], i: &mut usize, v: f64) {
    if let Some(slot) = values.get_mut(*i) {
        *slot = v as u32;
    }
    *i += 1;
}

fn compute_attributes(positions: Vec<f64>, shape: &[Cartesian2], br: &BoundingRectangle, vf: VertexFormat) -> Option<Geometry> {
    let shape_length = shape.len() as f64;
    let vertex_count = positions.len() as f64 / 3.0;
    let length = (vertex_count - shape_length * 2.0) / (shape_length * 2.0);
    let first_end_indices = triangulate(shape, &[]);
    let indices_count = (length - 1.0) * shape_length * 6.0 + first_end_indices.len() as f64 * 2.0;
    let mut indices = vec![0u32; if indices_count > 0.0 { indices_count as usize } else { 0 }];
    let offset = shape_length * 2.0;
    let mut index = 0;
    let mut i = 0.0;
    while i < length - 1.0 {
        let mut j = 0.0;
        while j < shape_length - 1.0 {
            let ll = j * 2.0 + i * shape_length * 2.0;
            let (lr, ul) = (ll + offset, ll + 1.0);
            let ur = ul + offset;
            for v in [ul, ll, ur, ur, ll, lr] {
                put(&mut indices, &mut index, v);
            }
            j += 1.0;
        }
        let ll = shape_length * 2.0 - 2.0 + i * shape_length * 2.0;
        let ul = ll + 1.0;
        let (ur, lr) = (ul + offset, ll + offset);
        for v in [ul, ll, ur, ur, ll, lr] {
            put(&mut indices, &mut index, v);
        }
        i += 1.0;
    }
    let mut attributes = GeometryAttributes::default();
    if vf.st || vf.tangent || vf.bitangent {
        let mut st = vec![0f32; (vertex_count * 2.0) as usize];
        let mut k = 0;
        let mut push = |s: f64, t: f64| {
            for v in [s, t] {
                if let Some(slot) = st.get_mut(k) {
                    *slot = v as f32;
                }
                k += 1;
            }
        };
        let length_st = 1.0 / (length - 1.0);
        let height_st = 1.0 / br.height;
        let height_offset = br.height / 2.0;
        let t_of = |p: &Cartesian2| height_st * (p.y + height_offset);
        let mut i = 0.0;
        while i < length {
            let s = i * length_st;
            push(s, t_of(&shape[0]));
            for p in &shape[1..] {
                push(s, t_of(p));
                push(s, t_of(p));
            }
            push(s, t_of(&shape[0]));
            i += 1.0;
        }
        for p in shape {
            push(0.0, t_of(p));
        }
        for p in shape {
            push((length - 1.0) * length_st, t_of(p));
        }
        attributes.st = Some(GeometryAttribute::new(
            super::component_datatype::ComponentDatatype::Float,
            2,
            super::typed_array::TypedArray::Float32(st),
        ));
    }
    let end_offset = vertex_count - shape_length * 2.0;
    for t in first_end_indices.as_chunks::<3>().0 {
        let [v0, v1, v2] = [t[0] as f64 + end_offset, t[1] as f64 + end_offset, t[2] as f64 + end_offset];
        for v in [v0, v1, v2, v2 + shape_length, v1 + shape_length, v0 + shape_length] {
            put(&mut indices, &mut index, v);
        }
    }
    let bounding_sphere = BoundingSphere::from_vertices(&positions, Cartesian3::ZERO, 3);
    if vf.position {
        attributes.position = Some(GeometryAttribute::doubles(3, positions));
    }
    let mut geometry =
        Geometry::new(attributes, Some(IndexDatatype::create_typed_array(vertex_count as usize, &indices)), PrimitiveType::Triangles);
    geometry.bounding_sphere = Some(bounding_sphere);
    if vf.normal {
        compute_normal(&mut geometry)?;
    }
    if vf.tangent || vf.bitangent {
        // Cesium catches the failure (no normals) and warns.
        let _ = compute_tangent_and_bitangent(&mut geometry);
        if !vf.tangent {
            geometry.attributes.tangent = None;
        }
        if !vf.bitangent {
            geometry.attributes.bitangent = None;
        }
        if !vf.st {
            geometry.attributes.st = None;
        }
    }
    Some(geometry)
}

impl PolylineVolumeGeometry {
    pub fn new(options: PolylineVolumeGeometryOptions) -> Self {
        let n = 1 + options.polyline_positions.len() * 3 + 1 + options.shape_positions.len() * 2;
        Self {
            positions: options.polyline_positions,
            shape: options.shape_positions,
            ellipsoid: options.ellipsoid,
            corner_type: options.corner_type,
            vertex_format: options.vertex_format,
            granularity: options.granularity,
            packed_length: n + Ellipsoid::PACKED_LENGTH + VertexFormat::PACKED_LENGTH + 2,
        }
    }

    pub fn pack(&self, array: &mut [f64], i: usize) {
        let mut i = pack_path(&self.positions, &self.shape, array, i);
        self.ellipsoid.pack(array, i);
        i += Ellipsoid::PACKED_LENGTH;
        self.vertex_format.pack(array, i);
        i += VertexFormat::PACKED_LENGTH;
        array[i] = self.corner_type as u32 as f64;
        array[i + 1] = self.granularity;
    }

    pub fn unpack(array: &[f64], i: usize) -> Self {
        let (positions, shape, mut i) = unpack_path(array, i);
        let ellipsoid = Ellipsoid::unpack(array, i);
        i += Ellipsoid::PACKED_LENGTH;
        let vertex_format = VertexFormat::unpack(array, i);
        i += VertexFormat::PACKED_LENGTH;
        Self::new(PolylineVolumeGeometryOptions {
            polyline_positions: positions,
            shape_positions: shape,
            ellipsoid,
            corner_type: corner_type_from(array[i]),
            vertex_format,
            granularity: array[i + 1],
        })
    }

    /// The volume; `None` for fewer than two distinct positions or three
    /// shape points, and without positions when normals are asked for
    /// (Cesium throws).
    pub fn create_geometry(&self) -> Option<Geometry> {
        let (mut clean, shape, br) = prepare(&self.positions, &self.shape)?;
        let positions = compute_positions(&mut clean, &shape, &br, &self.ellipsoid, self.granularity, self.corner_type, true)?;
        compute_attributes(positions, &shape, &br, self.vertex_format)
    }
}
