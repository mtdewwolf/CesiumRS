//! CesiumJS `WebMercatorProjection` (EPSG:3857 on a sphere of the semimajor axis).

use super::cartesian3::Cartesian3;
use super::cartographic::Cartographic;
use super::ellipsoid::Ellipsoid;
use super::map_projection::MapProjection;
use super::math::PI_OVER_TWO;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct WebMercatorProjection {
    ellipsoid: Ellipsoid,
    semimajor_axis: f64,
    one_over_semimajor_axis: f64,
}

impl Default for WebMercatorProjection {
    fn default() -> Self {
        Self::new(Ellipsoid::WGS84)
    }
}

impl WebMercatorProjection {
    pub fn new(ellipsoid: Ellipsoid) -> Self {
        let semimajor_axis = ellipsoid.maximum_radius;
        Self { ellipsoid, semimajor_axis, one_over_semimajor_axis: 1.0 / semimajor_axis }
    }

    /// Latitude where the projection is cut off to make the map square, about 85.05°.
    pub fn maximum_latitude() -> f64 {
        Self::mercator_angle_to_geodetic_latitude(std::f64::consts::PI)
    }

    pub fn mercator_angle_to_geodetic_latitude(mercator_angle: f64) -> f64 {
        PI_OVER_TWO - 2.0 * (-mercator_angle).exp().atan()
    }

    /// Clamps to `±maximum_latitude()` first.
    pub fn geodetic_latitude_to_mercator_angle(latitude: f64) -> f64 {
        let max = Self::maximum_latitude();
        let latitude = if latitude > max {
            max
        } else if latitude < -max {
            -max
        } else {
            latitude
        };
        let s = latitude.sin();
        0.5 * ((1.0 + s) / (1.0 - s)).ln()
    }
}

impl MapProjection for WebMercatorProjection {
    fn ellipsoid(&self) -> &Ellipsoid {
        &self.ellipsoid
    }

    fn project(&self, c: &Cartographic) -> Cartesian3 {
        Cartesian3::new(
            c.longitude * self.semimajor_axis,
            Self::geodetic_latitude_to_mercator_angle(c.latitude) * self.semimajor_axis,
            c.height,
        )
    }

    fn unproject(&self, p: Cartesian3) -> Cartographic {
        Cartographic::new(
            p.x * self.one_over_semimajor_axis,
            Self::mercator_angle_to_geodetic_latitude(p.y * self.one_over_semimajor_axis),
            p.z,
        )
    }
}
