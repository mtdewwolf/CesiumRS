//! CesiumJS `PerspectiveOffCenterFrustum`: an asymmetric view frustum given
//! by the near-plane extents.

use super::cartesian2::Cartesian2;
use super::cartesian3::Cartesian3;
use super::cartesian4::Cartesian4;
use super::culling_volume::CullingVolume;
use super::math::equals_epsilon;
use super::matrix4::Matrix4;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PerspectiveOffCenterFrustum {
    pub left: f64,
    pub right: f64,
    pub top: f64,
    pub bottom: f64,
    /// Cesium default 1.0.
    pub near: f64,
    /// Cesium default 500,000,000.
    pub far: f64,
}

pub(crate) fn plane(n: Cartesian3, w: f64) -> Cartesian4 {
    Cartesian4::new(n.x, n.y, n.z, w)
}

impl PerspectiveOffCenterFrustum {
    pub fn new(left: f64, right: f64, top: f64, bottom: f64, near: f64, far: f64) -> Self {
        Self { left, right, top, bottom, near, far }
    }

    pub fn projection_matrix(&self) -> Matrix4 {
        Matrix4::compute_perspective_off_center(self.left, self.right, self.bottom, self.top, self.near, self.far)
    }

    pub fn infinite_projection_matrix(&self) -> Matrix4 {
        Matrix4::compute_infinite_perspective_off_center(self.left, self.right, self.bottom, self.top, self.near)
    }

    /// Left, right, bottom, top, near, and far planes for a camera at
    /// `position` looking along `direction` with `up`.
    pub fn compute_culling_volume(&self, position: Cartesian3, direction: Cartesian3, up: Cartesian3) -> CullingVolume {
        let (t, b, r, l, n, f) = (self.top, self.bottom, self.right, self.left, self.near, self.far);
        let right = direction.cross(up);
        let near_center = position.add(direction.multiply_by_scalar(n));
        let far_center = position.add(direction.multiply_by_scalar(f));
        let mut planes = Vec::with_capacity(6);

        let normal = near_center.add(right.multiply_by_scalar(l)).subtract(position).normalize().cross(up).normalize();
        planes.push(plane(normal, -normal.dot(position)));

        let normal = up.cross(near_center.add(right.multiply_by_scalar(r)).subtract(position)).normalize();
        planes.push(plane(normal, -normal.dot(position)));

        let normal = right.cross(near_center.add(up.multiply_by_scalar(b)).subtract(position)).normalize();
        planes.push(plane(normal, -normal.dot(position)));

        let normal = near_center.add(up.multiply_by_scalar(t)).subtract(position).cross(right).normalize();
        planes.push(plane(normal, -normal.dot(position)));

        planes.push(plane(direction, -direction.dot(near_center)));
        let normal = direction.negate();
        planes.push(plane(normal, -normal.dot(far_center)));
        CullingVolume::new(planes)
    }

    /// Size of one pixel at `distance` from the camera, in meters.
    pub fn get_pixel_dimensions(
        &self,
        drawing_buffer_width: f64,
        drawing_buffer_height: f64,
        distance: f64,
        pixel_ratio: f64,
    ) -> Cartesian2 {
        let inverse_near = 1.0 / self.near;
        let mut tan_theta = self.top * inverse_near;
        let pixel_height = (2.0 * pixel_ratio * distance * tan_theta) / drawing_buffer_height;
        tan_theta = self.right * inverse_near;
        let pixel_width = (2.0 * pixel_ratio * distance * tan_theta) / drawing_buffer_width;
        Cartesian2::new(pixel_width, pixel_height)
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
