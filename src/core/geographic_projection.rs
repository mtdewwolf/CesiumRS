//! CesiumJS `GeographicProjection`: equirectangular, longitude and latitude
//! scaled by the semimajor axis.

use super::cartesian3::Cartesian3;
use super::cartographic::Cartographic;
use super::ellipsoid::Ellipsoid;
use super::map_projection::MapProjection;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GeographicProjection {
    ellipsoid: Ellipsoid,
    semimajor_axis: f64,
    one_over_semimajor_axis: f64,
}

impl Default for GeographicProjection {
    fn default() -> Self {
        Self::new(Ellipsoid::WGS84)
    }
}

impl GeographicProjection {
    pub fn new(ellipsoid: Ellipsoid) -> Self {
        let semimajor_axis = ellipsoid.maximum_radius;
        Self { ellipsoid, semimajor_axis, one_over_semimajor_axis: 1.0 / semimajor_axis }
    }
}

impl MapProjection for GeographicProjection {
    fn is_geographic(&self) -> bool {
        true
    }
    fn ellipsoid(&self) -> &Ellipsoid {
        &self.ellipsoid
    }

    fn project(&self, c: &Cartographic) -> Cartesian3 {
        Cartesian3::new(c.longitude * self.semimajor_axis, c.latitude * self.semimajor_axis, c.height)
    }

    fn unproject(&self, p: Cartesian3) -> Cartographic {
        Cartographic::new(p.x * self.one_over_semimajor_axis, p.y * self.one_over_semimajor_axis, p.z)
    }
}
