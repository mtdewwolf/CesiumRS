//! CesiumJS `GaussianSplatRenderResources`: encapsulates shader builder,
//! render state, and alpha options for Gaussian splatting draw stages.

use super::depth_function::DepthFunction;
use super::model::ModelAlphaOptions;
use crate::renderer::render_state::DepthTest;
use crate::renderer::{RenderState, ShaderBuilder, ShaderDestination};

/// Render resources assembled during Gaussian splat pipeline stages.
#[derive(Clone, Debug)]
pub struct GaussianSplatRenderResources {
    /// Incremental shader builder.
    pub shader_builder: ShaderBuilder,
    /// Options for configuring the render state (depth test, etc.).
    pub render_state: RenderState,
    /// Options for configuring alpha pass and cutoff.
    pub alpha_options: ModelAlphaOptions,
    /// Whether the model is part of a tileset that uses skipLevelOfDetail.
    pub has_skip_level_of_detail: bool,
}

impl Default for GaussianSplatRenderResources {
    fn default() -> Self {
        Self::new(false)
    }
}

impl GaussianSplatRenderResources {
    /// Creates a new Gaussian splat render resources structure.
    pub fn new(use_log_depth: bool) -> Self {
        let mut shader_builder = ShaderBuilder::default();

        let render_state = RenderState {
            depth_test: DepthTest {
                enabled: true,
                func: DepthFunction::LessOrEqual.value(),
            },
            ..Default::default()
        };

        if use_log_depth {
            shader_builder.add_define(
                "LOG_DEPTH_READ_ONLY",
                None,
                ShaderDestination::FRAGMENT,
            );
        }

        Self {
            shader_builder,
            render_state,
            alpha_options: ModelAlphaOptions::default(),
            has_skip_level_of_detail: false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_resources_match_upstream() {
        let res = GaussianSplatRenderResources::new(false);
        assert!(res.render_state.depth_test.enabled);
        assert_eq!(
            res.render_state.depth_test.func,
            DepthFunction::LessOrEqual.value()
        );
        assert!(!res.has_skip_level_of_detail);
    }

    #[test]
    fn log_depth_adds_fragment_define() {
        let res = GaussianSplatRenderResources::new(true);
        let (_, fs) = res.shader_builder.sources();
        assert!(fs.defines.iter().any(|d| d.contains("LOG_DEPTH_READ_ONLY")));
    }
}
