//! CesiumJS `barycentricCoordinates` and `pointInsideTriangle`.

use super::cartesian2::Cartesian2;
use super::cartesian3::Cartesian3;
use super::math::EPSILON14;

fn from_dots(dot00: f64, dot01: f64, dot02: f64, dot11: f64, dot12: f64) -> Option<Cartesian3> {
    let mut y = dot11 * dot02 - dot01 * dot12;
    let mut z = dot00 * dot12 - dot01 * dot02;
    let q = dot00 * dot11 - dot01 * dot01;
    if q == 0.0 {
        return None;
    }
    y /= q;
    z /= q;
    Some(Cartesian3::new(1.0 - y - z, y, z))
}

/// Barycentric coordinates of `point` in a 3D triangle; `None` if it is degenerate.
pub fn barycentric_coordinates(
    point: Cartesian3,
    p0: Cartesian3,
    p1: Cartesian3,
    p2: Cartesian3,
) -> Option<Cartesian3> {
    if point.equals_epsilon(p0, EPSILON14) {
        return Some(Cartesian3::UNIT_X);
    }
    if point.equals_epsilon(p1, EPSILON14) {
        return Some(Cartesian3::UNIT_Y);
    }
    if point.equals_epsilon(p2, EPSILON14) {
        return Some(Cartesian3::UNIT_Z);
    }
    let v0 = p1.subtract(p0);
    let v1 = p2.subtract(p0);
    let v2 = point.subtract(p0);
    from_dots(v0.dot(v0), v0.dot(v1), v0.dot(v2), v1.dot(v1), v1.dot(v2))
}

/// The same for a 2D triangle.
pub fn barycentric_coordinates_2d(
    point: Cartesian2,
    p0: Cartesian2,
    p1: Cartesian2,
    p2: Cartesian2,
) -> Option<Cartesian3> {
    if point.equals_epsilon(p0, EPSILON14) {
        return Some(Cartesian3::UNIT_X);
    }
    if point.equals_epsilon(p1, EPSILON14) {
        return Some(Cartesian3::UNIT_Y);
    }
    if point.equals_epsilon(p2, EPSILON14) {
        return Some(Cartesian3::UNIT_Z);
    }
    let v0 = p1.subtract(p0);
    let v1 = p2.subtract(p0);
    let v2 = point.subtract(p0);
    from_dots(v0.dot(v0), v0.dot(v1), v0.dot(v2), v1.dot(v1), v1.dot(v2))
}

/// Whether `point` is strictly inside the triangle.
pub fn point_inside_triangle(point: Cartesian3, p0: Cartesian3, p1: Cartesian3, p2: Cartesian3) -> bool {
    barycentric_coordinates(point, p0, p1, p2).is_some_and(|c| c.x > 0.0 && c.y > 0.0 && c.z > 0.0)
}

/// The same for a 2D triangle.
pub fn point_inside_triangle_2d(point: Cartesian2, p0: Cartesian2, p1: Cartesian2, p2: Cartesian2) -> bool {
    barycentric_coordinates_2d(point, p0, p1, p2).is_some_and(|c| c.x > 0.0 && c.y > 0.0 && c.z > 0.0)
}
