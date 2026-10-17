//! CesiumJS `EllipsoidalOccluder`: horizon culling against an ellipsoid.

use super::bounding_sphere::BoundingSphere;
use super::cartesian3::Cartesian3;
use super::ellipsoid::Ellipsoid;
use super::math::js_max;
use super::rectangle::Rectangle;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EllipsoidalOccluder {
    ellipsoid: Ellipsoid,
    camera_position: Cartesian3,
    camera_position_in_scaled_space: Cartesian3,
    distance_to_limb_in_scaled_space_squared: f64,
}

impl EllipsoidalOccluder {
    pub fn new(ellipsoid: &Ellipsoid, camera_position: Option<Cartesian3>) -> Self {
        let mut o = Self {
            ellipsoid: *ellipsoid,
            camera_position: Cartesian3::ZERO,
            camera_position_in_scaled_space: Cartesian3::ZERO,
            distance_to_limb_in_scaled_space_squared: 0.0,
        };
        if let Some(p) = camera_position {
            o.set_camera_position(p);
        }
        o
    }

    pub fn ellipsoid(&self) -> &Ellipsoid {
        &self.ellipsoid
    }

    pub fn camera_position(&self) -> Cartesian3 {
        self.camera_position
    }

    pub fn set_camera_position(&mut self, camera_position: Cartesian3) {
        let cv = self.ellipsoid.transform_position_to_scaled_space(camera_position);
        self.distance_to_limb_in_scaled_space_squared = cv.magnitude_squared() - 1.0;
        self.camera_position = camera_position;
        self.camera_position_in_scaled_space = cv;
    }

    pub fn is_point_visible(&self, occludee: Cartesian3) -> bool {
        is_scaled_space_point_visible(
            self.ellipsoid.transform_position_to_scaled_space(occludee),
            self.camera_position_in_scaled_space,
            self.distance_to_limb_in_scaled_space_squared,
        )
    }

    pub fn is_scaled_space_point_visible(&self, occludee_scaled_space_position: Cartesian3) -> bool {
        is_scaled_space_point_visible(
            occludee_scaled_space_position,
            self.camera_position_in_scaled_space,
            self.distance_to_limb_in_scaled_space_squared,
        )
    }

    /// Like [`Self::is_scaled_space_point_visible`], but tests against an
    /// ellipsoid shrunk by `minimum_height` when that is negative.
    pub fn is_scaled_space_point_visible_possibly_under_ellipsoid(
        &self,
        occludee: Cartesian3,
        minimum_height: Option<f64>,
    ) -> bool {
        let e = &self.ellipsoid;
        let (cv, vh) = match minimum_height {
            Some(h) if h < 0.0 && e.minimum_radius > -h => {
                let cv = Cartesian3::new(
                    self.camera_position.x / (e.radii.x + h),
                    self.camera_position.y / (e.radii.y + h),
                    self.camera_position.z / (e.radii.z + h),
                );
                (cv, cv.x * cv.x + cv.y * cv.y + cv.z * cv.z - 1.0)
            }
            _ => (self.camera_position_in_scaled_space, self.distance_to_limb_in_scaled_space_squared),
        };
        is_scaled_space_point_visible(occludee, cv, vh)
    }

    /// A point along `direction_to_point` that is visible only if one of
    /// `positions` might be. `None` if no such point exists.
    pub fn compute_horizon_culling_point(
        &self,
        direction_to_point: Cartesian3,
        positions: &[Cartesian3],
    ) -> Option<Cartesian3> {
        from_positions(&self.ellipsoid, direction_to_point, positions)
    }

    pub fn compute_horizon_culling_point_possibly_under_ellipsoid(
        &self,
        direction_to_point: Cartesian3,
        positions: &[Cartesian3],
        minimum_height: Option<f64>,
    ) -> Option<Cartesian3> {
        from_positions(&possibly_shrunk(&self.ellipsoid, minimum_height), direction_to_point, positions)
    }

    /// Vertices packed `x, y, z` every `stride` values, offset by `center`.
    pub fn compute_horizon_culling_point_from_vertices(
        &self,
        direction_to_point: Cartesian3,
        vertices: &[f64],
        stride: usize,
        center: Cartesian3,
    ) -> Option<Cartesian3> {
        from_vertices(&self.ellipsoid, direction_to_point, vertices, stride, center)
    }

