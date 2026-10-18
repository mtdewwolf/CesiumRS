//! CesiumJS `HeadingPitchRoll`: rotation about -Z, -Y, and X in a local frame.

use super::js_math::JsMath;
use super::math::{asin_clamped, equals_epsilon, RADIANS_PER_DEGREE};
use super::quaternion::Quaternion;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct HeadingPitchRoll {
    pub heading: f64,
    pub pitch: f64,
    pub roll: f64,
}

impl HeadingPitchRoll {
    pub const fn new(heading: f64, pitch: f64, roll: f64) -> Self {
        Self { heading, pitch, roll }
    }

    pub fn from_quaternion(q: &Quaternion) -> Self {
        let test = 2.0 * (q.w * q.y - q.z * q.x);
        let denominator_roll = 1.0 - 2.0 * (q.x * q.x + q.y * q.y);
        let numerator_roll = 2.0 * (q.w * q.x + q.y * q.z);
        let denominator_heading = 1.0 - 2.0 * (q.y * q.y + q.z * q.z);
        let numerator_heading = 2.0 * (q.w * q.z + q.x * q.y);
        Self {
            heading: -numerator_heading.js_atan2(denominator_heading),
            roll: numerator_roll.js_atan2(denominator_roll),
            pitch: -asin_clamped(test),
        }
    }

    pub fn from_degrees(heading: f64, pitch: f64, roll: f64) -> Self {
        Self::new(heading * RADIANS_PER_DEGREE, pitch * RADIANS_PER_DEGREE, roll * RADIANS_PER_DEGREE)
    }

    pub fn equals(&self, r: &Self) -> bool {
        self == r
    }

    pub fn equals_epsilon(&self, r: &Self, relative_epsilon: f64, absolute_epsilon: f64) -> bool {
        equals_epsilon(self.heading, r.heading, relative_epsilon, absolute_epsilon)
            && equals_epsilon(self.pitch, r.pitch, relative_epsilon, absolute_epsilon)
            && equals_epsilon(self.roll, r.roll, relative_epsilon, absolute_epsilon)
    }
}

impl std::fmt::Display for HeadingPitchRoll {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "({}, {}, {})", self.heading, self.pitch, self.roll)
    }
}
