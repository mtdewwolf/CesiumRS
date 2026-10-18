//! CesiumJS `WallOutlineGeometry`: the edges of a wall.

use super::bounding_sphere::BoundingSphere;
use super::cartesian3::Cartesian3;
use super::ellipsoid::Ellipsoid;
use super::geometry::{Geometry, GeometryAttribute, GeometryAttributes};
use super::index_datatype::IndexDatatype;
use super::math::{EPSILON10, RADIANS_PER_DEGREE};
use super::primitive_type::PrimitiveType;
use super::wall_geometry::{constant_heights, pack_wall, unpack_wall};
use super::wall_geometry_library::compute_positions;

#[derive(Clone, Debug, PartialEq)]
pub struct WallOutlineGeometry {
    pub positions: Vec<Cartesian3>,
    pub maximum_heights: Option<Vec<f64>>,
    pub minimum_heights: Option<Vec<f64>>,
    pub granularity: f64,
    pub ellipsoid: Ellipsoid,
    pub packed_length: usize,
}

impl WallOutlineGeometry {
    /// `granularity` defaults to one degree and `ellipsoid` to WGS84 in Cesium.
    pub fn new(
        positions: Vec<Cartesian3>,
        maximum_heights: Option<Vec<f64>>,
        minimum_heights: Option<Vec<f64>>,
        granularity: Option<f64>,
        ellipsoid: Option<Ellipsoid>,
    ) -> Self {
        let mut n = 1 + positions.len() * 3 + 2;
        n += minimum_heights.as_ref().map_or(0, Vec::len);
        n += maximum_heights.as_ref().map_or(0, Vec::len);
        Self {
            positions,
            maximum_heights,
            minimum_heights,
            granularity: granularity.unwrap_or(RADIANS_PER_DEGREE),
            ellipsoid: ellipsoid.unwrap_or(Ellipsoid::WGS84),
            packed_length: n + Ellipsoid::PACKED_LENGTH + 1,
        }
    }

    pub fn from_constant_heights(positions: Vec<Cartesian3>, minimum_height: Option<f64>, maximum_height: Option<f64>, ellipsoid: Ellipsoid) -> Self {
        let (minimum_heights, maximum_heights) = constant_heights(positions.len(), minimum_height, maximum_height);
        Self::new(positions, maximum_heights, minimum_heights, None, Some(ellipsoid))
    }

    pub fn pack(&self, array: &mut [f64], i: usize) {
        let mut i = pack_wall(&self.positions, self.minimum_heights.as_deref(), self.maximum_heights.as_deref(), array, i);
        self.ellipsoid.pack(array, i);
        i += Ellipsoid::PACKED_LENGTH;
        array[i] = self.granularity;
    }

    pub fn unpack(array: &[f64], i: usize) -> Self {
        let (positions, minimum_heights, maximum_heights, i) = unpack_wall(array, i);
        let ellipsoid = Ellipsoid::unpack(array, i);
        Self::new(positions, maximum_heights, minimum_heights, Some(array[i + Ellipsoid::PACKED_LENGTH]), Some(ellipsoid))
    }

    /// The outline; `None` for a wall with no area.
    pub fn create_geometry(&self) -> Option<Geometry> {
        let pos = compute_positions(
            &self.ellipsoid,
            &self.positions,
            self.maximum_heights.as_deref(),
            self.minimum_heights.as_deref(),
            self.granularity,
            false,
        )?;
        let length = pos.top_positions.len() / 3;
        let mut positions = Vec::with_capacity(length * 6);
        for i in 0..length {
            let top = Cartesian3::from_array(&pos.top_positions, i * 3);
            let bottom = Cartesian3::from_array(&pos.bottom_positions, i * 3);
            positions.extend([bottom.x, bottom.y, bottom.z, top.x, top.y, top.z]);
        }
        let num_vertices = positions.len() / 3;
        let mut indices = vec![0u32; (3 * num_vertices).saturating_sub(4)];
        let mut edge_index = 0;
        let mut put = |indices: &mut Vec<u32>, v: usize| {
            if let Some(slot) = indices.get_mut(edge_index) {
                *slot = v as u32;
            }
            edge_index += 1;
        };
        let mut i = 0;
        while i + 2 < num_vertices {
            let (ll, lr) = (i, i + 2);
            let pl = Cartesian3::from_array(&positions, ll * 3);
            let pr = Cartesian3::from_array(&positions, lr * 3);
            if !pl.equals_epsilon(pr, EPSILON10) {
                let (ul, ur) = (i + 1, i + 3);
                for v in [ul, ll, ul, ur, ll, lr] {
                    put(&mut indices, v);
                }
            }
            i += 2;
        }
        put(&mut indices, num_vertices.wrapping_sub(2));
        put(&mut indices, num_vertices.wrapping_sub(1));
        let bounding_sphere = BoundingSphere::from_vertices(&positions, Cartesian3::ZERO, 3);
        let attributes = GeometryAttributes { position: Some(GeometryAttribute::doubles(3, positions)), ..Default::default() };
        let mut geometry = Geometry::new(attributes, Some(IndexDatatype::create_typed_array(num_vertices, &indices)), PrimitiveType::Lines);
        geometry.bounding_sphere = Some(bounding_sphere);
        Some(geometry)
    }
}