    pub fn compute_horizon_culling_point_from_vertices_possibly_under_ellipsoid(
        &self,
        direction_to_point: Cartesian3,
        vertices: &[f64],
        stride: usize,
        center: Cartesian3,
        minimum_height: Option<f64>,
    ) -> Option<Cartesian3> {
        from_vertices(&possibly_shrunk(&self.ellipsoid, minimum_height), direction_to_point, vertices, stride, center)
    }

    pub fn compute_horizon_culling_point_from_rectangle(
        &self,
        rectangle: &Rectangle,
        ellipsoid: &Ellipsoid,
    ) -> Option<Cartesian3> {
        let positions = rectangle.subsample(ellipsoid, 0.0);
        let bs = BoundingSphere::from_points(&positions);
        if bs.center.magnitude() < 0.1 * ellipsoid.minimum_radius {
            return None;
        }
        self.compute_horizon_culling_point(bs.center, &positions)
    }
}

fn possibly_shrunk(ellipsoid: &Ellipsoid, minimum_height: Option<f64>) -> Ellipsoid {
    match minimum_height {
        Some(h) if h < 0.0 && ellipsoid.minimum_radius > -h => {
            Ellipsoid::new(ellipsoid.radii.x + h, ellipsoid.radii.y + h, ellipsoid.radii.z + h)
        }
        _ => *ellipsoid,
    }
}

fn scaled_space_direction(ellipsoid: &Ellipsoid, direction_to_point: Cartesian3) -> Cartesian3 {
    if direction_to_point == Cartesian3::ZERO {
        return direction_to_point;
    }
    ellipsoid.transform_position_to_scaled_space(direction_to_point).normalize()
}

fn compute_magnitude(ellipsoid: &Ellipsoid, position: Cartesian3, direction_to_point: Cartesian3) -> f64 {
    let scaled = ellipsoid.transform_position_to_scaled_space(position);
    let mut magnitude_squared = scaled.magnitude_squared();
    let mut magnitude = magnitude_squared.sqrt();
    let direction = scaled.divide_by_scalar(magnitude);
    magnitude_squared = js_max(1.0, magnitude_squared);
    magnitude = js_max(1.0, magnitude);
    let cos_alpha = direction.dot(direction_to_point);
    let sin_alpha = direction.cross(direction_to_point).magnitude();
    let cos_beta = 1.0 / magnitude;
    let sin_beta = (magnitude_squared - 1.0).sqrt() * cos_beta;
    1.0 / (cos_alpha * cos_beta - sin_alpha * sin_beta)
}

fn magnitude_to_point(direction_to_point: Cartesian3, magnitude: f64) -> Option<Cartesian3> {
    if magnitude <= 0.0 || magnitude == f64::INFINITY || magnitude.is_nan() {
        return None;
    }
    Some(direction_to_point.multiply_by_scalar(magnitude))
}

fn from_positions(
    ellipsoid: &Ellipsoid,
    direction_to_point: Cartesian3,
    positions: &[Cartesian3],
) -> Option<Cartesian3> {
    let direction = scaled_space_direction(ellipsoid, direction_to_point);
    let mut result_magnitude = 0.0;
    for p in positions {
        let candidate = compute_magnitude(ellipsoid, *p, direction);
        if candidate < 0.0 {
            return None;
        }
        result_magnitude = js_max(result_magnitude, candidate);
    }
    magnitude_to_point(direction, result_magnitude)
}

fn from_vertices(
    ellipsoid: &Ellipsoid,
    direction_to_point: Cartesian3,
    vertices: &[f64],
    stride: usize,
    center: Cartesian3,
) -> Option<Cartesian3> {
    let direction = scaled_space_direction(ellipsoid, direction_to_point);
    let mut result_magnitude = 0.0;
    for i in (0..vertices.len()).step_by(stride) {
        let p = Cartesian3::new(vertices[i] + center.x, vertices[i + 1] + center.y, vertices[i + 2] + center.z);
        let candidate = compute_magnitude(ellipsoid, p, direction);
        if candidate < 0.0 {
            return None;
        }
        result_magnitude = js_max(result_magnitude, candidate);
    }
    magnitude_to_point(direction, result_magnitude)
}

fn is_scaled_space_point_visible(occludee: Cartesian3, cv: Cartesian3, vh_magnitude_squared: f64) -> bool {
    let vt = occludee.subtract(cv);
    let vt_dot_vc = -vt.dot(cv);
    let is_occluded = if vh_magnitude_squared < 0.0 {
        vt_dot_vc > 0.0
    } else {
        vt_dot_vc > vh_magnitude_squared && (vt_dot_vc * vt_dot_vc) / vt.magnitude_squared() > vh_magnitude_squared
    };
    !is_occluded
}
