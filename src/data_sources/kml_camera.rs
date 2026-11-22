//! CesiumJS `KmlCamera`: camera pose parsed from KML `<Camera>`.
use crate::core::{Cartesian3, HeadingPitchRoll};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct KmlCamera {
    pub position: Cartesian3,
    pub heading_pitch_roll: HeadingPitchRoll,
}

impl KmlCamera {
    pub fn new(position: Cartesian3, heading_pitch_roll: HeadingPitchRoll) -> Self {
        Self { position, heading_pitch_roll }
    }
}
