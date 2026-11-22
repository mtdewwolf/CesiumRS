//! CesiumJS `GridMaterialProperty`.
use super::record_property::record_property;
use crate::core::{Cartesian2, Color};
record_property! {
    GridMaterialProperty;
    fields: [
        "color" => color, set_color;
        "cellAlpha" => cell_alpha, set_cell_alpha;
        "lineCount" => line_count, set_line_count;
        "lineThickness" => line_thickness, set_line_thickness;
        "lineOffset" => line_offset, set_line_offset;
    ];
    material: "Grid";
    evaluate: |out| {
        out.field("color", Color::WHITE)?;
        out.field_without_result("cellAlpha", 0.1)?;
        out.field("lineCount", Cartesian2::new(8., 8.))?;
        out.field("lineThickness", Cartesian2::new(1., 1.))?;
        out.field("lineOffset", Cartesian2::ZERO)?;
    }
}
impl GridMaterialProperty {
    pub fn new() -> Self {
        Self::default()
    }
}
