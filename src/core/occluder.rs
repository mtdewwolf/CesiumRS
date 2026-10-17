//! CesiumJS `Occluder`: a sphere that hides what is behind it from a camera.

use super::bounding_sphere::BoundingSphere;
use super::cartesian3::Cartesian3;
use super::ellipsoid::Ellipsoid;
use super::math::EPSILON13;
use super::rectangle::Rectangle;
use super::visibility::Visibility;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Occluder {
    occluder_position: Cartesian3,
    occluder_radius: f64,
    horizon_distance: f64,
    horizon_plane_normal: Option<Cartesian3>,
    horizon_plane_position: Option<Cartesian3>,
    camera_position: Cartesian3,
}

impl Occluder {
    pub fn new(occluder: &BoundingSphere, camera_position: Cartesian3) -> Self {
        let mut o = Self {
            occluder_position: occluder.center,
            occluder_radius: occluder.radius,
            horizon_distance: 0.0,
            horizon_plane_normal: None,
            horizon_plane_position: None,
            camera_position,
        };
        o.set_camera_position(camera_position);
        o
    }

    pub fn from_bounding_sphere(occluder: &BoundingSphere, camera_position: Cartesian3) -> Self {
        Self::new(occluder, camera_position)
    }

    pub fn position(&self) -> Cartesian3 {
        self.occluder_position
    }

    pub fn radius(&self) -> f64 {
        self.occluder_radius
    }

    pub fn camera_position(&self) -> Cartesian3 {
        self.camera_position
    }

    pub fn set_camera_position(&mut self, camera_position: Cartesian3) {
        let camera_to_occluder = self.occluder_position.subtract(camera_position);
        let mut inv_distance = camera_to_occluder.magnitude_squared();
        let occluder_radius_sqrd = self.occluder_radius * self.occluder_radius;
        let (horizon_distance, normal, position) = if inv_distance > occluder_radius_sqrd {
            let horizon_distance = (inv_distance - occluder_radius_sqrd).sqrt();
            inv_distance = 1.0 / inv_distance.sqrt();
            let normal = camera_to_occluder.multiply_by_scalar(inv_distance);
            let near_plane_distance = horizon_distance * horizon_distance * inv_distance;
            let position = camera_position.add(normal.multiply_by_scalar(near_plane_distance));
            (horizon_distance, Some(normal), Some(position))
        } else {
            (f64::MAX, None, None)
        };
        self.horizon_distance = horizon_distance;
        self.horizon_plane_normal = normal;
        self.horizon_plane_position = position;
        self.camera_position = camera_position;
    }

    pub fn is_point_visible(&self, occludee: Cartesian3) -> bool {
        if self.horizon_distance != f64::MAX {
            let temp_vec = occludee.subtract(self.occluder_position);
            let mut temp = self.occluder_radius;
            temp = temp_vec.magnitude_squared() - temp * temp;
            if temp > 0.0 {
                temp = temp.sqrt() + self.horizon_distance;
                let to_camera = occludee.subtract(self.camera_position);
                return temp * temp > to_camera.magnitude_squared();
            }
        }
        false
    }

    pub fn is_bounding_sphere_visible(&self, occludee: &BoundingSphere) -> bool {
        let occludee_position = occludee.center;
        let occludee_radius = occludee.radius;
        if self.horizon_distance != f64::MAX {
            let temp_vec = occludee_position.subtract(self.occluder_position);
            let mut temp = self.occluder_radius - occludee_radius;
            temp = temp_vec.magnitude_squared() - temp * temp;
            if occludee_radius < self.occluder_radius {
                if temp > 0.0 {
                    temp = temp.sqrt() + self.horizon_distance;
                    let to_camera = occludee_position.subtract(self.camera_position);
                    return temp * temp + occludee_radius * occludee_radius > to_camera.magnitude_squared();
                }
                return false;
            }
            if temp > 0.0 {
                let to_camera = occludee_position.subtract(self.camera_position);
                let to_camera_squared = to_camera.magnitude_squared();
                let occluder_radius_squared = self.occluder_radius * self.occluder_radius;
                let occludee_radius_squared = occludee_radius * occludee_radius;
                if (self.horizon_distance * self.horizon_distance + occluder_radius_squared) * occludee_radius_squared
                    > to_camera_squared * occluder_radius_squared
                {
                    return true;
                }
                temp = temp.sqrt() + self.horizon_distance;
                return temp * temp + occludee_radius_squared > to_camera_squared;
            }
            return true;
        }
        false
    }

