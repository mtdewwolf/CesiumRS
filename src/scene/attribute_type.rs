//! CesiumJS `AttributeType`: the shape of a glTF accessor element.

use super::MathType;

string_enum!(AttributeType {
    Scalar = "SCALAR" => "SCALAR",
    Vec2 = "VEC2" => "VEC2",
    Vec3 = "VEC3" => "VEC3",
    Vec4 = "VEC4" => "VEC4",
    Mat2 = "MAT2" => "MAT2",
    Mat3 = "MAT3" => "MAT3",
    Mat4 = "MAT4" => "MAT4",
});

impl AttributeType {
    /// The Cesium type an element reads as: `Number` for scalars.
    pub fn get_math_type(self) -> MathType {
        match self {
            Self::Scalar => MathType::Number,
            Self::Vec2 => MathType::Cartesian2,
            Self::Vec3 => MathType::Cartesian3,
            Self::Vec4 => MathType::Cartesian4,
            Self::Mat2 => MathType::Matrix2,
            Self::Mat3 => MathType::Matrix3,
            Self::Mat4 => MathType::Matrix4,
        }
    }

    pub fn get_number_of_components(self) -> usize {
        match self {
            Self::Scalar => 1,
            Self::Vec2 => 2,
            Self::Vec3 => 3,
            Self::Vec4 | Self::Mat2 => 4,
            Self::Mat3 => 9,
            Self::Mat4 => 16,
        }
    }

    /// Vertex attribute locations one element occupies: one per matrix column.
    pub fn get_attribute_location_count(self) -> usize {
        match self {
            Self::Scalar | Self::Vec2 | Self::Vec3 | Self::Vec4 => 1,
            Self::Mat2 => 2,
            Self::Mat3 => 3,
            Self::Mat4 => 4,
        }
    }

    pub fn get_glsl_type(self) -> &'static str {
        match self {
            Self::Scalar => "float",
            Self::Vec2 => "vec2",
            Self::Vec3 => "vec3",
            Self::Vec4 => "vec4",
            Self::Mat2 => "mat2",
            Self::Mat3 => "mat3",
            Self::Mat4 => "mat4",
        }
    }
}
