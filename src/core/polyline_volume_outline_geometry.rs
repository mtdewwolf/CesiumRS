//! CesiumJS `PolylineVolumeOutlineGeometry`: the outline of a 2D shape
//! swept along a polyline.

use super::bounding_sphere::BoundingSphere;
use super::cartesian2::Cartesian2;
use super::cartesian3::Cartesian3;
use super::corner_type::CornerType;
use super::ellipsoid::Ellipsoid;
use super::geometry::{Geometry, GeometryAttribute, GeometryAttributes};
use super::index_datatype::IndexDatatype;
use super::math::RADIANS_PER_DEGREE;
use super::polyline_volume_geometry::{corner_type_from, pack_path, prepare, unpack_path};
use super::polyline_volume_geometry_library::compute_positions;
use super::primitive_type::PrimitiveType;

#[derive(Clone, Debug, PartialEq)]
pub struct PolylineVolumeOutlineGeometry {
    pub positions: Vec<Cartesian3>,
    pub shape: Vec<Cartesian2>,
    pub ellipsoid: Ellipsoid,
    pub corner_type: CornerType,
    pub granularity: f64,
    pub packed_length: usize,
}

impl PolylineVolumeOutlineGeometry {
    /// The corner type defaults to rounded, the granularity to one degree,
    /// and the ellipsoid to WGS84 in Cesium.
    pub fn new(
        polyline_positions: Vec<Cartesian3>,
        shape_positions: Vec<Cartesian2>,
        ellipsoid: Option<Ellipsoid>,
        corner_type: Option<CornerType>,
        granularity: Option<f64>,
    ) -> Self {
        let n = 1 + polyline_positions.len() * 3 + 1 + shape_positions.len() * 2;
        Self {
            positions: polyline_positions,
            shape: shape_positions,
            ellipsoid: ellipsoid.unwrap_or(Ellipsoid::WGS84),
            corner_type: corner_type.unwrap_or(CornerType::Rounded),
            granularity: granularity.unwrap_or(RADIANS_PER_DEGREE),
            packed_length: n + Ellipsoid::PACKED_LENGTH + 2,
        }
    }

    pub fn pack(&self, array: &mut [f64], i: usize) {
        let mut i = pack_path(&self.positions, &self.shape, array, i);
        self.ellipsoid.pack(array, i);
        i += Ellipsoid::PACKED_LENGTH;
        array[i] = self.corner_type as u32 as f64;
        array[i + 1] = self.granularity;
    }

    pub fn unpack(array: &[f64], i: usize) -> Self {
        let (positions, shape, i) = unpack_path(array, i);
        let ellipsoid = Ellipsoid::unpack(array, i);
        let i = i + Ellipsoid::PACKED_LENGTH;
        Self::new(positions, shape, Some(ellipsoid), Some(corner_type_from(array[i])), Some(array[i + 1]))
    }

    /// The outline; `None` for fewer than two distinct positions or three
    /// shape points.
    pub fn create_geometry(&self) -> Option<Geometry> {
        let (mut clean, shape, br) = prepare(&self.positions, &self.shape)?;
        let positions = compute_positions(&mut clean, &shape, &br, &self.ellipsoid, self.granularity, self.corner_type, false)?;
        let shape_length = shape.len();
        let vertex_count = positions.len() / 3;
        let shape_count = vertex_count as f64 / shape_length as f64;
        let mut indices = vec![0u32; (2.0 * shape_length as f64 * (shape_count + 1.0)) as usize];
        let mut index = 0;
        let mut put = |indices: &mut Vec<u32>, v: f64| {
            if let Some(slot) = indices.get_mut(index) {
                *slot = v as u32;
            }
            index += 1;
        };
        for offset in [0.0, (shape_count - 1.0) * shape_length as f64] {
            for j in 0..shape_length - 1 {
                put(&mut indices, j as f64 + offset);
                put(&mut indices, j as f64 + offset + 1.0);
            }
            put(&mut indices, (shape_length - 1) as f64 + offset);
            put(&mut indices, offset);
        }
        let mut i = 0.0;
        while i < shape_count - 1.0 {
            let first = shape_length as f64 * i;
            let second = first + shape_length as f64;
            for j in 0..shape_length {
                put(&mut indices, j as f64 + first);
                put(&mut indices, j as f64 + second);
            }
            i += 1.0;
        }
        let bounding_sphere = BoundingSphere::from_vertices(&positions, Cartesian3::ZERO, 3);
        let attributes = GeometryAttributes { position: Some(GeometryAttribute::doubles(3, positions)), ..Default::default() };
        let mut geometry = Geometry::new(attributes, Some(IndexDatatype::create_typed_array(vertex_count, &indices)), PrimitiveType::Lines);
        geometry.bounding_sphere = Some(bounding_sphere);
        Some(geometry)
    }
}
