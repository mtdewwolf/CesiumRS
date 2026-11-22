//! CesiumJS `KmlLookAt`: look-at pose parsed from KML `<LookAt>`.
use crate::core::{Cartesian3, HeadingPitchRange};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct KmlLookAt {
    pub position: Cartesian3,
    pub heading_pitch_range: HeadingPitchRange,
}

impl KmlLookAt {
    pub fn new(position: Cartesian3, heading_pitch_range: HeadingPitchRange) -> Self {
        Self { position, heading_pitch_range }
    }
}
