//! CesiumJS Fog density and culling parameters.
use super::SceneMode;
use crate::core::{Cartesian3, RuntimeError};
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct FrameFog {
    pub enabled: bool,
    pub renderable: bool,
    pub density: f64,
    pub visual_density_scalar: f64,
    pub sse: f64,
    pub minimum_brightness: f64,
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Fog {
    pub enabled: bool,
    pub renderable: bool,
    pub density: f64,
    pub height_scalar: f64,
    height_falloff: f64,
    pub max_height: f64,
    pub visual_density_scalar: f64,
    pub screen_space_error_factor: f64,
    pub minimum_brightness: f64,
}
impl Default for Fog {
    fn default() -> Self {
        Self {
            enabled: true,
            renderable: true,
            density: 0.0006,
            height_scalar: 0.001,
            height_falloff: 0.59,
            max_height: 800000.,
            visual_density_scalar: 0.15,
            screen_space_error_factor: 2.,
            minimum_brightness: 0.03,
        }
    }
}
impl Fog {
    pub fn height_falloff(&self) -> f64 {
        self.height_falloff
    }
    pub fn set_height_falloff(&mut self, value: f64) -> Result<(), RuntimeError> {
        if value < 0. {
            return Err(RuntimeError::new("value must be positive."));
        }
        self.height_falloff = value;
        Ok(())
    }
    pub fn update(
        &self,
        frame: &mut FrameFog,
        mode: SceneMode,
        height: Option<f64>,
        position: Cartesian3,
        direction: Cartesian3,
    ) {
        frame.enabled = self.enabled;
        if !self.enabled {
            return;
        }
        frame.renderable = self.renderable;
        if height.is_none() || height.is_some_and(|h| h > self.max_height) || mode != SceneMode::Scene3D {
            frame.enabled = false;
            frame.density = 0.;
            return;
        }
        let height = height.unwrap();
        let mut density =
            self.density * self.height_scalar * (height / self.max_height).max(1e-4).powf(-self.height_falloff.max(0.));
        density *= 1. - direction.dot(position.normalize()).abs();
        frame.density = density;
        frame.visual_density_scalar = self.visual_density_scalar;
        frame.sse = self.screen_space_error_factor;
        frame.minimum_brightness = self.minimum_brightness;
    }
}
