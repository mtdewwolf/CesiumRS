//! CesiumJS `BlendEquation`: how source and destination colors are combined.

use crate::core::webgl_constants as gl;

numeric_enum!(BlendEquation: u32 {
    Add = gl::FUNC_ADD => "ADD",
    Subtract = gl::FUNC_SUBTRACT => "SUBTRACT",
    ReverseSubtract = gl::FUNC_REVERSE_SUBTRACT => "REVERSE_SUBTRACT",
    Min = gl::MIN => "MIN",
    Max = gl::MAX => "MAX",
});
