//! CesiumJS `PolylineDashMaterialProperty`.
use super::record_property::record_property;
use crate::core::Color;
record_property! {
    PolylineDashMaterialProperty;
    fields: [
        "color" => color, set_color;
        "gapColor" => gap_color, set_gap_color;
        "dashLength" => dash_length, set_dash_length;
        "dashPattern" => dash_pattern, set_dash_pattern;
    ];
    material: "PolylineDash";
    evaluate: |out| {
        out.field("color", Color::WHITE)?;
        out.field("gapColor", Color::TRANSPARENT)?;
        out.field_without_result("dashLength", 16.)?;
        out.field_without_result("dashPattern", 255.)?;
    }
}
impl PolylineDashMaterialProperty {
    pub fn new() -> Self {
        Self::default()
    }
}
