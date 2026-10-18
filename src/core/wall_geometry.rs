//! CesiumJS `WallGeometry`: a wall between minimum and maximum heights
//! along a polyline, with texture coordinates and normal frames.

use super::bounding_sphere::BoundingSphere;
use super::cartesian3::Cartesian3;
use super::ellipsoid::Ellipsoid;
use super::geometry::{Geometry, GeometryAttribute, GeometryAttributes};
use super::index_datatype::IndexDatatype;
use super::math::{EPSILON10, RADIANS_PER_DEGREE};
use super::primitive_type::PrimitiveType;
use super::vertex_format::VertexFormat;
use super::wall_geometry_library::compute_positions;

/// Construction options, with Cesium's defaults.
#[derive(Clone, Debug, PartialEq)]
pub struct WallGeometryOptions {
    pub positions: Vec<Cartesian3>,
    pub maximum_heights: Option<Vec<f64>>,
    pub minimum_heights: Option<Vec<f64>>,
    pub vertex_format: VertexFormat,
    pub granularity: f64,
    pub ellipsoid: Ellipsoid,
}

impl WallGeometryOptions {
    pub fn new(positions: Vec<Cartesian3>) -> Self {
        Self {
            positions,
            maximum_heights: None,
            minimum_heights: None,
            vertex_format: VertexFormat::DEFAULT,
            granularity: RADIANS_PER_DEGREE,
            ellipsoid: Ellipsoid::WGS84,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct WallGeometry {
    pub positions: Vec<Cartesian3>,
    pub maximum_heights: Option<Vec<f64>>,
    pub minimum_heights: Option<Vec<f64>>,
    pub vertex_format: VertexFormat,
    pub granularity: f64,
    pub ellipsoid: Ellipsoid,
    pub packed_length: usize,
}

pub(crate) fn pack_wall(
    positions: &[Cartesian3],
    minimum_heights: Option<&[f64]>,
    maximum_heights: Option<&[f64]>,
    array: &mut [f64],
    mut i: usize,
) -> usize {
    array[i] = positions.len() as f64;
    i += 1;
    for p in positions {
        p.pack(array, i);
        i += 3;
    }
    for heights in [minimum_heights, maximum_heights] {
        let heights = heights.unwrap_or(&[]);
        array[i] = heights.len() as f64;
        i += 1;
        array[i..i + heights.len()].copy_from_slice(heights);
        i += heights.len();
    }
    i
}

/// Positions, minimum heights, maximum heights, and the next index.
pub(crate) type UnpackedWall = (Vec<Cartesian3>, Option<Vec<f64>>, Option<Vec<f64>>, usize);

/// Positions and the two height lists, as packed.
pub(crate) fn unpack_wall(array: &[f64], mut i: usize) -> UnpackedWall {
    let n = array[i] as usize;
    i += 1;
    let positions = (0..n).map(|k| Cartesian3::unpack(array, i + k * 3)).collect();
    i += n * 3;
    let mut lists = [None, None];
    for list in lists.iter_mut() {
        let len = array[i] as usize;
        i += 1;
        if len > 0 {
            *list = Some(array[i..i + len].to_vec());
        }
        i += len;
    }
    let [minimum, maximum] = lists;
    (positions, minimum, maximum, i)
}

/// Cesium's `fromConstantHeights` height lists.
pub(crate) fn constant_heights(n: usize, min: Option<f64>, max: Option<f64>) -> (Option<Vec<f64>>, Option<Vec<f64>>) {
    (min.map(|m| vec![m; n]), max.map(|m| vec![m; n]))
}

impl WallGeometry {
    pub fn new(options: WallGeometryOptions) -> Self {
        let mut n = 1 + options.positions.len() * 3 + 2;
        n += options.minimum_heights.as_ref().map_or(0, Vec::len);
        n += options.maximum_heights.as_ref().map_or(0, Vec::len);
        Self {
            positions: options.positions,
            maximum_heights: options.maximum_heights,
            minimum_heights: options.minimum_heights,
            vertex_format: options.vertex_format,
            granularity: options.granularity,
            ellipsoid: options.ellipsoid,
            packed_length: n + Ellipsoid::PACKED_LENGTH + VertexFormat::PACKED_LENGTH + 1,
        }
    }

    /// A wall whose heights are the same at every position.
    pub fn from_constant_heights(
        positions: Vec<Cartesian3>,
        minimum_height: Option<f64>,
        maximum_height: Option<f64>,
        ellipsoid: Ellipsoid,
        vertex_format: VertexFormat,
    ) -> Self {
        let (minimum_heights, maximum_heights) = constant_heights(positions.len(), minimum_height, maximum_height);
        Self::new(WallGeometryOptions { positions, maximum_heights, minimum_heights, vertex_format, granularity: RADIANS_PER_DEGREE, ellipsoid })
    }

    pub fn pack(&self, array: &mut [f64], i: usize) {
        let mut i = pack_wall(&self.positions, self.minimum_heights.as_deref(), self.maximum_heights.as_deref(), array, i);
        self.ellipsoid.pack(array, i);
        i += Ellipsoid::PACKED_LENGTH;
        self.vertex_format.pack(array, i);
        i += VertexFormat::PACKED_LENGTH;
        array[i] = self.granularity;
    }

    pub fn unpack(array: &[f64], i: usize) -> Self {
        let (positions, minimum_heights, maximum_heights, mut i) = unpack_wall(array, i);
        let ellipsoid = Ellipsoid::unpack(array, i);
        i += Ellipsoid::PACKED_LENGTH;
        let vertex_format = VertexFormat::unpack(array, i);
        i += VertexFormat::PACKED_LENGTH;
        Self::new(WallGeometryOptions { positions, maximum_heights, minimum_heights, vertex_format, granularity: array[i], ellipsoid })
    }

    /// The wall; `None` for a wall with no area, and without positions in
    /// the vertex format (Cesium throws).
    pub fn create_geometry(&self) -> Option<Geometry> {
        let vf = self.vertex_format;
        let ellipsoid = &self.ellipsoid;
        let pos = compute_positions(
            ellipsoid,
            &self.positions,
            self.maximum_heights.as_deref(),
            self.minimum_heights.as_deref(),
            self.granularity,
            true,
        )?;
        if !vf.position {
            return None;
        }
        let (top, bottom, num_corners) = (&pos.top_positions, &pos.bottom_positions, pos.num_corners);
        let length = top.len() / 3;
        let mut positions = Vec::with_capacity(length * 6);
        let (mut normals, mut tangents, mut bitangents, mut st) = (Vec::new(), Vec::new(), Vec::new(), Vec::new());
        let (mut normal, mut tangent, mut bitangent) = (Cartesian3::ZERO, Cartesian3::ZERO, Cartesian3::ZERO);
        let mut recompute_normal = true;
        let mut s = 0.0;
        let ds = 1.0 / (length as f64 - num_corners as f64 - 1.0);
        for i in 0..length {
            let mut top_position = Cartesian3::from_array(top, i * 3);
            let bottom_position = Cartesian3::from_array(bottom, i * 3);
            positions.extend([bottom_position.x, bottom_position.y, bottom_position.z, top_position.x, top_position.y, top_position.z]);
            if vf.st {
                st.extend([s, 0.0, s, 1.0]);
            }
            if vf.normal || vf.tangent || vf.bitangent {
                let next_top = if i + 1 < length { Cartesian3::from_array(top, i * 3 + 3) } else { Cartesian3::ZERO };
                let ground = top_position.subtract(ellipsoid.geodetic_surface_normal(top_position)?);
                if recompute_normal {
                    let scaled_next = next_top.subtract(top_position);
                    let scaled_ground = ground.subtract(top_position);
                    normal = scaled_ground.cross(scaled_next).normalize();
                    recompute_normal = false;
                    // Cesium computes `scaledGroundPosition` into the scratch
                    // holding the top position, which the tangent then uses.
                    top_position = scaled_ground;
                }
                if top_position.equals_epsilon(next_top, EPSILON10) {
                    recompute_normal = true;
                } else {
                    s += ds;
                    if vf.tangent {
                        tangent = next_top.subtract(top_position).normalize();
                    }
                    if vf.bitangent {
                        bitangent = normal.cross(tangent).normalize();
                    }
                }
                for _ in 0..2 {
                    normals.extend([normal.x, normal.y, normal.z]);
                    tangents.extend([tangent.x, tangent.y, tangent.z]);
                    bitangents.extend([bitangent.x, bitangent.y, bitangent.z]);
                }
            }
        }
        let num_vertices = positions.len() / 3;
        let mut indices = vec![0u32; (positions.len()).saturating_sub(6 * (num_corners + 1))];
        let mut edge_index = 0;
        let mut i = 0;
        while i + 2 < num_vertices {
            let (ll, lr) = (i, i + 2);
            let pl = Cartesian3::from_array(&positions, ll * 3);
            let pr = Cartesian3::from_array(&positions, lr * 3);
            if !pl.equals_epsilon(pr, EPSILON10) {
                let (ul, ur) = (i + 1, i + 3);
                for v in [ul, ll, ur, ur, ll, lr] {
                    if let Some(slot) = indices.get_mut(edge_index) {
                        *slot = v as u32;
                    }
                    edge_index += 1;
                }
            }
            i += 2;
        }
        let bounding_sphere = BoundingSphere::from_vertices(&positions, Cartesian3::ZERO, 3);
        let mut attributes = GeometryAttributes { position: Some(GeometryAttribute::doubles(3, positions)), ..Default::default() };
        if vf.normal {
            attributes.normal = Some(GeometryAttribute::floats(3, &normals));
        }
        if vf.tangent {
            attributes.tangent = Some(GeometryAttribute::floats(3, &tangents));
        }
        if vf.bitangent {
            attributes.bitangent = Some(GeometryAttribute::floats(3, &bitangents));
        }
        if vf.st {
            attributes.st = Some(GeometryAttribute::floats(2, &st));
        }
        let mut geometry =
            Geometry::new(attributes, Some(IndexDatatype::create_typed_array(num_vertices, &indices)), PrimitiveType::Triangles);
        geometry.bounding_sphere = Some(bounding_sphere);
        Some(geometry)
    }
}
