//! CesiumJS `IntersectionTests`: rays, segments, and triangles against
//! planes, spheres, boxes, and ellipsoids.

use super::axis_aligned_bounding_box::AxisAlignedBoundingBox;
use super::bounding_sphere::BoundingSphere;
use super::cartesian3::Cartesian3;
use super::ellipsoid::Ellipsoid;
use super::interval::Interval;
use super::math::{clamp, js_max, js_min, sign, EPSILON12, EPSILON15, EPSILON6};
use super::matrix3::Matrix3;
use super::plane::Plane;
use super::quadratic_real_polynomial::{self, add_with_cancellation_check};
use super::quartic_real_polynomial;
use super::ray::Ray;

/// A triangle split by a plane: the original three positions, two new
/// points on the plane, and indices for the resulting triangles (the ones
/// behind the plane first when one vertex is behind).
#[derive(Clone, Debug, PartialEq)]
pub struct TrianglePlaneIntersection {
    pub positions: [Cartesian3; 5],
    pub indices: [u32; 9],
}

/// Point where the ray meets the plane, or `None` if it is parallel or behind.
pub fn ray_plane(ray: &Ray, plane: &Plane) -> Option<Cartesian3> {
    let denominator = plane.normal.dot(ray.direction);
    if denominator.abs() < EPSILON15 {
        return None;
    }
    let t = (-plane.distance - plane.normal.dot(ray.origin)) / denominator;
    if t < 0.0 {
        return None;
    }
    Some(ray.origin.add(ray.direction.multiply_by_scalar(t)))
}

/// Möller–Trumbore. Returns the ray parameter, which may be negative.
pub fn ray_triangle_parametric(
    ray: &Ray,
    p0: Cartesian3,
    p1: Cartesian3,
    p2: Cartesian3,
    cull_back_faces: bool,
) -> Option<f64> {
    let origin = ray.origin;
    let direction = ray.direction;
    let edge0 = p1.subtract(p0);
    let edge1 = p2.subtract(p0);
    let p = direction.cross(edge1);
    let det = edge0.dot(p);
    if cull_back_faces {
        if det < EPSILON6 {
            return None;
        }
        let tvec = origin.subtract(p0);
        let u = tvec.dot(p);
        if u < 0.0 || u > det {
            return None;
        }
        let q = tvec.cross(edge0);
        let v = direction.dot(q);
        if v < 0.0 || u + v > det {
            return None;
        }
        Some(edge1.dot(q) / det)
    } else {
        if det.abs() < EPSILON6 {
            return None;
        }
        let inv_det = 1.0 / det;
        let tvec = origin.subtract(p0);
        let u = tvec.dot(p) * inv_det;
        if !(0.0..=1.0).contains(&u) {
            return None;
        }
        let q = tvec.cross(edge0);
        let v = direction.dot(q) * inv_det;
        if v < 0.0 || u + v > 1.0 {
            return None;
        }
        Some(edge1.dot(q) * inv_det)
    }
}

pub fn ray_triangle(ray: &Ray, p0: Cartesian3, p1: Cartesian3, p2: Cartesian3, cull_back_faces: bool) -> Option<Cartesian3> {
    let t = ray_triangle_parametric(ray, p0, p1, p2, cull_back_faces)?;
    if t < 0.0 {
        return None;
    }
    Some(ray.origin.add(ray.direction.multiply_by_scalar(t)))
}

pub fn line_segment_triangle(
    v0: Cartesian3,
    v1: Cartesian3,
    p0: Cartesian3,
    p1: Cartesian3,
    p2: Cartesian3,
    cull_back_faces: bool,
) -> Option<Cartesian3> {
    let ray = Ray { origin: v0, direction: v1.subtract(v0).normalize() };
    let t = ray_triangle_parametric(&ray, p0, p1, p2, cull_back_faces)?;
    if t < 0.0 || t > v0.distance(v1) {
        return None;
    }
    Some(ray.origin.add(ray.direction.multiply_by_scalar(t)))
}

fn solve_quadratic(a: f64, b: f64, c: f64) -> Option<(f64, f64)> {
    let det = b * b - 4.0 * a * c;
    if det < 0.0 {
        return None;
    } else if det > 0.0 {
        let denom = 1.0 / (2.0 * a);
        let disc = det.sqrt();
        let root0 = (-b + disc) * denom;
        let root1 = (-b - disc) * denom;
        return Some(if root0 < root1 { (root0, root1) } else { (root1, root0) });
    }
    let root = -b / (2.0 * a);
    if root == 0.0 {
        return None;
    }
    Some((root, root))
}

