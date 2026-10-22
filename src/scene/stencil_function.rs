//! CesiumJS `StencilFunction`: the stencil test comparison.

use crate::core::webgl_constants as gl;

numeric_enum!(StencilFunction: u32 {
    Never = gl::NEVER => "NEVER",
    Less = gl::LESS => "LESS",
    Equal = gl::EQUAL => "EQUAL",
    LessOrEqual = gl::LEQUAL => "LESS_OR_EQUAL",
    Greater = gl::GREATER => "GREATER",
    NotEqual = gl::NOTEQUAL => "NOT_EQUAL",
    GreaterOrEqual = gl::GEQUAL => "GREATER_OR_EQUAL",
    Always = gl::ALWAYS => "ALWAYS",
});