    pub fn compute_visibility(&self, occludee: &BoundingSphere) -> Visibility {
        let occludee_position = occludee.center;
        let occludee_radius = occludee.radius;
        if occludee_radius > self.occluder_radius {
            return Visibility::Full;
        }
        if self.horizon_distance != f64::MAX {
            let temp_vec = occludee_position.subtract(self.occluder_position);
            let mut temp = self.occluder_radius - occludee_radius;
            let occluder_to_occludee_dist_sqrd = temp_vec.magnitude_squared();
            temp = occluder_to_occludee_dist_sqrd - temp * temp;
            if temp > 0.0 {
                temp = temp.sqrt() + self.horizon_distance;
                let camera_to_occludee_dist_sqrd = occludee_position.subtract(self.camera_position).magnitude_squared();
                if temp * temp + occludee_radius * occludee_radius < camera_to_occludee_dist_sqrd {
                    return Visibility::None;
                }
                temp = self.occluder_radius + occludee_radius;
                temp = occluder_to_occludee_dist_sqrd - temp * temp;
                if temp > 0.0 {
                    temp = temp.sqrt() + self.horizon_distance;
                    return if camera_to_occludee_dist_sqrd < temp * temp + occludee_radius * occludee_radius {
                        Visibility::Full
                    } else {
                        Visibility::Partial
                    };
                }
                let plane_position = self.horizon_plane_position.unwrap_or_default();
                let plane_normal = self.horizon_plane_normal.unwrap_or_default();
                return if occludee_position.subtract(plane_position).dot(plane_normal) > -occludee_radius {
                    Visibility::Partial
                } else {
                    Visibility::Full
                };
            }
        }
        Visibility::None
    }

    /// A point that, when hidden by the occluder, guarantees every one of
    /// `positions` is hidden. `None` if no such point exists.
    pub fn compute_occludee_point(
        occluder: &BoundingSphere,
        occludee_position: Cartesian3,
        positions: &[Cartesian3],
    ) -> Option<Cartesian3> {
        let occluder_position = occluder.center;
        let occluder_radius = occluder.radius;
        if positions.is_empty() || occluder_position == occludee_position {
            return None;
        }
        let plane_normal = occludee_position.subtract(occluder_position).normalize();
        let plane_d = -plane_normal.dot(occluder_position);
        let any_rotation_vector = any_rotation_vector(occluder_position, plane_normal, plane_d);
        // Cesium treats a dot of exactly 0 (or NaN) like a failure, since it tests `!dot`.
        let usable = |d: Option<f64>| d.filter(|v| *v != 0.0 && !v.is_nan());
        let mut dot = usable(horizon_to_plane_normal_dot_product(
            occluder,
            plane_normal,
            plane_d,
            any_rotation_vector,
            positions[0],
        ))?;
        for p in &positions[1..] {
            let temp_dot =
                usable(horizon_to_plane_normal_dot_product(occluder, plane_normal, plane_d, any_rotation_vector, *p))?;
            if temp_dot < dot {
                dot = temp_dot;
            }
        }
        if dot < 0.001_745_328_365_898_308_835_778_202_720_85 {
            return None;
        }
        let distance = occluder_radius / dot;
        Some(occluder_position.add(plane_normal.multiply_by_scalar(distance)))
    }

    /// [`Self::compute_occludee_point`] for a rectangle on the ellipsoid.
    pub fn compute_occludee_point_from_rectangle(rectangle: &Rectangle, ellipsoid: &Ellipsoid) -> Option<Cartesian3> {
        let positions = rectangle.subsample(ellipsoid, 0.0);
        let bs = BoundingSphere::from_points(&positions);
        if bs.center == Cartesian3::ZERO {
            return None;
        }
        Self::compute_occludee_point(
            &BoundingSphere::new(Cartesian3::ZERO, ellipsoid.minimum_radius),
            bs.center,
            &positions,
        )
    }
}

