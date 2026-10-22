//! CesiumJS `DirectionalLight`: a light with a fixed world direction.

use super::Light;
use crate::core::{Cartesian3, Color};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DirectionalLight {
    pub direction: Cartesian3,
    pub color: Color,
    pub intensity: f64,
}

/// `new DirectionalLight(options)`: color defaults to white and intensity
/// to 1.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DirectionalLightOptions {
    pub direction: Cartesian3,
    pub color: Option<Color>,
    pub intensity: Option<f64>,
}

impl DirectionalLight {
    /// `None` for a zero direction, where Cesium throws.
    pub fn new(options: DirectionalLightOptions) -> Option<Self> {
        if options.direction == Cartesian3::ZERO {
            return None;
        }
        Some(Self {
            direction: options.direction,
            color: options.color.unwrap_or(Color::WHITE),
            intensity: options.intensity.unwrap_or(1.0),
        })
    }
}

impl Light for DirectionalLight {
    fn color(&self) -> Color {
        self.color
    }

    fn intensity(&self) -> f64 {
        self.intensity
    }
}
