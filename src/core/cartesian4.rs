//! CesiumJS `Cartesian4`.

use super::cartesian::cartesian_common;
use super::color::Color;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Cartesian4 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub w: f64,
}

cartesian_common!(Cartesian4, 4, x, y, z, w);

impl Cartesian4 {
    pub const ZERO: Self = Self::new(0.0, 0.0, 0.0, 0.0);
    pub const ONE: Self = Self::new(1.0, 1.0, 1.0, 1.0);
    pub const UNIT_X: Self = Self::new(1.0, 0.0, 0.0, 0.0);
    pub const UNIT_Y: Self = Self::new(0.0, 1.0, 0.0, 0.0);
    pub const UNIT_Z: Self = Self::new(0.0, 0.0, 1.0, 0.0);
    pub const UNIT_W: Self = Self::new(0.0, 0.0, 0.0, 1.0);

    pub fn from_color(color: &Color) -> Self {
        Self::new(color.red, color.green, color.blue, color.alpha)
    }

    pub fn most_orthogonal_axis(&self) -> Self {
        let f = self.normalize().abs();
        if f.x <= f.y {
            if f.x <= f.z {
                if f.x <= f.w {
                    Self::UNIT_X
                } else {
                    Self::UNIT_W
                }
            } else if f.z <= f.w {
                Self::UNIT_Z
            } else {
                Self::UNIT_W
            }
        } else if f.y <= f.z {
            if f.y <= f.w {
                Self::UNIT_Y
            } else {
                Self::UNIT_W
            }
        } else if f.z <= f.w {
            Self::UNIT_Z
        } else {
            Self::UNIT_W
        }
    }

    /// The four bytes of `value` as an IEEE float32, little-endian, one per component.
    pub fn pack_float(value: f64) -> Self {
        let b = (value as f32).to_le_bytes();
        Self::new(f64::from(b[0]), f64::from(b[1]), f64::from(b[2]), f64::from(b[3]))
    }

    /// Inverse of [`Cartesian4::pack_float`]. Components are truncated to bytes.
    pub fn unpack_float(packed: Self) -> f64 {
        let byte = |v: f64| (v as i64 & 0xff) as u8;
        f64::from(f32::from_le_bytes([byte(packed.x), byte(packed.y), byte(packed.z), byte(packed.w)]))
    }
}
