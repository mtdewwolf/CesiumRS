//! CesiumJS `ExtrapolationType`: what a sampled value does outside its samples.

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum ExtrapolationType {
    None = 0,
    Hold = 1,
    Extrapolate = 2,
}

impl ExtrapolationType {
    pub const ALL: [Self; 3] = [Self::None, Self::Hold, Self::Extrapolate];

    /// The variant with Cesium's numeric value.
    pub fn from_value(value: u8) -> Option<Self> {
        Self::ALL.into_iter().find(|v| *v as u8 == value)
    }

    pub fn validate(value: u8) -> bool {
        Self::from_value(value).is_some()
    }
}