fn ray_sphere_raw(ray: &Ray, sphere: &BoundingSphere) -> Option<Interval> {
    let radius_squared = sphere.radius * sphere.radius;
    let diff = ray.origin.subtract(sphere.center);
    let a = ray.direction.dot(ray.direction);
    let b = 2.0 * ray.direction.dot(diff);
    let c = diff.magnitude_squared() - radius_squared;
    let (root0, root1) = solve_quadratic(a, b, c)?;
    Some(Interval::new(root0, root1))
}

/// Parameter interval where the ray is inside the sphere, clipped to `t >= 0`.
pub fn ray_sphere(ray: &Ray, sphere: &BoundingSphere) -> Option<Interval> {
    let mut result = ray_sphere_raw(ray, sphere)?;
    if result.stop < 0.0 {
        return None;
    }
    result.start = js_max(result.start, 0.0);
    Some(result)
}

/// Distances along the segment `p0 → p1` where it is inside the sphere.
pub fn line_segment_sphere(p0: Cartesian3, p1: Cartesian3, sphere: &BoundingSphere) -> Option<Interval> {
    let direction = p1.subtract(p0);
    let max_t = direction.magnitude();
    let ray = Ray { origin: p0, direction: direction.normalize() };
    let mut result = ray_sphere_raw(&ray, sphere)?;
    if result.stop < 0.0 || result.start > max_t {
        return None;
    }
    result.start = js_max(result.start, 0.0);
    result.stop = js_min(result.stop, max_t);
    Some(result)
}

/// Parameter interval where the ray is inside the ellipsoid, starting at 0
/// when the origin is inside. `None` if it misses.
pub fn ray_ellipsoid(ray: &Ray, ellipsoid: &Ellipsoid) -> Option<Interval> {
    let inverse_radii = ellipsoid.one_over_radii;
    let q = inverse_radii.multiply_components(ray.origin);
    let w = inverse_radii.multiply_components(ray.direction);
    let q2 = q.magnitude_squared();
    let qw = q.dot(w);
    if q2 > 1.0 {
        if qw >= 0.0 {
            return None;
        }
        let qw2 = qw * qw;
        let difference = q2 - 1.0;
        let w2 = w.magnitude_squared();
        let product = w2 * difference;
        if qw2 < product {
            return None;
        } else if qw2 > product {
            let discriminant = qw * qw - product;
            let temp = -qw + discriminant.sqrt();
            let root0 = temp / w2;
            let root1 = difference / temp;
            return Some(if root0 < root1 {
                Interval::new(root0, root1)
            } else {
                Interval::new(root1, root0)
            });
        }
        let root = (difference / w2).sqrt();
        return Some(Interval::new(root, root));
    } else if q2 < 1.0 {
        let difference = q2 - 1.0;
        let w2 = w.magnitude_squared();
        let product = w2 * difference;
        let discriminant = qw * qw - product;
        let temp = -qw + discriminant.sqrt();
        return Some(Interval::new(0.0, temp / w2));
    }
    if qw < 0.0 {
        let w2 = w.magnitude_squared();
        return Some(Interval::new(0.0, -qw / w2));
    }
    None
}

fn ray_interval_along_aabb_axis(origin: f64, direction: f64, min: f64, max: f64) -> Interval {
    let start = (min - origin) / direction;
    let stop = (max - origin) / direction;
    if stop < start {
        Interval::new(stop, start)
    } else {
        Interval::new(start, stop)
    }
}

/// Slab test. The interval may start before the ray origin.
pub fn ray_axis_aligned_bounding_box(ray: &Ray, box_: &AxisAlignedBoundingBox) -> Option<Interval> {
    let tx = ray_interval_along_aabb_axis(ray.origin.x, ray.direction.x, box_.minimum.x, box_.maximum.x);
    let ty = ray_interval_along_aabb_axis(ray.origin.y, ray.direction.y, box_.minimum.y, box_.maximum.y);
    let tz = ray_interval_along_aabb_axis(ray.origin.z, ray.direction.z, box_.minimum.z, box_.maximum.z);
    let mut result = Interval::new(
        if tx.start > ty.start { tx.start } else { ty.start },
        if tx.stop < ty.stop { tx.stop } else { ty.stop },
    );
    if tx.start > ty.stop || ty.start > tx.stop {
        return None;
    }
    if result.start > tz.stop || tz.start > result.stop {
        return None;
    }
    if tz.start > result.start {
        result.start = tz.start;
    }
    if tz.stop < result.stop {
        result.stop = tz.stop;
    }
    Some(result)
}

