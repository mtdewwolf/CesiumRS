//! CesiumJS `GeocodeType`: a full search or an autocomplete query.

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum GeocodeType {
    Search = 0,
    Autocomplete = 1,
}

impl GeocodeType {
    pub const ALL: [Self; 2] = [Self::Search, Self::Autocomplete];

    /// The variant with Cesium's numeric value.
    pub fn from_value(value: u8) -> Option<Self> {
        Self::ALL.into_iter().find(|v| *v as u8 == value)
    }

    pub fn validate(value: u8) -> bool {
        Self::from_value(value).is_some()
    }
}
