//! CesiumJS `StencilOperation`: what the stencil test writes.

use crate::core::webgl_constants as gl;

numeric_enum!(StencilOperation: u32 {
    Zero = gl::ZERO => "ZERO",
    Keep = gl::KEEP => "KEEP",
    Replace = gl::REPLACE => "REPLACE",
    Increment = gl::INCR => "INCREMENT",
    Decrement = gl::DECR => "DECREMENT",
    Invert = gl::INVERT => "INVERT",
    IncrementWrap = gl::INCR_WRAP => "INCREMENT_WRAP",
    DecrementWrap = gl::DECR_WRAP => "DECREMENT_WRAP",
});
