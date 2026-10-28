//! CesiumJS `GlobeTranslucency`: properties controlling front- and back-face
//! translucency, distance-based alpha fading, and geographical bounding for the globe.

use crate::core::{NearFarScalar, Rectangle, RuntimeError};

/// Configuration and properties for controlling globe surface translucency.
#[derive(Clone, Debug, PartialEq)]
pub struct GlobeTranslucency {
    pub enabled: bool,
    front_face_alpha: f64,
    pub front_face_alpha_by_distance: Option<NearFarScalar>,
    back_face_alpha: f64,
    pub back_face_alpha_by_distance: Option<NearFarScalar>,
    pub rectangle: Rectangle,
}

impl Default for GlobeTranslucency {
    fn default() -> Self {
        Self::new()
    }
}

impl GlobeTranslucency {
    /// Creates a new `GlobeTranslucency` instance with default values (translucency disabled).
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

    /// Constant translucency alpha applied to front faces of the globe (range 0.0 to 1.0).
    pub fn front_face_alpha(&self) -> f64 {
        self.front_face_alpha
    }

    /// Sets front face alpha clamped to [0.0, 1.0].
    pub fn set_front_face_alpha(&mut self, alpha: f64) -> Result<(), RuntimeError> {
        if !(0.0..=1.0).contains(&alpha) {
            return Err(RuntimeError::new("front_face_alpha must be between 0.0 and 1.0."));
        }
        self.front_face_alpha = alpha;
        Ok(())
    }

    /// Constant translucency alpha applied to back faces of the globe (range 0.0 to 1.0).
    pub fn back_face_alpha(&self) -> f64 {
        self.back_face_alpha
    }

    /// Sets back face alpha clamped to [0.0, 1.0].
    pub fn set_back_face_alpha(&mut self, alpha: f64) -> Result<(), RuntimeError> {
        if !(0.0..=1.0).contains(&alpha) {
            return Err(RuntimeError::new("back_face_alpha must be between 0.0 and 1.0."));
        }
        self.back_face_alpha = alpha;
        Ok(())
    }

    /// Sets near and far alpha limits for back faces based on distance to camera.
    pub fn set_back_face_alpha_by_distance(
        &mut self,
        scalar: Option<NearFarScalar>,
    ) -> Result<(), RuntimeError> {
        if let Some(s) = &scalar {
            if s.far < s.near {
                return Err(RuntimeError::new("far distance must be greater than near distance."));
            }
        }
        self.back_face_alpha_by_distance = scalar;
        Ok(())
    }

    /// Sets near and far alpha limits for front faces based on distance to camera.
    pub fn set_front_face_alpha_by_distance(
        &mut self,
        scalar: Option<NearFarScalar>,
    ) -> Result<(), RuntimeError> {
        if let Some(s) = &scalar {
            if s.far < s.near {
                return Err(RuntimeError::new("far distance must be greater than near distance."));
            }
        }
        self.front_face_alpha_by_distance = scalar;
        Ok(())
    }
}
