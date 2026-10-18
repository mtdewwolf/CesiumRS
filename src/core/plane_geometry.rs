//! CesiumJS `PlaneGeometry` and `PlaneOutlineGeometry`: a unit square in the
//! xy-plane, centered at the origin.

use super::bounding_sphere::BoundingSphere;
use super::cartesian3::Cartesian3;
use super::geometry::{Geometry, GeometryAttribute, GeometryAttributes};
use super::primitive_type::PrimitiveType;
use super::typed_array::TypedArray;
use super::vertex_format::VertexFormat;

const POSITIONS: [f64; 12] = [-0.5, -0.5, 0.0, 0.5, -0.5, 0.0, 0.5, 0.5, 0.0, -0.5, 0.5, 0.0];

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PlaneGeometry {
    pub vertex_format: VertexFormat,
}

impl PlaneGeometry {
    pub const PACKED_LENGTH: usize = VertexFormat::PACKED_LENGTH;

    pub fn new(vertex_format: VertexFormat) -> Self {
        Self { vertex_format }
    }

    pub fn pack(&self, array: &mut [f64], starting_index: usize) {
        self.vertex_format.pack(array, starting_index);
    }

    pub fn unpack(array: &[f64], starting_index: usize) -> Self {
        Self { vertex_format: VertexFormat::unpack(array, starting_index) }
    }

    /// Without positions in the vertex format, the geometry is empty.
    pub fn create_geometry(&self) -> Geometry {
        let vertex_format = self.vertex_format;
        let mut attributes = GeometryAttributes::default();
        let mut indices = None;
        if vertex_format.position {
            attributes.position = Some(GeometryAttribute::doubles(3, POSITIONS.to_vec()));
            if vertex_format.normal {
                attributes.normal =
                    Some(GeometryAttribute::floats(3, &[0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0]));
            }
            if vertex_format.st {
                attributes.st = Some(GeometryAttribute::floats(2, &[0.0, 0.0, 1.0, 0.0, 1.0, 1.0, 0.0, 1.0]));
            }
            if vertex_format.tangent {
                attributes.tangent =
                    Some(GeometryAttribute::floats(3, &[1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0]));
            }
            if vertex_format.bitangent {
                attributes.bitangent =
                    Some(GeometryAttribute::floats(3, &[0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0, 0.0, 1.0, 0.0]));
            }
            indices = Some(TypedArray::Uint16(vec![0, 1, 2, 0, 2, 3]));
        }
        let mut geometry = Geometry::new(attributes, indices, PrimitiveType::Triangles);
        geometry.bounding_sphere = Some(BoundingSphere::new(Cartesian3::ZERO, 2f64.sqrt()));
        geometry
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PlaneOutlineGeometry;

impl PlaneOutlineGeometry {
    pub const PACKED_LENGTH: usize = 0;

    pub fn create_geometry(&self) -> Geometry {
        let attributes = GeometryAttributes {
            position: Some(GeometryAttribute::doubles(3, POSITIONS.to_vec())),
            ..Default::default()
        };
        let indices = TypedArray::Uint16(vec![0, 1, 1, 2, 2, 3, 3, 0]);
        let mut geometry = Geometry::new(attributes, Some(indices), PrimitiveType::Lines);
        geometry.bounding_sphere = Some(BoundingSphere::new(Cartesian3::ZERO, 2f64.sqrt()));
        geometry
    }
}
