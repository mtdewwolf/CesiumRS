//! CesiumJS `UniformType`: GLSL types a custom shader uniform can declare.

string_enum!(UniformType {
    Float = "float" => "FLOAT",
    Vec2 = "vec2" => "VEC2",
    Vec3 = "vec3" => "VEC3",
    Vec4 = "vec4" => "VEC4",
    Int = "int" => "INT",
    IntVec2 = "ivec2" => "INT_VEC2",
    IntVec3 = "ivec3" => "INT_VEC3",
    IntVec4 = "ivec4" => "INT_VEC4",
    Bool = "bool" => "BOOL",
    BoolVec2 = "bvec2" => "BOOL_VEC2",
    BoolVec3 = "bvec3" => "BOOL_VEC3",
    BoolVec4 = "bvec4" => "BOOL_VEC4",
    Mat2 = "mat2" => "MAT2",
    Mat3 = "mat3" => "MAT3",
    Mat4 = "mat4" => "MAT4",
    Sampler2D = "sampler2D" => "SAMPLER_2D",
    SamplerCube = "samplerCube" => "SAMPLER_CUBE",
});
