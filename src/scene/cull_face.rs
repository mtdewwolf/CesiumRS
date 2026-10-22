//! CesiumJS `CullFace`: which triangle faces are culled.

use crate::core::webgl_constants as gl;

numeric_enum!(CullFace: u32 {
    Front = gl::FRONT => "FRONT",
    Back = gl::BACK => "BACK",
    FrontAndBack = gl::FRONT_AND_BACK => "FRONT_AND_BACK",
});
