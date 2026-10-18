//! CesiumJS `VertexFormat`: which vertex attributes a geometry computes.

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct VertexFormat {
    pub position: bool,
    pub normal: bool,
    pub st: bool,
    pub bitangent: bool,
    pub tangent: bool,
    pub color: bool,
}

impl VertexFormat {
    pub const POSITION_ONLY: Self = Self::with(true, false, false, false, false, false);
    pub const POSITION_AND_NORMAL: Self = Self::with(true, true, false, false, false, false);
    pub const POSITION_NORMAL_AND_ST: Self = Self::with(true, true, true, false, false, false);
    pub const POSITION_AND_ST: Self = Self::with(true, false, true, false, false, false);
    pub const POSITION_AND_COLOR: Self = Self::with(true, false, false, false, false, true);
    pub const ALL: Self = Self::with(true, true, true, true, true, false);
    pub const DEFAULT: Self = Self::POSITION_NORMAL_AND_ST;
    pub const PACKED_LENGTH: usize = 6;

    const fn with(position: bool, normal: bool, st: bool, bitangent: bool, tangent: bool, color: bool) -> Self {
        Self { position, normal, st, bitangent, tangent, color }
    }

    /// Writes position, normal, st, tangent, bitangent, color as 1 or 0.
    pub fn pack(&self, array: &mut [f64], starting_index: usize) {
        let flags = [self.position, self.normal, self.st, self.tangent, self.bitangent, self.color];
        for (i, f) in flags.into_iter().enumerate() {
            array[starting_index + i] = if f { 1.0 } else { 0.0 };
        }
    }

    pub fn unpack(array: &[f64], starting_index: usize) -> Self {
        let f = |i: usize| array[starting_index + i] == 1.0;
        Self { position: f(0), normal: f(1), st: f(2), tangent: f(3), bitangent: f(4), color: f(5) }
    }
}
