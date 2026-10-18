//! CesiumJS `TileEdge`: an edge or corner of a terrain tile.

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum TileEdge {
    West = 0,
    North = 1,
    East = 2,
    South = 3,
    Northwest = 4,
    Northeast = 5,
    Southwest = 6,
    Southeast = 7,
}

impl TileEdge {
    pub const ALL: [Self; 8] = [
        Self::West,
        Self::North,
        Self::East,
        Self::South,
        Self::Northwest,
        Self::Northeast,
        Self::Southwest,
        Self::Southeast,
    ];

    /// The variant with Cesium's numeric value.
    pub fn from_value(value: u8) -> Option<Self> {
        Self::ALL.into_iter().find(|v| *v as u8 == value)
    }

    pub fn validate(value: u8) -> bool {
        Self::from_value(value).is_some()
    }
}
