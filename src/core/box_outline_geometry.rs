//! CesiumJS `BoxOutlineGeometry`: the twelve edges of a box.

use super::axis_aligned_bounding_box::AxisAlignedBoundingBox;
use super::bounding_sphere::BoundingSphere;
use super::box_geometry::CORNER_POSITIONS;
use super::cartesian3::Cartesian3;
use super::geometry::{Geometry, GeometryAttribute, GeometryAttributes, GeometryOffsetAttribute};
use super::primitive_type::PrimitiveType;
use super::typed_array::TypedArray;

const INDICES: [u16; 24] = [4, 5, 5, 6, 6, 7, 7, 4, 0, 1, 1, 2, 2, 3, 3, 0, 0, 4, 1, 5, 2, 6, 3, 7];

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct BoxOutlineGeometry {
    pub minimum: Cartesian3,
    pub maximum: Cartesian3,
    pub offset_attribute: Option<GeometryOffsetAttribute>,
}

impl BoxOutlineGeometry {
    pub const PACKED_LENGTH: usize = 2 * 3 + 1;

    /// `None` for [`GeometryOffsetAttribute::Top`], which a box does not support.
    pub fn new(
        minimum: Cartesian3,
        maximum: Cartesian3,
        offset_attribute: Option<GeometryOffsetAttribute>,
    ) -> Option<Self> {
        if offset_attribute == Some(GeometryOffsetAttribute::Top) {
            return None;
        }
        Some(Self { minimum, maximum, offset_attribute })
    }

    pub fn from_dimensions(dimensions: Cartesian3, offset_attribute: Option<GeometryOffsetAttribute>) -> Option<Self> {
        let corner = dimensions.multiply_by_scalar(0.5);
        Self::new(corner.negate(), corner, offset_attribute)
    }

    pub fn from_axis_aligned_bounding_box(bounding_box: &AxisAlignedBoundingBox) -> Self {
        Self { minimum: bounding_box.minimum, maximum: bounding_box.maximum, offset_attribute: None }
    }

    pub fn pack(&self, array: &mut [f64], starting_index: usize) {
        self.minimum.pack(array, starting_index);
        self.maximum.pack(array, starting_index + 3);
        array[starting_index + 6] = GeometryOffsetAttribute::pack(self.offset_attribute);
    }

    pub fn unpack(array: &[f64], starting_index: usize) -> Self {
        Self {
            minimum: Cartesian3::unpack(array, starting_index),
            maximum: Cartesian3::unpack(array, starting_index + 3),
            offset_attribute: GeometryOffsetAttribute::unpack(array[starting_index + 6]),
        }
    }

    /// The edges; `None` when the minimum and maximum coincide.
    pub fn create_geometry(&self) -> Option<Geometry> {
        let (min, max) = (self.minimum, self.maximum);
        if min == max {
            return None;
        }
        let corners = [min, max];
        let positions: Vec<f64> =
            CORNER_POSITIONS.iter().map(|&(c, axis)| [corners[c].x, corners[c].y, corners[c].z][axis]).collect();
        let mut attributes = GeometryAttributes::default();
        let vertex_count = positions.len() / 3;
        attributes.position = Some(GeometryAttribute::doubles(3, positions));
        let radius = max.subtract(min).magnitude() * 0.5;
        if let Some(offset) = self.offset_attribute {
            attributes.set_other("applyOffset", GeometryAttribute::apply_offset(vertex_count, offset));
        }
        let mut geometry = Geometry::new(attributes, Some(TypedArray::Uint16(INDICES.to_vec())), PrimitiveType::Lines);
        geometry.bounding_sphere = Some(BoundingSphere::new(Cartesian3::ZERO, radius));
        geometry.offset_attribute = self.offset_attribute;
        Some(geometry)
    }
}