fn any_rotation_vector(occluder_position: Cartesian3, plane_normal: Cartesian3, plane_d: f64) -> Cartesian3 {
    let a = plane_normal.abs();
    let mut major_axis = if a.x > a.y { 0 } else { 1 };
    if (major_axis == 0 && a.z > a.x) || (major_axis == 1 && a.z > a.y) {
        major_axis = 2;
    }
    let (temp_vec0, temp_vec1) = match major_axis {
        0 => (
            Cartesian3::new(occluder_position.x, occluder_position.y + 1.0, occluder_position.z + 1.0),
            Cartesian3::UNIT_X,
        ),
        1 => (
            Cartesian3::new(occluder_position.x + 1.0, occluder_position.y, occluder_position.z + 1.0),
            Cartesian3::UNIT_Y,
        ),
        _ => (
            Cartesian3::new(occluder_position.x + 1.0, occluder_position.y + 1.0, occluder_position.z),
            Cartesian3::UNIT_Z,
        ),
    };
    let u = (plane_normal.dot(temp_vec0) + plane_d) / -plane_normal.dot(temp_vec1);
    temp_vec0.add(temp_vec1.multiply_by_scalar(u)).subtract(occluder_position).normalize()
}

fn rotation_vector(
    occluder_position: Cartesian3,
    plane_normal: Cartesian3,
    position: Cartesian3,
    any_rotation_vector: Cartesian3,
) -> Cartesian3 {
    let position_direction = position.subtract(occluder_position).normalize();
    if plane_normal.dot(position_direction) < 0.999_999_984_769_129_049_327_808_509_034_44 {
        let cross_product = plane_normal.cross(position_direction);
        if cross_product.magnitude() > EPSILON13 {
            return cross_product.normalize();
        }
    }
    any_rotation_vector
}

/// `None` when `position` is inside the occluder (Cesium returns `false`).
fn horizon_to_plane_normal_dot_product(
    occluder: &BoundingSphere,
    plane_normal: Cartesian3,
    _plane_d: f64,
    any_rotation_vector: Cartesian3,
    position: Cartesian3,
) -> Option<f64> {
    let pos = position;
    let occluder_position = occluder.center;
    let occluder_radius = occluder.radius;
    let position_to_occluder = occluder_position.subtract(pos);
    let distance_squared = position_to_occluder.magnitude_squared();
    let occluder_radius_squared = occluder_radius * occluder_radius;
    if distance_squared < occluder_radius_squared {
        return None;
    }
    let horizon_distance_squared = distance_squared - occluder_radius_squared;
    let horizon_distance = horizon_distance_squared.sqrt();
    let occluder_to_position_distance = distance_squared.sqrt();
    let inv = 1.0 / occluder_to_position_distance;
    let cos_theta = horizon_distance * inv;
    let horizon_plane_distance = cos_theta * horizon_distance;
    let position_to_occluder = position_to_occluder.normalize();
    let horizon_plane_position = pos.add(position_to_occluder.multiply_by_scalar(horizon_plane_distance));
    let horizon_cross_distance = (horizon_distance_squared - horizon_plane_distance * horizon_plane_distance).sqrt();
    let t = rotation_vector(occluder_position, plane_normal, pos, any_rotation_vector);
    let p = position_to_occluder;
    let horizon_cross_direction = Cartesian3::new(
        t.x * t.x * p.x + (t.x * t.y - t.z) * p.y + (t.x * t.z + t.y) * p.z,
        (t.x * t.y + t.z) * p.x + t.y * t.y * p.y + (t.y * t.z - t.x) * p.z,
        (t.x * t.z - t.y) * p.x + (t.y * t.z + t.x) * p.y + t.z * t.z * p.z,
    )
    .normalize();
    let offset = horizon_cross_direction.multiply_by_scalar(horizon_cross_distance);
    let v0 = horizon_plane_position.add(offset).subtract(occluder_position).normalize();
    let dot0 = plane_normal.dot(v0);
    let v1 = horizon_plane_position.subtract(offset).subtract(occluder_position).normalize();
    let dot1 = plane_normal.dot(v1);
    Some(if dot0 < dot1 { dot0 } else { dot1 })
}
