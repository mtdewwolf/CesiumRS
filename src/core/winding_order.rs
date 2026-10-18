//! CesiumJS `WindingOrder`: the vertex order of a front-facing triangle.

use super::webgl_constants as gl;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u32)]
pub enum WindingOrder {
    Clockwise = gl::CW,
    CounterClockwise = gl::CCW,
}

impl WindingOrder {
    pub const ALL: [Self; 2] = [Self::Clockwise, Self::CounterClockwise];

    /// The variant with Cesium's numeric value.
    pub fn from_value(value: u32) -> Option<Self> {
        Self::ALL.into_iter().find(|v| *v as u32 == value)
    }

    pub fn validate(value: u32) -> bool {
        Self::from_value(value).is_some()
    }
}
