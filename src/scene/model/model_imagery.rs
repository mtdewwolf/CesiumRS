//! CesiumJS `Scene/Model/ModelImagery`: manages draped imagery across all
//! primitives of a model.

use super::{ImageryConfiguration, ModelPrimitiveImagery};
use crate::renderer::Gl;

/// Manages the collection of `ModelPrimitiveImagery` instances for a model.
#[derive(Clone)]
pub struct ModelImagery<G: Gl> {
    pub model_primitive_imageries: Vec<ModelPrimitiveImagery<G>>,
    pub imagery_configurations: Vec<ImageryConfiguration>,
    pub ready: bool,
}

impl<G: Gl> Default for ModelImagery<G> {
    fn default() -> Self {
        Self { model_primitive_imageries: Vec::new(), imagery_configurations: Vec::new(), ready: true }
    }
}

impl<G: Gl> ModelImagery<G> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn is_ready(&self) -> bool {
        self.ready && self.model_primitive_imageries.iter().all(|pi| pi.is_ready())
    }

    pub fn primitive_imageries(&self) -> &[ModelPrimitiveImagery<G>] {
        &self.model_primitive_imageries
    }

    pub fn primitive_imageries_mut(&mut self) -> &mut [ModelPrimitiveImagery<G>] {
        &mut self.model_primitive_imageries
    }

    pub fn reset(&mut self) {
        self.model_primitive_imageries.clear();
        self.imagery_configurations.clear();
        self.ready = false;
    }
}
