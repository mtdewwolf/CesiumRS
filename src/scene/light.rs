//! CesiumJS `Light`: the scene light interface. Cesium's abstract base class
//! is this trait.

use crate::core::Color;

pub trait Light {
    /// The light's color.
    fn color(&self) -> Color;
    /// The light's intensity, multiplied with `color`.
    fn intensity(&self) -> f64;
}
