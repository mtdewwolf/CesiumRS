//! CesiumJS `BlendingState`: the common blending render states.

use super::{BlendEquation, BlendFunction};
use crate::renderer::render_state::Blending;

fn blend(source_rgb: BlendFunction, destination: BlendFunction) -> Blending {
    Blending {
        enabled: true,
        equation_rgb: BlendEquation::Add.value(),
        equation_alpha: BlendEquation::Add.value(),
        function_source_rgb: source_rgb.value(),
        function_source_alpha: BlendFunction::One.value(),
        function_destination_rgb: destination.value(),
        function_destination_alpha: destination.value(),
        ..Blending::default()
    }
}

/// Blending off. Cesium's object sets only `enabled`; the other fields are
/// the render-state defaults it resolves to.
pub fn disabled() -> Blending {
    Blending { enabled: false, ..Blending::default() }
}

/// `source * alpha + destination * (1 - alpha)`.
pub fn alpha_blend() -> Blending {
    blend(BlendFunction::SourceAlpha, BlendFunction::OneMinusSourceAlpha)
}

/// Alpha blending for colors already multiplied by alpha.
pub fn pre_multiplied_alpha_blend() -> Blending {
    blend(BlendFunction::One, BlendFunction::OneMinusSourceAlpha)
}

/// `source * alpha + destination`.
pub fn additive_blend() -> Blending {
    blend(BlendFunction::SourceAlpha, BlendFunction::One)
}
