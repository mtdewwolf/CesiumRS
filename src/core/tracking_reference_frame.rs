//! CesiumJS `TrackingReferenceFrame`: the frame the camera tracks an entity in.

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum TrackingReferenceFrame {
    Autodetect = 0,
    Enu = 1,
    Inertial = 2,
    Velocity = 3,
}

impl TrackingReferenceFrame {
    pub const ALL: [Self; 4] = [Self::Autodetect, Self::Enu, Self::Inertial, Self::Velocity];

    /// The variant with Cesium's numeric value.
    pub fn from_value(value: u8) -> Option<Self> {
        Self::ALL.into_iter().find(|v| *v as u8 == value)
    }

    pub fn validate(value: u8) -> bool {
        Self::from_value(value).is_some()
    }
}
