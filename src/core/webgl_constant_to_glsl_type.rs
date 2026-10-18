//! CesiumJS `webGLConstantToGlslType`.

use super::webgl_constants as gl;

/// The GLSL type name for a uniform's GL type, for the types Cesium maps.
pub fn webgl_constant_to_glsl_type(value: u32) -> Option<&'static str> {
    Some(match value {
        gl::FLOAT => "float",
        gl::FLOAT_VEC2 => "vec2",
        gl::FLOAT_VEC3 => "vec3",
        gl::FLOAT_VEC4 => "vec4",
        gl::FLOAT_MAT2 => "mat2",
        gl::FLOAT_MAT3 => "mat3",
        gl::FLOAT_MAT4 => "mat4",
        gl::SAMPLER_2D => "sampler2D",
        gl::BOOL => "bool",
        _ => return None,
    })
}
