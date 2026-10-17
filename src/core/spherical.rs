//! CesiumJS `Spherical`: clock (azimuth), cone (from +Z), and magnitude.

use super::cartesian3::Cartesian3;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spherical {
    pub clock: f64,
    pub cone: f64,
    pub magnitude: f64,
}

impl Default for Spherical {
    fn default() -> Self {
        Self::new(0.0, 0.0, 1.0)
    }
}

impl Spherical {
    pub const fn new(clock: f64, cone: f64, magnitude: f64) -> Self {
        Self { clock, cone, magnitude }
    }

    pub fn from_cartesian3(c: Cartesian3) -> Self {
        let radial_squared = c.x * c.x + c.y * c.y;
        Self {
            clock: c.y.atan2(c.x),
            cone: radial_squared.sqrt().atan2(c.z),
            magnitude: (radial_squared + c.z * c.z).sqrt(),
        }
    }

    /// Same direction with unit magnitude.
    pub fn normalize(&self) -> Self {
        Self::new(self.clock, self.cone, 1.0)
    }

    pub fn equals(&self, other: &Self) -> bool {
        self == other
    }

    pub fn equals_epsilon(&self, other: &Self, epsilon: f64) -> bool {
        (self.clock - other.clock).abs() <= epsilon
            && (self.cone - other.cone).abs() <= epsilon
            && (self.magnitude - other.magnitude).abs() <= epsilon
    }
}

impl std::fmt::Display for Spherical {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "({}, {}, {})", self.clock, self.cone, self.magnitude)
    }
}
