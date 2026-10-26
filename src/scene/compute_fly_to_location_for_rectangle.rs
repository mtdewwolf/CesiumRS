//! CesiumJS `computeFlyToLocationForRectangle`: computes the final camera location
//! to view a rectangle adjusted for the current terrain heights.

use super::{camera::Camera, scene_mode::SceneMode};
use crate::core::{Cartographic, Ellipsoid, MapProjection, Rectangle};

/// Computes the 5 sample cartographic positions (center, SE, SW, NE, NW) to query
/// for terrain height adjustment.
pub fn rectangle_terrain_sample_points(rectangle: &Rectangle) -> [Cartographic; 5] {
    [rectangle.center(), rectangle.southeast(), rectangle.southwest(), rectangle.northeast(), rectangle.northwest()]
}

/// Computes the optimal camera location to view a rectangle, taking optional
/// sampled terrain heights into account.
pub fn compute_fly_to_location_for_rectangle(
    rectangle: &Rectangle,
    camera: &mut Camera,
    mode: SceneMode,
    ellipsoid: &Ellipsoid,
    projection: &dyn MapProjection,
    sampled_terrain_heights: Option<&[Option<f64>]>,
) -> Option<Cartographic> {
    let tmp = camera.get_rectangle_camera_coordinates(*rectangle)?;

    let mut position_without_terrain = match mode {
        SceneMode::Scene3D => ellipsoid.cartesian_to_cartographic(tmp)?,
        _ => projection.unproject(tmp),
    };

    if mode != SceneMode::Scene2D {
        if let Some(heights) = sampled_terrain_heights {
            let mut height_found = false;
            let mut max_height = -f64::MAX;

            for h in heights.iter().flatten() {
                height_found = true;
                if *h > max_height {
                    max_height = *h;
                }
            }

            if height_found {
                position_without_terrain.height += max_height;
            }
        }
    }

    Some(position_without_terrain)
}
