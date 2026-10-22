//! CesiumJS `VaryingType`: GLSL types a custom shader varying can declare.

string_enum!(VaryingType {
    Float = "float" => "FLOAT",
    Vec2 = "vec2" => "VEC2",
    Vec3 = "vec3" => "VEC3",
    Vec4 = "vec4" => "VEC4",
    Mat2 = "mat2" => "MAT2",
    Mat3 = "mat3" => "MAT3",
    Mat4 = "mat4" => "MAT4",
});
