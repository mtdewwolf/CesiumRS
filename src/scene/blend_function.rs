//! CesiumJS `BlendFunction`: the factors blending multiplies source and destination by.

use crate::core::webgl_constants as gl;

numeric_enum!(BlendFunction: u32 {
    Zero = gl::ZERO => "ZERO",
    One = gl::ONE => "ONE",
    SourceColor = gl::SRC_COLOR => "SOURCE_COLOR",
    OneMinusSourceColor = gl::ONE_MINUS_SRC_COLOR => "ONE_MINUS_SOURCE_COLOR",
    DestinationColor = gl::DST_COLOR => "DESTINATION_COLOR",
    OneMinusDestinationColor = gl::ONE_MINUS_DST_COLOR => "ONE_MINUS_DESTINATION_COLOR",
    SourceAlpha = gl::SRC_ALPHA => "SOURCE_ALPHA",
    OneMinusSourceAlpha = gl::ONE_MINUS_SRC_ALPHA => "ONE_MINUS_SOURCE_ALPHA",
    DestinationAlpha = gl::DST_ALPHA => "DESTINATION_ALPHA",
    OneMinusDestinationAlpha = gl::ONE_MINUS_DST_ALPHA => "ONE_MINUS_DESTINATION_ALPHA",
    ConstantColor = gl::CONSTANT_COLOR => "CONSTANT_COLOR",
    OneMinusConstantColor = gl::ONE_MINUS_CONSTANT_COLOR => "ONE_MINUS_CONSTANT_COLOR",
    ConstantAlpha = gl::CONSTANT_ALPHA => "CONSTANT_ALPHA",
    OneMinusConstantAlpha = gl::ONE_MINUS_CONSTANT_ALPHA => "ONE_MINUS_CONSTANT_ALPHA",
    SourceAlphaSaturate = gl::SRC_ALPHA_SATURATE => "SOURCE_ALPHA_SATURATE",
});
