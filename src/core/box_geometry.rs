//! CesiumJS `BoxGeometry`: a box centered at the origin, with per-face
//! normals, texture coordinates, tangents, and bitangents.

use super::axis_aligned_bounding_box::AxisAlignedBoundingBox;
use super::bounding_sphere::BoundingSphere;
use super::cartesian3::Cartesian3;
use super::geometry::{Geometry, GeometryAttribute, GeometryAttributes, GeometryOffsetAttribute};
use super::primitive_type::PrimitiveType;
use super::typed_array::TypedArray;
use super::vertex_format::VertexFormat;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BoxGeometry {
    pub minimum: Cartesian3,
    pub maximum: Cartesian3,
    pub vertex_format: VertexFormat,
    pub offset_attribute: Option<GeometryOffsetAttribute>,
}

impl BoxGeometry {
    pub const PACKED_LENGTH: usize = 2 * 3 + VertexFormat::PACKED_LENGTH + 1;

    /// `None` for [`GeometryOffsetAttribute::Top`], which a box does not support.
    pub fn new(
        minimum: Cartesian3,
        maximum: Cartesian3,
        vertex_format: VertexFormat,
        offset_attribute: Option<GeometryOffsetAttribute>,
    ) -> Option<Self> {
        if offset_attribute == Some(GeometryOffsetAttribute::Top) {
            return None;
        }
        Some(Self { minimum, maximum, vertex_format, offset_attribute })
    }

    /// A box with the given side lengths, centered at the origin.
    pub fn from_dimensions(
        dimensions: Cartesian3,
        vertex_format: VertexFormat,
        offset_attribute: Option<GeometryOffsetAttribute>,
    ) -> Option<Self> {
        let corner = dimensions.multiply_by_scalar(0.5);
        Self::new(corner.negate(), corner, vertex_format, offset_attribute)
    }

    pub fn from_axis_aligned_bounding_box(bounding_box: &AxisAlignedBoundingBox) -> Self {
        Self {
            minimum: bounding_box.minimum,
            maximum: bounding_box.maximum,
            vertex_format: VertexFormat::DEFAULT,
            offset_attribute: None,
        }
    }

    pub fn pack(&self, array: &mut [f64], starting_index: usize) {
        self.minimum.pack(array, starting_index);
        self.maximum.pack(array, starting_index + 3);
        self.vertex_format.pack(array, starting_index + 6);
        array[starting_index + 6 + VertexFormat::PACKED_LENGTH] = GeometryOffsetAttribute::pack(self.offset_attribute);
    }

    pub fn unpack(array: &[f64], starting_index: usize) -> Self {
        Self {
            minimum: Cartesian3::unpack(array, starting_index),
            maximum: Cartesian3::unpack(array, starting_index + 3),
            vertex_format: VertexFormat::unpack(array, starting_index + 6),
            offset_attribute: GeometryOffsetAttribute::unpack(array[starting_index + 6 + VertexFormat::PACKED_LENGTH]),
        }
    }

    /// The triangles; `None` when the minimum and maximum coincide.
    pub fn create_geometry(&self) -> Option<Geometry> {
        let (min, max) = (self.minimum, self.maximum);
        if min == max {
            return None;
        }
        let vertex_format = self.vertex_format;
        let corners = [min, max];
        let select = |table: &[(usize, usize)]| -> Vec<f64> {
            table.iter().map(|&(c, axis)| [corners[c].x, corners[c].y, corners[c].z][axis]).collect()
        };
        let mut attributes = GeometryAttributes::default();
        let positions;
        let indices;
        if vertex_format.position
            && (vertex_format.st || vertex_format.normal || vertex_format.tangent || vertex_format.bitangent)
        {
            positions = select(&FULL_POSITIONS);
            attributes.position = Some(GeometryAttribute::doubles(3, positions.clone()));
            if vertex_format.normal {
                attributes.normal = Some(GeometryAttribute::floats(3, &NORMALS));
            }
            if vertex_format.st {
                attributes.st = Some(GeometryAttribute::floats(2, &TEXTURE_COORDINATES));
            }
            if vertex_format.tangent {
                attributes.tangent = Some(GeometryAttribute::floats(3, &TANGENTS));
            }
            if vertex_format.bitangent {
                attributes.bitangent = Some(GeometryAttribute::floats(3, &BITANGENTS));
            }
            indices = FULL_INDICES.iter().map(|&i| i as u16).collect();
        } else {
            positions = select(&CORNER_POSITIONS);
            attributes.position = Some(GeometryAttribute::doubles(3, positions.clone()));
            indices = CORNER_INDICES.iter().map(|&i| i as u16).collect();
        }

        let radius = max.subtract(min).magnitude() * 0.5;
        if let Some(offset) = self.offset_attribute {
            attributes.set_other("applyOffset", GeometryAttribute::apply_offset(positions.len() / 3, offset));
        }
        let mut geometry = Geometry::new(attributes, Some(TypedArray::Uint16(indices)), PrimitiveType::Triangles);
        geometry.bounding_sphere = Some(BoundingSphere::new(Cartesian3::ZERO, radius));
        geometry.offset_attribute = self.offset_attribute;
        Some(geometry)
    }

