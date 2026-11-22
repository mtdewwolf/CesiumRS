//! CesiumJS `ColorMaterialProperty`, a solid-color material.
use super::{record_property::record_property, ComponentRef};
use crate::core::Color;

record_property! {
    ColorMaterialProperty;
    fields: ["color" => color, set_color];
    material: "Color";
    evaluate: |out| { out.field("color", Color::WHITE)?; }
}
impl ColorMaterialProperty {
    pub fn new(color: Option<PropertyInput>) -> Result<Self, RuntimeError> {
        let value = Self::default();
        value.set_color(color)?;
        Ok(value)
    }
}
pub(crate) fn color_material_component(color: Color) -> Result<ComponentRef, RuntimeError> {
    Ok(std::rc::Rc::new(ColorMaterialProperty::new(Some(PropertyInput::value(color)))?))
}
