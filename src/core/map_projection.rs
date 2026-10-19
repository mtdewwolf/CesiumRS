//! CesiumJS `MapProjection`: maps cartographic positions to 2D map coordinates.

use super::cartesian3::Cartesian3;
use super::cartographic::Cartographic;
use super::ellipsoid::Ellipsoid;

pub trait MapProjection {
    /// Replaces GeographicProjection instanceof checks in terrain processing.
    fn is_geographic(&self) -> bool {
        false
    }
    fn ellipsoid(&self) -> &Ellipsoid;
    /// Map coordinates in meters, with the height passed through as `z`.
    fn project(&self, cartographic: &Cartographic) -> Cartesian3;
    fn unproject(&self, cartesian: Cartesian3) -> Cartographic;
}
