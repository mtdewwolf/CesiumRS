//! CesiumJS `SunLight`: a directional light whose direction follows the sun.

use super::Light;
use crate::core::Color;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SunLight {
    pub color: Color,
    pub intensity: f64,
}

/// `new SunLight(options)`: unset fields default to white at intensity 2.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct SunLightOptions {
    pub color: Option<Color>,
    pub intensity: Option<f64>,
}

impl SunLight {
    pub fn new(options: SunLightOptions) -> Self {
        Self { color: options.color.unwrap_or(Color::WHITE), intensity: options.intensity.unwrap_or(2.0) }
    }
}

impl Default for SunLight {
    fn default() -> Self {
        Self::new(SunLightOptions::default())
    }
}

impl Light for SunLight {
    fn color(&self) -> Color {
        self.color
    }

    fn intensity(&self) -> f64 {
        self.intensity
    }
}
