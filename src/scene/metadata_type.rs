//! CesiumJS `MetadataType`: the element type of a 3D Metadata property.

use super::MathType;

string_enum!(MetadataType {
    Scalar = "SCALAR" => "SCALAR",
    Vec2 = "VEC2" => "VEC2",
    Vec3 = "VEC3" => "VEC3",
    Vec4 = "VEC4" => "VEC4",
    Mat2 = "MAT2" => "MAT2",
    Mat3 = "MAT3" => "MAT3",
    Mat4 = "MAT4" => "MAT4",
    Boolean = "BOOLEAN" => "BOOLEAN",
    String = "STRING" => "STRING",
    Enum = "ENUM" => "ENUM",
});

impl MetadataType {
    pub fn is_vector_type(self) -> bool {
        matches!(self, Self::Vec2 | Self::Vec3 | Self::Vec4)
    }

    pub fn is_matrix_type(self) -> bool {
        matches!(self, Self::Mat2 | Self::Mat3 | Self::Mat4)
    }

    pub fn get_component_count(self) -> usize {
        match self {
            Self::Scalar | Self::String | Self::Enum | Self::Boolean => 1,
            Self::Vec2 => 2,
            Self::Vec3 => 3,
            Self::Vec4 | Self::Mat2 => 4,
            Self::Mat3 => 9,
            Self::Mat4 => 16,
        }
    }

    /// The Cesium type of a vector or matrix value; `None` for the others.
    pub fn get_math_type(self) -> Option<MathType> {
        Some(match self {
            Self::Vec2 => MathType::Cartesian2,
            Self::Vec3 => MathType::Cartesian3,
            Self::Vec4 => MathType::Cartesian4,
            Self::Mat2 => MathType::Matrix2,
            Self::Mat3 => MathType::Matrix3,
            Self::Mat4 => MathType::Matrix4,
            Self::Scalar | Self::Boolean | Self::String | Self::Enum => return None,
        })
    }
}
