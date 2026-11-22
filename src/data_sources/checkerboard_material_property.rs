//! CesiumJS `CheckerboardMaterialProperty`.
use super::record_property::record_property;
use crate::core::{Cartesian2, Color};
record_property! {
    CheckerboardMaterialProperty;
    fields: [
        "evenColor" => even_color, set_even_color;
        "oddColor" => odd_color, set_odd_color;
        "repeat" => repeat, set_repeat;
    ];
    material: "Checkerboard";
    evaluate: |out| {
        out.field_as("evenColor", "lightColor", Color::WHITE)?;
        out.field_as("oddColor", "darkColor", Color::BLACK)?;
        out.field_without_result("repeat", Cartesian2::new(2., 2.))?;
    }
}
impl CheckerboardMaterialProperty {
    pub fn new() -> Self {
        Self::default()
    }
}
