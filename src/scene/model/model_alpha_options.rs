//! CesiumJS `ModelAlphaOptions`: the pass and alpha cutoff a model's
//! pipeline stages choose.

use crate::renderer::Pass;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ModelAlphaOptions {
    pub pass: Option<Pass>,
    pub alpha_cutoff: Option<f64>,
}
