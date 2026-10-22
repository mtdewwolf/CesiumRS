//! CesiumJS `ColorBlendMode`: how a model's color mixes with its source color.

use crate::core::math::{clamp, EPSILON4};

numeric_enum!(ColorBlendMode: i32 {
    Highlight = 0 => "HIGHLIGHT",
    Replace = 1 => "REPLACE",
    Mix = 2 => "MIX",
});

impl ColorBlendMode {
    /// The blend factor the shader uses: 0 to highlight, 1 to replace, and
    /// `color_blend_amount` clamped to [1e-4, 1] to mix.
    pub fn get_color_blend(self, color_blend_amount: f64) -> f64 {
        match self {
            Self::Highlight => 0.0,
            Self::Replace => 1.0,
            Self::Mix => clamp(color_blend_amount, EPSILON4, 1.0),
        }
    }
}
