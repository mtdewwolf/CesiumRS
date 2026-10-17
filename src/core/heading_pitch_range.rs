//! CesiumJS `HeadingPitchRange`: a camera offset in a local east-north-up frame.

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct HeadingPitchRange {
    /// Radians clockwise from north.
    pub heading: f64,
    /// Radians above the local horizontal plane; negative looks down.
    pub pitch: f64,
    /// Meters from the center of the local frame.
    pub range: f64,
}

impl HeadingPitchRange {
    pub const fn new(heading: f64, pitch: f64, range: f64) -> Self {
        Self { heading, pitch, range }
    }
}
