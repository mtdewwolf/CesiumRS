//! CesiumJS `GlobeTranslucency`: front- and back-face translucency, distance
//! based alpha and the rectangle that limits translucency on the globe.
//!
//! Setters validate as Cesium's `Check` does: an alpha is rejected only when
//! it is below 0 or above 1, so NaN is accepted, and a distance scalar is
//! rejected only when `far < near`.

use crate::core::{NearFarScalar, Rectangle, RuntimeError};

#[derive(Clone, Debug, PartialEq)]
pub struct GlobeTranslucency {
    /// `enabled`; Cesium only checks that the value is a boolean.
    pub enabled: bool,
    front_face_alpha: f64,
    front_face_alpha_by_distance: Option<NearFarScalar>,
    back_face_alpha: f64,
    back_face_alpha_by_distance: Option<NearFarScalar>,
    rectangle: Rectangle,
}

impl Default for GlobeTranslucency {
    fn default() -> Self {
        Self::new()
    }
}

fn check_alpha(name: &str, alpha: f64) -> Result<(), RuntimeError> {
    if alpha < 0.0 {
        return Err(RuntimeError::new(format!(
            "Expected {name} to be greater than or equal to 0, actual value was {}",
            crate::core::math::js_number_to_string(alpha)
        )));
    }
    if alpha > 1.0 {
        return Err(RuntimeError::new(format!(
            "Expected {name} to be less than or equal to 1, actual value was {}",
            crate::core::math::js_number_to_string(alpha)
        )));
    }
    Ok(())
}

fn check_distance(scalar: &Option<NearFarScalar>) -> Result<(), RuntimeError> {
    match scalar {
        Some(s) if s.far < s.near => Err(RuntimeError::new("far distance must be greater than near distance.")),
        _ => Ok(()),
    }
}

impl GlobeTranslucency {
    /// Translucency disabled, opaque faces, no distance scalars and
    /// `Rectangle.MAX_VALUE`.
    pub fn new() -> Self {
        Self {
            enabled: false,
            front_face_alpha: 1.0,
            front_face_alpha_by_distance: None,
            back_face_alpha: 1.0,
            back_face_alpha_by_distance: None,
            rectangle: Rectangle::MAX_VALUE,
        }
    }

    pub fn front_face_alpha(&self) -> f64 {
        self.front_face_alpha
    }

    pub fn set_front_face_alpha(&mut self, alpha: f64) -> Result<(), RuntimeError> {
        check_alpha("frontFaceAlpha", alpha)?;
        self.front_face_alpha = alpha;
        Ok(())
    }

    pub fn back_face_alpha(&self) -> f64 {
        self.back_face_alpha
    }

    pub fn set_back_face_alpha(&mut self, alpha: f64) -> Result<(), RuntimeError> {
        check_alpha("backFaceAlpha", alpha)?;
        self.back_face_alpha = alpha;
        Ok(())
    }

    pub fn front_face_alpha_by_distance(&self) -> Option<NearFarScalar> {
        self.front_face_alpha_by_distance
    }

    pub fn set_front_face_alpha_by_distance(&mut self, scalar: Option<NearFarScalar>) -> Result<(), RuntimeError> {
        check_distance(&scalar)?;
        self.front_face_alpha_by_distance = scalar;
        Ok(())
    }

    pub fn back_face_alpha_by_distance(&self) -> Option<NearFarScalar> {
        self.back_face_alpha_by_distance
    }

    pub fn set_back_face_alpha_by_distance(&mut self, scalar: Option<NearFarScalar>) -> Result<(), RuntimeError> {
        check_distance(&scalar)?;
        self.back_face_alpha_by_distance = scalar;
        Ok(())
    }

    pub fn rectangle(&self) -> Rectangle {
        self.rectangle
    }

    /// Setting `None` restores `Rectangle.MAX_VALUE`.
    pub fn set_rectangle(&mut self, rectangle: Option<Rectangle>) {
        self.rectangle = rectangle.unwrap_or(Rectangle::MAX_VALUE);
    }
}
