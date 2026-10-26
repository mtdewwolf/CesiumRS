//! CesiumJS `Cesium3DTilesetHeatmap`: colorizes visible tiles in a heatmap palette
//! based on dynamic or referenced min/max value windows.

use crate::core::{math::EPSILON7, Color};
use std::collections::HashMap;

/// The fixed six-color gradient used for 3D tileset heatmap colorization:
/// Dark Gray -> Blue -> Pink -> Red -> Orange -> Yellow.
pub const HEATMAP_COLORS: [Color; 6] = [
    Color { red: 0.1, green: 0.1, blue: 0.1, alpha: 1.0 },
    Color { red: 0.153, green: 0.278, blue: 0.878, alpha: 1.0 },
    Color { red: 0.827, green: 0.231, blue: 0.49, alpha: 1.0 },
    Color { red: 0.827, green: 0.188, blue: 0.22, alpha: 1.0 },
    Color { red: 1.0, green: 0.592, blue: 0.259, alpha: 1.0 },
    Color { red: 1.0, green: 0.843, blue: 0.0, alpha: 1.0 },
];

/// Computes the interpolated heatmap color between `min` and `max`.
pub fn compute_heatmap_color(value: f64, min: f64, max: f64) -> Color {
    let shifted_max = max - min + EPSILON7;
    let shifted_value = (value - min).clamp(0.0, shifted_max);
    let zero_to_one = shifted_value / shifted_max;
    let last_index = (HEATMAP_COLORS.len() - 1) as f64;
    let color_position = zero_to_one * last_index;
    let color_position_floor = color_position.floor() as usize;
    let color_position_ceil = color_position.ceil() as usize;
    let t = color_position - (color_position_floor as f64);

    let c0 = &HEATMAP_COLORS[color_position_floor.min(HEATMAP_COLORS.len() - 1)];
    let c1 = &HEATMAP_COLORS[color_position_ceil.min(HEATMAP_COLORS.len() - 1)];

    Color {
        red: crate::core::math::lerp(c0.red, c1.red, t),
        green: crate::core::math::lerp(c0.green, c1.green, t),
        blue: crate::core::math::lerp(c0.blue, c1.blue, t),
        alpha: 1.0,
    }
}

/// A heatmap colorizer for tilesets.
#[derive(Clone, Debug, PartialEq)]
pub struct Cesium3DTilesetHeatmap {
    pub tile_property_name: Option<String>,
    pub minimum: f64,
    pub maximum: f64,
    pub previous_minimum: f64,
    pub previous_maximum: f64,
    pub reference_minimum: HashMap<String, f64>,
    pub reference_maximum: HashMap<String, f64>,
}

impl Default for Cesium3DTilesetHeatmap {
    fn default() -> Self {
        Self::new(None)
    }
}

impl Cesium3DTilesetHeatmap {
    pub fn new(tile_property_name: Option<String>) -> Self {
        Self {
            tile_property_name,
            minimum: f64::MAX,
            maximum: -f64::MAX,
            previous_minimum: f64::MAX,
            previous_maximum: -f64::MAX,
            reference_minimum: HashMap::new(),
            reference_maximum: HashMap::new(),
        }
    }

    /// Sets explicit reference minimum and maximum for a given tile property.
    pub fn set_reference_minimum_maximum(&mut self, minimum: f64, maximum: f64, tile_property_name: impl Into<String>) {
        let name = tile_property_name.into();
        self.reference_minimum.insert(name.clone(), minimum);
        self.reference_maximum.insert(name, maximum);
    }

    /// Colorizes a tile value relative to the previous frame's minimum and maximum.
    /// Also records the value into the current frame's dynamic range.
    pub fn colorize_value(
        &mut self,
        value: f64,
        content_available: bool,
        is_selected_this_frame: bool,
    ) -> Option<Color> {
        if self.tile_property_name.is_none() || !content_available || !is_selected_this_frame {
            return None;
        }

        self.maximum = self.maximum.max(value);
        self.minimum = self.minimum.min(value);

        if self.previous_minimum == f64::MAX || self.previous_maximum == -f64::MAX {
            return None;
        }

        Some(compute_heatmap_color(value, self.previous_minimum, self.previous_maximum))
    }

    /// Resets the tracked minimum and maximum at the start of a traversal frame.
    pub fn reset_minimum_maximum(&mut self) {
        if let Some(prop) = &self.tile_property_name {
            let ref_min = self.reference_minimum.get(prop).copied();
            let ref_max = self.reference_maximum.get(prop).copied();
            let use_ref = ref_min.is_some() && ref_max.is_some();

            self.previous_minimum = if use_ref { ref_min.unwrap() } else { self.minimum };
            self.previous_maximum = if use_ref { ref_max.unwrap() } else { self.maximum };
            self.minimum = f64::MAX;
            self.maximum = -f64::MAX;
        }
    }
}
