//! CesiumJS `TerrainQuantization`: how terrain vertices are quantized.

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum TerrainQuantization {
    None = 0,
    Bits12 = 1,
}

impl TerrainQuantization {
    pub const ALL: [Self; 2] = [Self::None, Self::Bits12];

    /// The variant with Cesium's numeric value.
    pub fn from_value(value: u8) -> Option<Self> {
        Self::ALL.into_iter().find(|v| *v as u8 == value)
    }

    pub fn validate(value: u8) -> bool {
        Self::from_value(value).is_some()
    }
}
