//! CesiumJS `PolylineOutlineMaterialProperty`.
use super::record_property::record_property;
use crate::core::Color;
record_property! {
    PolylineOutlineMaterialProperty;
    fields: [
        "color" => color, set_color;
        "outlineColor" => outline_color, set_outline_color;
        "outlineWidth" => outline_width, set_outline_width;
    ];
    material: "PolylineOutline";
    evaluate: |out| {
        out.field("color", Color::WHITE)?;
        out.field("outlineColor", Color::BLACK)?;
        out.field_without_result("outlineWidth", 1.)?;
    }
}
impl PolylineOutlineMaterialProperty {
    pub fn new() -> Self {
        Self::default()
    }
}