/// Solves `xᵀAx + bᵀx + c = 0` on the circle of radius `w` at `x` in the
/// y-z plane; returns the solution points.
pub fn quadratic_vector_expression(a: &Matrix3, b: Cartesian3, c: f64, x: f64, w: f64) -> Vec<Cartesian3> {
    let m = &a.0;
    let x_squared = x * x;
    let w_squared = w * w;
    let l2 = (m[Matrix3::COLUMN1ROW1] - m[Matrix3::COLUMN2ROW2]) * w_squared;
    let l1 = w * (x * add_with_cancellation_check(m[Matrix3::COLUMN1ROW0], m[Matrix3::COLUMN0ROW1], EPSILON15) + b.y);
    let l0 = m[Matrix3::COLUMN0ROW0] * x_squared + m[Matrix3::COLUMN2ROW2] * w_squared + x * b.x + c;
    let r1 = w_squared * add_with_cancellation_check(m[Matrix3::COLUMN2ROW1], m[Matrix3::COLUMN1ROW2], EPSILON15);
    // Cesium passes no tolerance here, so the sum is never zeroed.
    let r0 = w * (x * add_with_cancellation_check(m[Matrix3::COLUMN2ROW0], m[Matrix3::COLUMN0ROW2], f64::NAN) + b.z);

    let mut solutions = Vec::new();
    if r0 == 0.0 && r1 == 0.0 {
        let cosines = quadratic_real_polynomial::compute_real_roots(l2, l1, l0);
        if cosines.is_empty() {
            return solutions;
        }
        let cosine0 = cosines[0];
        let sine0 = js_max(1.0 - cosine0 * cosine0, 0.0).sqrt();
        solutions.push(Cartesian3::new(x, w * cosine0, w * -sine0));
        solutions.push(Cartesian3::new(x, w * cosine0, w * sine0));
        if cosines.len() == 2 {
            let cosine1 = cosines[1];
            let sine1 = js_max(1.0 - cosine1 * cosine1, 0.0).sqrt();
            solutions.push(Cartesian3::new(x, w * cosine1, w * -sine1));
            solutions.push(Cartesian3::new(x, w * cosine1, w * sine1));
        }
        return solutions;
    }

    let r0_squared = r0 * r0;
    let r1_squared = r1 * r1;
    let l2_squared = l2 * l2;
    let r0r1 = r0 * r1;
    let c4 = l2_squared + r1_squared;
    let c3 = 2.0 * (l1 * l2 + r0r1);
    let c2 = 2.0 * l0 * l2 + l1 * l1 - r1_squared + r0_squared;
    let c1 = 2.0 * (l0 * l1 - r0r1);
    let c0 = l0 * l0 - r0_squared;
    if c4 == 0.0 && c3 == 0.0 && c2 == 0.0 && c1 == 0.0 {
        return solutions;
    }
    let cosines = quartic_real_polynomial::compute_real_roots(c4, c3, c2, c1, c0);
    let length = cosines.len();
    let mut i = 0;
    while i < length {
        let cosine = cosines[i];
        let cosine_squared = cosine * cosine;
        let sine_squared = js_max(1.0 - cosine_squared, 0.0);
        let sine = sine_squared.sqrt();
        let left = if sign(l2) == sign(l0) {
            add_with_cancellation_check(l2 * cosine_squared + l0, l1 * cosine, EPSILON12)
        } else if sign(l0) == sign(l1 * cosine) {
            add_with_cancellation_check(l2 * cosine_squared, l1 * cosine + l0, EPSILON12)
        } else {
            add_with_cancellation_check(l2 * cosine_squared + l1 * cosine, l0, EPSILON12)
        };
        let right = add_with_cancellation_check(r1 * cosine, r0, EPSILON15);
        let product = left * right;
        if product < 0.0 {
            solutions.push(Cartesian3::new(x, w * cosine, w * sine));
        } else if product > 0.0 {
            solutions.push(Cartesian3::new(x, w * cosine, w * -sine));
        } else if sine != 0.0 {
            solutions.push(Cartesian3::new(x, w * cosine, w * -sine));
            solutions.push(Cartesian3::new(x, w * cosine, w * sine));
            i += 1;
        } else {
            solutions.push(Cartesian3::new(x, w * cosine, w * sine));
        }
        i += 1;
    }
    solutions
}

