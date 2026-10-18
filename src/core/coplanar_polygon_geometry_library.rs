//! CesiumJS `CoplanarPolygonGeometryLibrary`: the plane a set of coplanar
//! points lies in, from their oriented bounding box, and projection onto it.

use super::cartesian2::Cartesian2;
use super::cartesian3::Cartesian3;
use super::math::js_min;
use super::oriented_bounding_box::OrientedBoundingBox;

fn axes(positions: &[Cartesian3]) -> (OrientedBoundingBox, [Cartesian3; 3], [f64; 3]) {
    let obb = OrientedBoundingBox::from_points(positions);
    let a = [obb.half_axes.get_column(0), obb.half_axes.get_column(1), obb.half_axes.get_column(2)];
    let m = a.map(|v| v.magnitude());
    (obb, a, m)
}

fn degenerate([x, y, z]: [f64; 3]) -> bool {
    (x == 0.0 && (y == 0.0 || z == 0.0)) || (y == 0.0 && z == 0.0)
}

/// Whether the points span at least a line (an outline can be drawn).
pub fn valid_outline(positions: &[Cartesian3]) -> bool {
    !degenerate(axes(positions).2)
}

/// The plane's center and two in-plane unit axes: the box axes other than
/// the shortest. `None` if the points do not span a plane.
pub fn compute_project_to_2d_arguments(positions: &[Cartesian3]) -> Option<(Cartesian3, Cartesian3, Cartesian3)> {
    let (obb, [x_axis, y_axis, z_axis], mags) = axes(positions);
    let [x_mag, y_mag, z_mag] = mags;
    if degenerate(mags) {
        return None;
    }
    let min = js_min(js_min(x_mag, y_mag), z_mag);
    let (mut axis1, mut axis2) = (None, None);
    if min == y_mag || min == z_mag {
        axis1 = Some(x_axis);
    }
    if min == x_mag {
        axis1 = Some(y_axis);
    } else if min == z_mag {
        axis2 = Some(y_axis);
    }
    if min == x_mag || min == y_mag {
        axis2 = Some(z_axis);
    }
    Some((obb.center, axis1?.normalize(), axis2?.normalize()))
}

/// `position` in the plane's 2D coordinates.
pub fn project_to_2d(position: Cartesian3, center: Cartesian3, axis1: Cartesian3, axis2: Cartesian3) -> Cartesian2 {
    let v = position.subtract(center);
    Cartesian2::new(axis1.dot(v), axis2.dot(v))
}
