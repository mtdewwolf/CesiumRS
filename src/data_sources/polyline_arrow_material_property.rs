//! CesiumJS `PolylineArrowMaterialProperty`.
use super::record_property::record_property;
use crate::core::Color;
record_property! {
    PolylineArrowMaterialProperty;
    fields: [
        "color" => color, set_color;
    ];
    material: "PolylineArrow";
    evaluate: |out| {
        out.field("color", Color::WHITE)?;
    }
}
impl PolylineArrowMaterialProperty {
    pub fn new(color: Option<PropertyInput>) -> Result<Self, RuntimeError> {
        let property = Self::default();
        property.set_color(color)?;
        Ok(property)
    }
}