/// Point on the ellipsoid nearest the ray, raised to the ray's altitude
/// there (negative if the ray hits the ellipsoid). Returns the ray origin
/// when it points away from the surface.
pub fn grazing_altitude_location(ray: &Ray, ellipsoid: &Ellipsoid) -> Option<Cartesian3> {
    let position = ray.origin;
    let direction = ray.direction;
    if position != Cartesian3::ZERO {
        let normal = ellipsoid.geodetic_surface_normal(position)?;
        if direction.dot(normal) >= 0.0 {
            return Some(position);
        }
    }
    let intersects = ray_ellipsoid(ray, ellipsoid).is_some();
    let f = ellipsoid.transform_position_to_scaled_space(direction);
    let first_axis = f.normalize();
    let reference = first_axis.most_orthogonal_axis();
    let second_axis = reference.cross(first_axis).normalize();
    let third_axis = first_axis.cross(second_axis).normalize();
    let big_b = Matrix3([
        first_axis.x,
        first_axis.y,
        first_axis.z,
        second_axis.x,
        second_axis.y,
        second_axis.z,
        third_axis.x,
        third_axis.y,
        third_axis.z,
    ]);
    let b_t = big_b.transpose();
    let d_i = Matrix3::from_scale(ellipsoid.radii);
    let d = Matrix3::from_scale(ellipsoid.one_over_radii);
    let c = Matrix3([
        0.0,
        -direction.z,
        direction.y,
        direction.z,
        0.0,
        -direction.x,
        -direction.y,
        direction.x,
        0.0,
    ]);
    let temp = b_t.multiply(&d).multiply(&c);
    let a = temp.multiply(&d_i).multiply(&big_b);
    let b = temp.multiply_by_vector(position);
    let solutions = quadratic_vector_expression(&a, b.negate(), 0.0, 0.0, 1.0);
    if solutions.is_empty() {
        return None;
    }
    let mut closest = Cartesian3::ZERO;
    let mut maximum_value = f64::NEG_INFINITY;
    for solution in &solutions {
        let s = d_i.multiply_by_vector(big_b.multiply_by_vector(*solution));
        let v = s.subtract(position).normalize();
        let dot_product = v.dot(direction);
        if dot_product > maximum_value {
            maximum_value = dot_product;
            closest = s;
        }
    }
    let mut surface_point = ellipsoid.cartesian_to_cartographic(closest)?;
    maximum_value = clamp(maximum_value, 0.0, 1.0);
    let mut altitude = closest.subtract(position).magnitude() * (1.0 - maximum_value * maximum_value).sqrt();
    if intersects {
        altitude = -altitude;
    }
    surface_point.height = altitude;
    Some(ellipsoid.cartographic_to_cartesian(&surface_point))
}

/// Where the segment crosses the plane, or `None` if it does not.
pub fn line_segment_plane(end_point0: Cartesian3, end_point1: Cartesian3, plane: &Plane) -> Option<Cartesian3> {
    let difference = end_point1.subtract(end_point0);
    let normal = plane.normal;
    let n_dot_diff = normal.dot(difference);
    if n_dot_diff.abs() < EPSILON6 {
        return None;
    }
    let n_dot_p0 = normal.dot(end_point0);
    let t = -(plane.distance + n_dot_p0) / n_dot_diff;
    if !(0.0..=1.0).contains(&t) {
        return None;
    }
    Some(end_point0.add(difference.multiply_by_scalar(t)))
}

/// Splits a triangle by a plane. `None` unless the plane separates its vertices.
pub fn triangle_plane_intersection(
    p0: Cartesian3,
    p1: Cartesian3,
    p2: Cartesian3,
    plane: &Plane,
) -> Option<TrianglePlaneIntersection> {
    let n = plane.normal;
    let d = plane.distance;
    let p0_behind = n.dot(p0) + d < 0.0;
    let p1_behind = n.dot(p1) + d < 0.0;
    let p2_behind = n.dot(p2) + d < 0.0;
    let num_behind = u32::from(p0_behind) + u32::from(p1_behind) + u32::from(p2_behind);
    // Cesium leaves a split point at zero when the segment test misses.
    let seg = |a: Cartesian3, b: Cartesian3| line_segment_plane(a, b, plane).unwrap_or(Cartesian3::ZERO);
    let make = |u1: Cartesian3, u2: Cartesian3, indices: [u32; 9]| TrianglePlaneIntersection {
        positions: [p0, p1, p2, u1, u2],
        indices,
    };
    match num_behind {
        1 => {
            if p0_behind {
                Some(make(seg(p0, p1), seg(p0, p2), [0, 3, 4, 1, 2, 4, 1, 4, 3]))
            } else if p1_behind {
                Some(make(seg(p1, p2), seg(p1, p0), [1, 3, 4, 2, 0, 4, 2, 4, 3]))
            } else {
                Some(make(seg(p2, p0), seg(p2, p1), [2, 3, 4, 0, 1, 4, 0, 4, 3]))
            }
        }
        2 => {
            if !p0_behind {
                Some(make(seg(p1, p0), seg(p2, p0), [1, 2, 4, 1, 4, 3, 0, 3, 4]))
            } else if !p1_behind {
                Some(make(seg(p2, p1), seg(p0, p1), [2, 0, 4, 2, 4, 3, 1, 3, 4]))
            } else {
                Some(make(seg(p0, p2), seg(p1, p2), [0, 1, 4, 0, 4, 3, 2, 3, 4]))
            }
        }
        _ => None,
    }
}
