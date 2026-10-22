//! CesiumJS `Cartesian3`.

use super::cartesian::cartesian_common;
use super::cartesian4::Cartesian4;
use super::ellipsoid::Ellipsoid;
use super::js_math::JsMath;
use super::math::to_radians;
use super::spherical::Spherical;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Cartesian3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

cartesian_common!(Cartesian3, 3, x, y, z);

impl Cartesian3 {
    pub const ZERO: Self = Self::new(0.0, 0.0, 0.0);
    pub const ONE: Self = Self::new(1.0, 1.0, 1.0);
    pub const UNIT_X: Self = Self::new(1.0, 0.0, 0.0);
    pub const UNIT_Y: Self = Self::new(0.0, 1.0, 0.0);
    pub const UNIT_Z: Self = Self::new(0.0, 0.0, 1.0);

    pub fn from_spherical(spherical: &Spherical) -> Self {
        let radial = spherical.magnitude * spherical.cone.js_sin();
        Self::new(
            radial * spherical.clock.js_cos(),
            radial * spherical.clock.js_sin(),
            spherical.magnitude * spherical.cone.js_cos(),
        )
    }

    pub fn from_cartesian4(c: Cartesian4) -> Self {
        Self::new(c.x, c.y, c.z)
    }

    pub fn cross(&self, right: Self) -> Self {
        Self::new(
            self.y * right.z - self.z * right.y,
            self.z * right.x - self.x * right.z,
            self.x * right.y - self.y * right.x,
        )
    }

    pub fn midpoint(&self, right: Self) -> Self {
        Self::new((self.x + right.x) * 0.5, (self.y + right.y) * 0.5, (self.z + right.z) * 0.5)
    }

    /// Angle in radians, computed with `atan2` for accuracy near 0 and π.
    pub fn angle_between(&self, right: Self) -> f64 {
        let a = self.normalize();
        let b = right.normalize();
        let cosine = a.dot(b);
        let sine = a.cross(b).magnitude();
        sine.js_atan2(cosine)
    }

    /// The unit axis least aligned with this vector.
    pub fn most_orthogonal_axis(&self) -> Self {
        let f = self.normalize().abs();
        if f.x <= f.y {
            if f.x <= f.z {
                Self::UNIT_X
            } else {
                Self::UNIT_Z
            }
        } else if f.y <= f.z {
            Self::UNIT_Y
        } else {
            Self::UNIT_Z
        }
    }

    /// Projection of `self` onto `b`.
    pub fn project_vector(&self, b: Self) -> Self {
        b.multiply_by_scalar(self.dot(b) / b.dot(b))
    }

    /// Longitude and latitude in degrees, height in meters.
    pub fn from_degrees(longitude: f64, latitude: f64, height: f64, ellipsoid: &Ellipsoid) -> Self {
        Self::from_radians(to_radians(longitude), to_radians(latitude), height, ellipsoid)
    }

    /// Longitude and latitude in radians, height in meters.
    pub fn from_radians(longitude: f64, latitude: f64, height: f64, ellipsoid: &Ellipsoid) -> Self {
        let cos_latitude = latitude.js_cos();
        let n = Self::new(cos_latitude * longitude.js_cos(), cos_latitude * longitude.js_sin(), latitude.js_sin())
            .normalize();
        let k = ellipsoid.radii_squared.multiply_components(n);
        let gamma = n.dot(k).sqrt();
        k.divide_by_scalar(gamma).add(n.multiply_by_scalar(height))
    }

    /// `[lon0, lat0, lon1, lat1, ...]` in degrees.
    pub fn from_degrees_array(coordinates: &[f64], ellipsoid: &Ellipsoid) -> Vec<Self> {
        assert!(
            coordinates.len() >= 2 && coordinates.len().is_multiple_of(2),
            "the number of coordinates must be a multiple of 2 and at least 2"
        );
        coordinates.as_chunks::<2>().0.iter().map(|c| Self::from_degrees(c[0], c[1], 0.0, ellipsoid)).collect()
    }

    /// `[lon0, lat0, lon1, lat1, ...]` in radians.
    pub fn from_radians_array(coordinates: &[f64], ellipsoid: &Ellipsoid) -> Vec<Self> {
        assert!(
            coordinates.len() >= 2 && coordinates.len().is_multiple_of(2),
            "the number of coordinates must be a multiple of 2 and at least 2"
        );
        coordinates.as_chunks::<2>().0.iter().map(|c| Self::from_radians(c[0], c[1], 0.0, ellipsoid)).collect()
    }

    /// `[lon0, lat0, h0, ...]` with degrees and meters.
    pub fn from_degrees_array_heights(coordinates: &[f64], ellipsoid: &Ellipsoid) -> Vec<Self> {
        assert!(
            coordinates.len() >= 3 && coordinates.len().is_multiple_of(3),
            "the number of coordinates must be a multiple of 3 and at least 3"
        );
        coordinates.as_chunks::<3>().0.iter().map(|c| Self::from_degrees(c[0], c[1], c[2], ellipsoid)).collect()
    }

    /// `[lon0, lat0, h0, ...]` with radians and meters.
    pub fn from_radians_array_heights(coordinates: &[f64], ellipsoid: &Ellipsoid) -> Vec<Self> {
        assert!(
            coordinates.len() >= 3 && coordinates.len().is_multiple_of(3),
            "the number of coordinates must be a multiple of 3 and at least 3"
        );
        coordinates.as_chunks::<3>().0.iter().map(|c| Self::from_radians(c[0], c[1], c[2], ellipsoid)).collect()
    }
}
