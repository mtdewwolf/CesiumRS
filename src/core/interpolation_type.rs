//! CesiumJS `InterpolationType`: glTF animation interpolation.

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum InterpolationType {
    Step = 0,
    Linear = 1,
    Cubicspline = 2,
}

impl InterpolationType {
    pub const ALL: [Self; 3] = [Self::Step, Self::Linear, Self::Cubicspline];

    /// The variant with Cesium's numeric value.
    pub fn from_value(value: u8) -> Option<Self> {
        Self::ALL.into_iter().find(|v| *v as u8 == value)
    }

    pub fn validate(value: u8) -> bool {
        Self::from_value(value).is_some()
    }
}
