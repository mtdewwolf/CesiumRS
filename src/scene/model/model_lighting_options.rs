//! CesiumJS `ModelLightingOptions`: the lighting model a model is shaded with.

use super::LightingModel;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ModelLightingOptions {
    pub lighting_model: LightingModel,
}

impl Default for ModelLightingOptions {
    /// Unlit, as `new ModelLightingOptions()` is.
    fn default() -> Self {
        Self { lighting_model: LightingModel::Unlit }
    }
}
