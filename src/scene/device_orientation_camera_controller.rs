//! Device orientation deltas applied to the camera's local basis.
//!
//! The host forwards `deviceorientation` angles (degrees) to `orientation`;
//! owning/removing the platform listener stays with the host.
use crate::{
    core::{Matrix3, Quaternion, RuntimeError},
    scene::Camera,
};

#[derive(Default)]
pub struct DeviceOrientationCameraController {
    angles: Option<[f64; 3]>,
    last: Option<[f64; 3]>,
    destroyed: bool,
}
impl DeviceOrientationCameraController {
    pub fn new() -> Self {
        Self::default()
    }
    /// A missing alpha invalidates the current reading, as in Cesium. It does
    /// not erase the previous reading used when sensor data resumes.
    pub fn orientation(&mut self, alpha: Option<f64>, beta: f64, gamma: f64) -> Result<(), RuntimeError> {
        self.alive()?;
        self.angles = alpha.map(|a| [a.to_radians(), beta.to_radians(), gamma.to_radians()]);
        Ok(())
    }
    pub fn update(&mut self, camera: &mut Camera) -> Result<(), RuntimeError> {
        self.alive()?;
        if let Some(current) = self.angles {
            let last = self.last.unwrap_or(current);
            let a = current[0] - last[0];
            let b = last[1] - current[1];
            let g = last[2] - current[2];
            let rotation = Quaternion::from_axis_angle(camera.up, a).multiply(
                &Quaternion::from_axis_angle(camera.right, g)
                    .multiply(&Quaternion::from_axis_angle(camera.direction, b)),
            );
            let m = Matrix3::from_quaternion(&rotation);
            camera.right = m.multiply_by_vector(camera.right);
            camera.up = m.multiply_by_vector(camera.up);
            camera.direction = m.multiply_by_vector(camera.direction);
            self.last = Some(current);
        }
        Ok(())
    }
    pub fn is_destroyed(&self) -> bool {
        self.destroyed
    }
    pub fn destroy(&mut self) {
        self.destroyed = true;
        self.angles = None;
    }
    fn alive(&self) -> Result<(), RuntimeError> {
        if self.destroyed {
            Err(RuntimeError::new("This object was destroyed, i.e., destroy() was called."))
        } else {
            Ok(())
        }
    }
}
