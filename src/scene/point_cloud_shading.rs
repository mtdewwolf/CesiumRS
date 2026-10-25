//! CesiumJS `PointCloudShading`: point cloud attenuation and eye dome
//! lighting options.

use crate::renderer::ContextCapabilities;

/// `new PointCloudShading(options)`: unset fields take Cesium's defaults.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PointCloudShadingOptions {
    pub attenuation: Option<bool>,
    pub geometric_error_scale: Option<f64>,
    pub maximum_attenuation: Option<f64>,
    pub base_resolution: Option<f64>,
    pub eye_dome_lighting: Option<bool>,
    pub eye_dome_lighting_strength: Option<f64>,
    pub eye_dome_lighting_radius: Option<f64>,
    pub back_face_culling: Option<bool>,
    pub normal_shading: Option<bool>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct PointCloudShading {
    /// Size points by geometric error; off by default.
    pub attenuation: bool,
    pub geometric_error_scale: f64,
    /// Pixels; `None` uses the tileset's maximum screen space error.
    pub maximum_attenuation: Option<f64>,
    /// Meters; `None` estimates it from the tileset.
    pub base_resolution: Option<f64>,
    pub eye_dome_lighting: bool,
    pub eye_dome_lighting_strength: f64,
    pub eye_dome_lighting_radius: f64,
    pub back_face_culling: bool,
    pub normal_shading: bool,
}

impl PointCloudShading {
    pub fn new(options: PointCloudShadingOptions) -> Self {
        Self {
            attenuation: options.attenuation.unwrap_or(false),
            geometric_error_scale: options.geometric_error_scale.unwrap_or(1.0),
            maximum_attenuation: options.maximum_attenuation,
            base_resolution: options.base_resolution,
            eye_dome_lighting: options.eye_dome_lighting.unwrap_or(true),
            eye_dome_lighting_strength: options.eye_dome_lighting_strength.unwrap_or(1.0),
            eye_dome_lighting_radius: options.eye_dome_lighting_radius.unwrap_or(1.0),
            back_face_culling: options.back_face_culling.unwrap_or(false),
            normal_shading: options.normal_shading.unwrap_or(true),
        }
    }

    /// Whether eye dome lighting can run: CesiumJS's
    /// `PointCloudEyeDomeLighting.isSupported`, which needs multiple draw
    /// buffers and fragment depth.
    pub fn is_supported(capabilities: &ContextCapabilities) -> bool {
        capabilities.draw_buffers && capabilities.fragment_depth
    }
}

impl Default for PointCloudShading {
    fn default() -> Self {
        Self::new(PointCloudShadingOptions::default())
    }
}
