//! CesiumJS `RequestState`: where a request is in its life cycle.

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum RequestState {
    Unissued = 0,
    Issued = 1,
    Active = 2,
    Received = 3,
    Cancelled = 4,
    Failed = 5,
}

impl RequestState {
    pub const ALL: [Self; 6] =
        [Self::Unissued, Self::Issued, Self::Active, Self::Received, Self::Cancelled, Self::Failed];

    /// The variant with Cesium's numeric value.
    pub fn from_value(value: u8) -> Option<Self> {
        Self::ALL.into_iter().find(|v| *v as u8 == value)
    }

    pub fn validate(value: u8) -> bool {
        Self::from_value(value).is_some()
    }
}
