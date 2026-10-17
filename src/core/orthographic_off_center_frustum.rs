//! CesiumJS `OrthographicOffCenterFrustum`: a box-shaped view volume.

use super::cartesian2::Cartesian2;
use super::cartesian3::Cartesian3;
use super::culling_volume::CullingVolume;
use super::math::equals_epsilon;
use super::matrix4::Matrix4;
use super::perspective_off_center_frustum::plane;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OrthographicOffCenterFrustum {
    pub left: f64,
    pub right: f64,
    pub top: f64,
    pub bottom: f64,
    pub near: f64,
    pub far: f64,
}

impl OrthographicOffCenterFrustum {
    pub fn new(left: f64, right: f64, top: f64, bottom: f64, near: f64, far: f64) -> Self {
        Self { left, right, top, bottom, near, far }
    }

    pub fn projection_matrix(&self) -> Matrix4 {
        Matrix4::compute_orthographic_off_center(self.left, self.right, self.bottom, self.top, self.near, self.far)
    }

    pub fn compute_culling_volume(&self, position: Cartesian3, direction: Cartesian3, up: Cartesian3) -> CullingVolume {
        let (t, b, r, l, n, f) = (self.top, self.bottom, self.right, self.left, self.near, self.far);
        let right = direction.cross(up).normalize();
        let near_center = position.add(direction.multiply_by_scalar(n));
        let mut planes = Vec::with_capacity(6);

        let point = near_center.add(right.multiply_by_scalar(l));
        planes.push(plane(right, -right.dot(point)));

        let point = near_center.add(right.multiply_by_scalar(r));
        let neg = right.negate();
        planes.push(plane(neg, -neg.dot(point)));

        let point = near_center.add(up.multiply_by_scalar(b));
        planes.push(plane(up, -up.dot(point)));

        let point = near_center.add(up.multiply_by_scalar(t));
        let neg = up.negate();
        planes.push(plane(neg, -neg.dot(point)));

        planes.push(plane(direction, -direction.dot(near_center)));
        let point = position.add(direction.multiply_by_scalar(f));
        let neg = direction.negate();
        planes.push(plane(neg, -neg.dot(point)));
        CullingVolume::new(planes)
    }

    pub fn get_pixel_dimensions(
        &self,
        drawing_buffer_width: f64,
        drawing_buffer_height: f64,
        _distance: f64,
        pixel_ratio: f64,
    ) -> Cartesian2 {
        let frustum_width = self.right - self.left;
        let frustum_height = self.top - self.bottom;
        Cartesian2::new(
            (pixel_ratio * frustum_width) / drawing_buffer_width,
            (pixel_ratio * frustum_height) / drawing_buffer_height,
        )
    }

    pub fn equals(&self, other: &Self) -> bool {
        self == other
    }

    pub fn equals_epsilon(&self, other: &Self, relative_epsilon: f64, absolute_epsilon: f64) -> bool {
        let e = |a: f64, b: f64| equals_epsilon(a, b, relative_epsilon, absolute_epsilon);
        e(self.right, other.right)
            && e(self.left, other.left)
            && e(self.top, other.top)
            && e(self.bottom, other.bottom)
            && e(self.near, other.near)
            && e(self.far, other.far)
    }
}
