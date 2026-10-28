//! CesiumJS `DepthPlane`: computes horizon depth quad for the ellipsoid
//! and renders depth-only to prevent rendering behind the globe.

use super::camera::{Camera, CameraFrustum};
use super::frame_state::FrameState;
use super::scene::SceneCommand;
use super::scene_mode::SceneMode;
use crate::core::{BoundingSphere, Cartesian3, Ellipsoid};
use crate::renderer::render_state::{ColorMask, Cull, DepthTest};
use crate::renderer::{Pass, RenderState};
use std::rc::Rc;

/// Computes the depth quad coordinates for an ellipsoid horizon from the camera view.
/// Returns 12 `f32` numbers representing the 4 corners:
/// `[upperLeft(3), lowerLeft(3), upperRight(3), lowerRight(3)]`.
pub fn compute_depth_quad(ellipsoid: &Ellipsoid, camera: &mut Camera) -> [f32; 12] {
    let radii = ellipsoid.radii;
    let is_ortho = matches!(
        camera.frustum,
        CameraFrustum::Orthographic(_) | CameraFrustum::OrthographicOffCenter(_)
    );

    let (center, east_offset, north_offset) = if is_ortho {
        (
            Cartesian3::ZERO,
            camera.right_wc(),
            camera.up_wc(),
        )
    } else {
        let p = camera.position_wc();
        let q = Cartesian3::multiply_components(&ellipsoid.one_over_radii, p);
        let q_magnitude = q.magnitude();

        if q_magnitude <= 1.0 {
            // Camera is inside or on the ellipsoid surface
            (Cartesian3::ZERO, camera.right_wc(), camera.up_wc())
        } else {
            let q_unit = q.normalize();
            let e_unit = Cartesian3::cross(&Cartesian3::UNIT_Z, q).normalize();
            let n_unit = Cartesian3::cross(&q_unit, e_unit).normalize();
            let w_magnitude = (q_magnitude * q_magnitude - 1.0).sqrt();

            let center = q_unit.multiply_by_scalar(1.0 / q_magnitude);
            let scalar = w_magnitude / q_magnitude;
            let east_offset = e_unit.multiply_by_scalar(scalar);
            let north_offset = n_unit.multiply_by_scalar(scalar);
            (center, east_offset, north_offset)
        }
    };

    let upper_left = Cartesian3::multiply_components(
        &radii,
        center.add(north_offset).subtract(east_offset),
    );
    let lower_left = Cartesian3::multiply_components(
        &radii,
        center.subtract(north_offset).subtract(east_offset),
    );
    let upper_right = Cartesian3::multiply_components(
        &radii,
        center.add(north_offset).add(east_offset),
    );
    let lower_right = Cartesian3::multiply_components(
        &radii,
        center.subtract(north_offset).add(east_offset),
    );

    [
        upper_left.x as f32,
        upper_left.y as f32,
        upper_left.z as f32,
        lower_left.x as f32,
        lower_left.y as f32,
        lower_left.z as f32,
        upper_right.x as f32,
        upper_right.y as f32,
        upper_right.z as f32,
        lower_right.x as f32,
        lower_right.y as f32,
        lower_right.z as f32,
    ]
}

/// Renders a depth-only plane behind the globe to mask objects on the far side of the Earth.
pub struct DepthPlane {
    pub ellipsoid_offset: f64,
    pub mode: Option<SceneMode>,
    pub use_log_depth: bool,
    pub render_state: RenderState,
    pub bounding_volume: Option<BoundingSphere>,
    pub command: Option<SceneCommand>,
    destroyed: bool,
}

impl Default for DepthPlane {
    fn default() -> Self {
        Self::new(0.0)
    }
}

impl DepthPlane {
    pub fn new(ellipsoid_offset: f64) -> Self {
        let render_state = RenderState {
            cull: Cull {
                enabled: true,
                ..Default::default()
            },
            depth_test: DepthTest {
                enabled: true,
                ..Default::default()
            },
            color_mask: ColorMask {
                red: false,
                green: false,
                blue: false,
                alpha: false,
            },
            ..Default::default()
        };

        Self {
            ellipsoid_offset,
            mode: None,
            use_log_depth: false,
            render_state,
            bounding_volume: None,
            command: None,
            destroyed: false,
        }
    }

    pub fn update(&mut self, camera: &mut Camera, frame_state: &FrameState) {
        self.mode = Some(frame_state.mode);
        if frame_state.mode != SceneMode::Scene3D {
            return;
        }

        let radii = frame_state.uniform.map_projection.ellipsoid().radii;
        let ellipsoid = Ellipsoid::new(
            radii.x + self.ellipsoid_offset,
            radii.y + self.ellipsoid_offset,
            radii.z + self.ellipsoid_offset,
        );

        let _depth_quad = compute_depth_quad(&ellipsoid, camera);

        if self.command.is_none() {
            let bounding_volume =
                BoundingSphere::new(Cartesian3::ZERO, ellipsoid.maximum_radius);
            self.bounding_volume = Some(bounding_volume);

            let execute: super::scene::SceneExecute = Rc::new(|_| Err(crate::core::RuntimeError::new("DepthPlane rendering is not implemented.")));
            let command = SceneCommand::new("DepthPlane", Pass::Opaque, execute);
            self.command = Some(command);
        }
    }

    pub fn is_destroyed(&self) -> bool {
        self.destroyed
    }

    pub fn destroy(&mut self) {
        self.command = None;
        self.destroyed = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::Cartographic;
    use crate::scene::camera::CameraOptions;

    #[test]
    fn default_depth_plane_properties() {
        let dp = DepthPlane::default();
        assert_eq!(dp.ellipsoid_offset, 0.0);
        assert!(dp.render_state.cull.enabled);
        assert!(dp.render_state.depth_test.enabled);
        assert!(!dp.render_state.color_mask.red);
        assert!(!dp.render_state.color_mask.alpha);
        assert!(!dp.is_destroyed());
    }

    #[test]
    fn computes_depth_quad_from_orbit() {
        let ellipsoid = Ellipsoid::WGS84;
        let mut camera = Camera::new(CameraOptions::new(1024.0, 768.0));
        // Place camera above the equator looking down at center
        let pos = ellipsoid.cartographic_to_cartesian(&Cartographic::from_degrees(0.0, 0.0, 10_000_000.0));
        camera.position = pos;
        camera.direction = Cartesian3::new(-1.0, 0.0, 0.0);
        camera.up = Cartesian3::UNIT_Z;
        camera.right = Cartesian3::UNIT_Y;

        let quad = compute_depth_quad(&ellipsoid, &mut camera);
        assert_eq!(quad.len(), 12);

        // Quad corners should be non-zero and finite
        for val in quad {
            assert!(val.is_finite());
        }
    }
}
