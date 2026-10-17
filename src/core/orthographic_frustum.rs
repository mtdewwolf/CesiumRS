//! CesiumJS `OrthographicFrustum`: a symmetric box view volume from a width
//! and aspect ratio.

use super::cartesian2::Cartesian2;
use super::cartesian3::Cartesian3;
use super::culling_volume::CullingVolume;
use super::math::equals_epsilon;
use super::matrix4::Matrix4;
use super::orthographic_off_center_frustum::OrthographicOffCenterFrustum;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct OrthographicFrustum {
    pub width: f64,
    pub aspect_ratio: f64,
    pub near: f64,
    pub far: f64,
}

impl OrthographicFrustum {
    pub const PACKED_LENGTH: usize = 4;

    /// Cesium's defaults: near 1, far 500,000,000.
    pub fn new(width: f64, aspect_ratio: f64) -> Self {
        Self { width, aspect_ratio, near: 1.0, far: 500_000_000.0 }
    }

    pub fn pack(&self, array: &mut [f64], starting_index: usize) {
        array[starting_index..starting_index + 4].copy_from_slice(&[
            self.width,
            self.aspect_ratio,
            self.near,
            self.far,
        ]);
    }

    pub fn unpack(array: &[f64], starting_index: usize) -> Self {
        let a = &array[starting_index..starting_index + 4];
        Self { width: a[0], aspect_ratio: a[1], near: a[2], far: a[3] }
    }

    pub fn off_center_frustum(&self) -> OrthographicOffCenterFrustum {
        let ratio = 1.0 / self.aspect_ratio;
        let right = self.width * 0.5;
        let top = ratio * right;
        OrthographicOffCenterFrustum { left: -right, right, top, bottom: -top, near: self.near, far: self.far }
    }

    pub fn projection_matrix(&self) -> Matrix4 {
        self.off_center_frustum().projection_matrix()
    }

    pub fn compute_culling_volume(&self, position: Cartesian3, direction: Cartesian3, up: Cartesian3) -> CullingVolume {
        self.off_center_frustum().compute_culling_volume(position, direction, up)
    }

    pub fn get_pixel_dimensions(&self, width: f64, height: f64, distance: f64, pixel_ratio: f64) -> Cartesian2 {
        self.off_center_frustum().get_pixel_dimensions(width, height, distance, pixel_ratio)
    }

    pub fn equals(&self, other: &Self) -> bool {
        self.width == other.width
            && self.aspect_ratio == other.aspect_ratio
            && self.off_center_frustum() == other.off_center_frustum()
    }

    pub fn equals_epsilon(&self, other: &Self, relative_epsilon: f64, absolute_epsilon: f64) -> bool {
        equals_epsilon(self.width, other.width, relative_epsilon, absolute_epsilon)
            && equals_epsilon(self.aspect_ratio, other.aspect_ratio, relative_epsilon, absolute_epsilon)
            && self.off_center_frustum().equals_epsilon(&other.off_center_frustum(), relative_epsilon, absolute_epsilon)
    }
}
