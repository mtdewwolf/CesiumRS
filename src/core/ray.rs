//! CesiumJS `Ray`: an origin and a unit direction.

use super::cartesian3::Cartesian3;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Ray {
    pub origin: Cartesian3,
    pub direction: Cartesian3,
}

impl Ray {
    /// Normalizes `direction` unless it is zero, like Cesium's constructor.
    pub fn new(origin: Cartesian3, direction: Cartesian3) -> Self {
        let direction = if direction == Cartesian3::ZERO { direction } else { direction.normalize() };
        Self { origin, direction }
    }

    /// `origin + t * direction`.
    pub fn get_point(&self, t: f64) -> Cartesian3 {
        self.origin.add(self.direction.multiply_by_scalar(t))
    }
}
