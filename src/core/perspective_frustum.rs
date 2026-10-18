//! CesiumJS `PerspectiveFrustum`: a symmetric view frustum from a field of
//! view and aspect ratio, with optional off-center offsets.

use super::cartesian2::Cartesian2;
use super::cartesian3::Cartesian3;
use super::culling_volume::CullingVolume;
use super::js_math::JsMath;
use super::math::equals_epsilon;
use super::matrix4::Matrix4;
use super::perspective_off_center_frustum::PerspectiveOffCenterFrustum;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PerspectiveFrustum {
    /// Field of view in radians, across the larger of width and height.
    pub fov: f64,
    pub aspect_ratio: f64,
    pub near: f64,
    pub far: f64,
    pub x_offset: f64,
    pub y_offset: f64,
}

impl PerspectiveFrustum {
    pub const PACKED_LENGTH: usize = 6;

    /// Cesium's defaults: near 1, far 500,000,000, no offset.
    pub fn new(fov: f64, aspect_ratio: f64) -> Self {
        Self { fov, aspect_ratio, near: 1.0, far: 500_000_000.0, x_offset: 0.0, y_offset: 0.0 }
    }

    pub fn pack(&self, array: &mut [f64], starting_index: usize) {
        array[starting_index..starting_index + 6].copy_from_slice(&[
            self.fov,
            self.aspect_ratio,
            self.near,
            self.far,
            self.x_offset,
            self.y_offset,
        ]);
    }

    pub fn unpack(array: &[f64], starting_index: usize) -> Self {
        let a = &array[starting_index..starting_index + 6];
        Self { fov: a[0], aspect_ratio: a[1], near: a[2], far: a[3], x_offset: a[4], y_offset: a[5] }
    }

    /// Vertical field of view.
    pub fn fovy(&self) -> f64 {
        if self.aspect_ratio <= 1.0 {
            self.fov
        } else {
            ((self.fov * 0.5).js_tan() / self.aspect_ratio).js_atan() * 2.0
        }
    }

    /// `2 tan(fovy / 2)`, used in screen-space-error computations.
    pub fn sse_denominator(&self) -> f64 {
        2.0 * (0.5 * self.fovy()).js_tan()
    }

    pub fn off_center_frustum(&self) -> PerspectiveOffCenterFrustum {
        let top = self.near * (0.5 * self.fovy()).js_tan();
        let bottom = -top;
        let right = self.aspect_ratio * top;
        let left = -right;
        PerspectiveOffCenterFrustum {
            left: left + self.x_offset,
            right: right + self.x_offset,
            top: top + self.y_offset,
            bottom: bottom + self.y_offset,
            near: self.near,
            far: self.far,
        }
    }

    pub fn projection_matrix(&self) -> Matrix4 {
        self.off_center_frustum().projection_matrix()
    }

    pub fn infinite_projection_matrix(&self) -> Matrix4 {
        self.off_center_frustum().infinite_projection_matrix()
    }

    pub fn compute_culling_volume(&self, position: Cartesian3, direction: Cartesian3, up: Cartesian3) -> CullingVolume {
        self.off_center_frustum().compute_culling_volume(position, direction, up)
    }

    pub fn get_pixel_dimensions(&self, width: f64, height: f64, distance: f64, pixel_ratio: f64) -> Cartesian2 {
        self.off_center_frustum().get_pixel_dimensions(width, height, distance, pixel_ratio)
    }

    pub fn equals(&self, other: &Self) -> bool {
        self.fov == other.fov
            && self.aspect_ratio == other.aspect_ratio
            && self.off_center_frustum() == other.off_center_frustum()
    }

    pub fn equals_epsilon(&self, other: &Self, relative_epsilon: f64, absolute_epsilon: f64) -> bool {
        equals_epsilon(self.fov, other.fov, relative_epsilon, absolute_epsilon)
            && equals_epsilon(self.aspect_ratio, other.aspect_ratio, relative_epsilon, absolute_epsilon)
            && self.off_center_frustum().equals_epsilon(&other.off_center_frustum(), relative_epsilon, absolute_epsilon)
    }
}
