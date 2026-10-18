//! CesiumJS `Cartographic`: longitude and latitude in radians, height in meters.

use super::cartesian3::Cartesian3;
use super::ellipsoid::Ellipsoid;
use super::js_math::JsMath;
use super::math::{sign, to_radians};
use super::scale_to_geodetic_surface::scale_to_geodetic_surface;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Cartographic {
    pub longitude: f64,
    pub latitude: f64,
    pub height: f64,
}

impl Cartographic {
    pub const ZERO: Self = Self::new(0.0, 0.0, 0.0);

    pub const fn new(longitude: f64, latitude: f64, height: f64) -> Self {
        Self { longitude, latitude, height }
    }

    pub const fn from_radians(longitude: f64, latitude: f64, height: f64) -> Self {
        Self::new(longitude, latitude, height)
    }

    pub fn from_degrees(longitude: f64, latitude: f64, height: f64) -> Self {
        Self::new(to_radians(longitude), to_radians(latitude), height)
    }

    /// `None` at the center of the ellipsoid.
    pub fn from_cartesian(cartesian: Cartesian3, ellipsoid: &Ellipsoid) -> Option<Self> {
        let p = scale_to_geodetic_surface(
            cartesian,
            ellipsoid.one_over_radii,
            ellipsoid.one_over_radii_squared,
            ellipsoid.center_tolerance_squared,
        )?;
        let n = p.multiply_components(ellipsoid.one_over_radii_squared).normalize();
        let h = cartesian.subtract(p);
        Some(Self::new(n.y.js_atan2(n.x), n.z.js_asin(), sign(h.dot(cartesian)) * h.magnitude()))
    }

    pub fn to_cartesian(&self, ellipsoid: &Ellipsoid) -> Cartesian3 {
        Cartesian3::from_radians(self.longitude, self.latitude, self.height, ellipsoid)
    }

    pub fn equals(&self, right: &Self) -> bool {
        self == right
    }

    /// Every component within `epsilon` (absolute).
    pub fn equals_epsilon(&self, right: &Self, epsilon: f64) -> bool {
        (self.longitude - right.longitude).abs() <= epsilon
            && (self.latitude - right.latitude).abs() <= epsilon
            && (self.height - right.height).abs() <= epsilon
    }
}

impl std::fmt::Display for Cartographic {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "({}, {}, {})", self.longitude, self.latitude, self.height)
    }
}