    /// A position-only unit cube.
    pub fn get_unit_box() -> Geometry {
        Self::from_dimensions(Cartesian3::new(1.0, 1.0, 1.0), VertexFormat::POSITION_ONLY, None)
            .and_then(|b| b.create_geometry())
            .expect("the unit box is valid")
    }
}

// Tables extracted from Cesium's BoxGeometry.js. Positions are (min = 0 or
// max = 1, axis) pairs.

const FULL_POSITIONS: [(usize, usize); 72] = [
    (0, 0),
    (0, 1),
    (1, 2),
    (1, 0),
    (0, 1),
    (1, 2),
    (1, 0),
    (1, 1),
    (1, 2),
    (0, 0),
    (1, 1),
    (1, 2),
    (0, 0),
    (0, 1),
    (0, 2),
    (1, 0),
    (0, 1),
    (0, 2),
    (1, 0),
    (1, 1),
    (0, 2),
    (0, 0),
    (1, 1),
    (0, 2),
    (1, 0),
    (0, 1),
    (0, 2),
    (1, 0),
    (1, 1),
    (0, 2),
    (1, 0),
    (1, 1),
    (1, 2),
    (1, 0),
    (0, 1),
    (1, 2),
    (0, 0),
    (0, 1),
    (0, 2),
    (0, 0),
    (1, 1),
    (0, 2),
    (0, 0),
    (1, 1),
    (1, 2),
    (0, 0),
    (0, 1),
    (1, 2),
    (0, 0),
    (1, 1),
    (0, 2),
    (1, 0),
    (1, 1),
    (0, 2),
    (1, 0),
    (1, 1),
    (1, 2),
    (0, 0),
    (1, 1),
    (1, 2),
    (0, 0),
    (0, 1),
    (0, 2),
    (1, 0),
    (0, 1),
    (0, 2),
    (1, 0),
    (0, 1),
    (1, 2),
    (0, 0),
    (0, 1),
    (1, 2),
];
const NORMALS: [f64; 72] = [
    0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, -1.0, 0.0, 0.0, -1.0, 0.0, 0.0, -1.0, 0.0,
    0.0, -1.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, -1.0, 0.0, 0.0, -1.0, 0.0, 0.0, -1.0, 0.0,
    0.0, -1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, -1.0, 0.0, 0.0, -1.0, 0.0,
    0.0, -1.0, 0.0, 0.0, -1.0, 0.0,
];
const TEXTURE_COORDINATES: [f64; 48] = [
    0.0, 0.0, 1.0, 0.0, 1.0, 1.0, 0.0, 1.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 0.0, 0.0, 1.0, 0.0, 1.0, 1.0, 0.0,
    1.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 1.0, 1.0, 0.0, 0.0, 1.0, 0.0, 1.0, 1.0,
    0.0, 1.0,
];
const TANGENTS: [f64; 72] = [
    1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, -1.0, 0.0, 0.0, -1.0, 0.0, 0.0, -1.0, 0.0, 0.0, -1.0,
    0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, -1.0, 0.0, 0.0, -1.0, 0.0, 0.0, -1.0,
    0.0, 0.0, -1.0, 0.0, -1.0, 0.0, 0.0, -1.0, 0.0, 0.0, -1.0, 0.0, 0.0, -1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0,
    1.0, 0.0, 0.0, 1.0, 0.0, 0.0,
];
const BITANGENTS: [f64; 72] = [
    0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0,
    0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0,
    0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0,
    0.0, 0.0, 1.0,
];
const FULL_INDICES: [u32; 36] = [
    0, 1, 2, 0, 2, 3, 6, 5, 4, 7, 6, 4, 8, 9, 10, 8, 10, 11, 14, 13, 12, 15, 14, 12, 18, 17, 16, 19, 18, 16, 20, 21,
    22, 20, 22, 23,
];
pub(crate) const CORNER_POSITIONS: [(usize, usize); 24] = [
    (0, 0),
    (0, 1),
    (0, 2),
    (1, 0),
    (0, 1),
    (0, 2),
    (1, 0),
    (1, 1),
    (0, 2),
    (0, 0),
    (1, 1),
    (0, 2),
    (0, 0),
    (0, 1),
    (1, 2),
    (1, 0),
    (0, 1),
    (1, 2),
    (1, 0),
    (1, 1),
    (1, 2),
    (0, 0),
    (1, 1),
    (1, 2),
];
const CORNER_INDICES: [u32; 36] =
    [4, 5, 6, 4, 6, 7, 1, 0, 3, 1, 3, 2, 1, 6, 5, 1, 2, 6, 2, 3, 7, 2, 7, 6, 3, 0, 4, 3, 4, 7, 0, 1, 5, 0, 5, 4];
