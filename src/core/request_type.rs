//! CesiumJS `RequestType`: the kind of data a request fetches, for throttling.

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum RequestType {
    Terrain = 0,
    Imagery = 1,
    Tiles3d = 2,
    Other = 3,
}

impl RequestType {
    pub const ALL: [Self; 4] = [Self::Terrain, Self::Imagery, Self::Tiles3d, Self::Other];

    /// The variant with Cesium's numeric value.
    pub fn from_value(value: u8) -> Option<Self> {
        Self::ALL.into_iter().find(|v| *v as u8 == value)
    }

    pub fn validate(value: u8) -> bool {
        Self::from_value(value).is_some()
    }
}
