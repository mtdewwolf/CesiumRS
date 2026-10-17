//! CesiumJS `Cartesian2`.

use super::cartesian::cartesian_common;
use super::cartesian3::Cartesian3;
use super::cartesian4::Cartesian4;
use super::math::acos_clamped;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Cartesian2 {
    pub x: f64,
    pub y: f64,
}

cartesian_common!(Cartesian2, 2, x, y);

impl Cartesian2 {
    pub const ZERO: Self = Self::new(0.0, 0.0);
    pub const ONE: Self = Self::new(1.0, 1.0);
    pub const UNIT_X: Self = Self::new(1.0, 0.0);
    pub const UNIT_Y: Self = Self::new(0.0, 1.0);

    pub fn from_cartesian3(c: Cartesian3) -> Self {
        Self::new(c.x, c.y)
    }

    pub fn from_cartesian4(c: Cartesian4) -> Self {
        Self::new(c.x, c.y)
    }

    /// The z component of the 3D cross product.
    pub fn cross(&self, other: Self) -> f64 {
        self.x * other.y - self.y * other.x
    }

    pub fn angle_between(&self, other: Self) -> f64 {
        acos_clamped(self.normalize().dot(other.normalize()))
    }

    pub fn most_orthogonal_axis(&self) -> Self {
        let f = self.normalize().abs();
        if f.x <= f.y {
            Self::UNIT_X
        } else {
            Self::UNIT_Y
        }
    }
}
