//! CesiumJS `DebugInspector`: shader modifications and color overrides for
//! debug inspection of frustums and scene draw commands.

use crate::core::Color;
use std::collections::HashMap;

/// Inspects and transforms draw commands and shaders to visualize scene frustums
/// and individual command boundaries.
#[derive(Clone, Debug, Default)]
pub struct DebugInspector {
    cached_shaders: HashMap<String, String>,
}

impl DebugInspector {
    /// Creates a new `DebugInspector`.
    pub fn new() -> Self {
        Self {
            cached_shaders: HashMap::new(),
        }
    }

    /// Computes the debug frustum color corresponding to a bitmask of overlapping frustum indices.
    /// Bit 0: red, Bit 1: green, Bit 2: blue.
    pub fn compute_frustum_color(overlapping_frustums_mask: u32) -> Color {
        let red = if overlapping_frustums_mask & (1 << 0) != 0 { 1.0 } else { 0.0 };
        let green = if overlapping_frustums_mask & (1 << 1) != 0 { 1.0 } else { 0.0 };
        let blue = if overlapping_frustums_mask & (1 << 2) != 0 { 1.0 } else { 0.0 };
        Color::new(red, green, blue, 1.0)
    }

    /// Rewrites fragment shader source code to wrap `main()` with debug frustum
    /// and command color multipliers.
    pub fn rewrite_fragment_shader_for_debug(source: &str) -> String {
        let mut rewritten = crate::renderer::ShaderSource::replace_main(source, "czm_Debug_main");

        let mut targets = Vec::new();
        let needle = "out_FragData_";
        let mut remainder = source;
        while let Some(pos) = remainder.find(needle) {
            let after = &remainder[pos + needle.len()..];
            let digits_len = after.chars().take_while(|c| c.is_ascii_digit()).count();
            if digits_len > 0 {
                let idx_str = &after[..digits_len];
                if !targets.contains(&idx_str.to_string()) {
                    targets.push(idx_str.to_string());
                }
                remainder = &after[digits_len..];
            } else {
                remainder = after;
            }
        }

        let mut footer = String::new();
        footer.push_str("\nuniform vec3 debugShowCommandsColor;\n");
        footer.push_str("uniform vec3 debugShowFrustumsColor;\n");
        footer.push_str("void main()\n{\n");
        footer.push_str("    czm_Debug_main();\n");

        if !targets.is_empty() {
            for target in &targets {
                footer.push_str(&format!(
                    "    out_FragData_{target}.rgb *= debugShowCommandsColor;\n"
                ));
                footer.push_str(&format!(
                    "    out_FragData_{target}.rgb *= debugShowFrustumsColor;\n"
                ));
            }
        } else {
            footer.push_str("    out_FragColor.rgb *= debugShowCommandsColor;\n");
            footer.push_str("    out_FragColor.rgb *= debugShowFrustumsColor;\n");
        }
        footer.push_str("}\n");

        rewritten.push_str(&footer);
        rewritten
    }

    /// Obtains or caches the debug fragment shader for a given shader key.
    pub fn get_or_create_debug_shader<'a>(&'a mut self, shader_id: &str, source: &str) -> &'a str {
        self.cached_shaders
            .entry(shader_id.to_string())
            .or_insert_with(|| Self::rewrite_fragment_shader_for_debug(source))
            .as_str()
    }

    /// Clears any cached debug shaders.
    pub fn clear(&mut self) {
        self.cached_shaders.clear();
    }
}
