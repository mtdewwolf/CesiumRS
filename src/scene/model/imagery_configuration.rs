//! CesiumJS `ImageryConfiguration`: a snapshot of the imagery layer
//! settings a model's imagery was built with, so that changes can be
//! detected.
//!
//! CesiumJS layers may give some settings as per-tile callbacks; the
//! snapshot copies values, so here they are numbers.

use crate::core::Color;

/// The settings of an imagery layer that a model's imagery depends on.
pub trait ImageryLayerSettings {
    fn show(&self) -> bool;
    fn alpha(&self) -> f64;
    fn brightness(&self) -> f64;
    fn contrast(&self) -> f64;
    fn hue(&self) -> f64;
    fn saturation(&self) -> f64;
    fn gamma(&self) -> f64;
    fn color_to_alpha(&self) -> Option<Color>;
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ImageryConfiguration {
    pub show: bool,
    pub alpha: f64,
    pub brightness: f64,
    pub contrast: f64,
    pub hue: f64,
    pub saturation: f64,
    pub gamma: f64,
    pub color_to_alpha: Option<Color>,
}

impl ImageryConfiguration {
    pub fn new(imagery_layer: &impl ImageryLayerSettings) -> Self {
        Self {
            show: imagery_layer.show(),
            alpha: imagery_layer.alpha(),
            brightness: imagery_layer.brightness(),
            contrast: imagery_layer.contrast(),
            hue: imagery_layer.hue(),
            saturation: imagery_layer.saturation(),
            gamma: imagery_layer.gamma(),
            color_to_alpha: imagery_layer.color_to_alpha(),
        }
    }
}
