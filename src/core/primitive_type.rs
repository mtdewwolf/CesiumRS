//! CesiumJS `PrimitiveType`: how vertices are assembled for drawing.

use super::webgl_constants as gl;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u32)]
pub enum PrimitiveType {
    Points = gl::POINTS,
    Lines = gl::LINES,
    LineLoop = gl::LINE_LOOP,
    LineStrip = gl::LINE_STRIP,
    Triangles = gl::TRIANGLES,
    TriangleStrip = gl::TRIANGLE_STRIP,
    TriangleFan = gl::TRIANGLE_FAN,
}

impl PrimitiveType {
    pub fn is_lines(self) -> bool {
        matches!(self, Self::Lines | Self::LineLoop | Self::LineStrip)
    }

    pub fn is_triangles(self) -> bool {
        matches!(self, Self::Triangles | Self::TriangleStrip | Self::TriangleFan)
    }

    pub fn from_gl(value: u32) -> Option<Self> {
        [
            Self::Points,
            Self::Lines,
            Self::LineLoop,
            Self::LineStrip,
            Self::Triangles,
            Self::TriangleStrip,
            Self::TriangleFan,
        ]
        .into_iter()
        .find(|p| *p as u32 == value)
    }

    pub fn validate(value: u32) -> bool {
        Self::from_gl(value).is_some()
    }
}
