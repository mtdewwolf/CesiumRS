//! CesiumJS `VerticalExaggeration`: scaling heights about a reference height.

use super::cartesian3::Cartesian3;
use super::ellipsoid::Ellipsoid;

pub fn get_height(height: f64, scale: f64, relative_height: f64) -> f64 {
    (height - relative_height) * scale + relative_height
}

/// `position` with its height above `ellipsoid` exaggerated; unchanged at
/// the ellipsoid's center.
pub fn get_position(
    position: Cartesian3,
    ellipsoid: &Ellipsoid,
    vertical_exaggeration: f64,
    vertical_exaggeration_relative_height: f64,
) -> Cartesian3 {
    match ellipsoid.cartesian_to_cartographic(position) {
        None => position,
        Some(c) => {
            let height = get_height(c.height, vertical_exaggeration, vertical_exaggeration_relative_height);
            Cartesian3::from_radians(c.longitude, c.latitude, height, ellipsoid)
        }
    }
}
