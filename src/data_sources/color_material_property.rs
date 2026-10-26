//! CesiumJS `ColorMaterialProperty`: solid-color material uniforms.
use super::{
    graphics::GraphicsProperties, ComponentRef, DefinitionChanged, DynamicValue, EntityComponent, PropertyInput,
};
use crate::core::{Color, JulianDate, RuntimeError};
use std::any::Any;
/// A material that evaluates to a solid [`Color`] uniform.
pub struct ColorMaterialProperty {
    properties: GraphicsProperties,
}
impl ColorMaterialProperty {
    /// Creates a material with an optional `color` property. Passing a raw
    /// value stores a ConstantProperty, matching createPropertyDescriptor.
    pub fn new(color: Option<PropertyInput>) -> Result<Self, RuntimeError> {
        let value = Self { properties: GraphicsProperties::new(&["color"]) };
        value.set_color(color)?;
        Ok(value)
    }
    pub fn color(&self) -> Option<PropertyInput> {
        self.properties.get("color")
    }
    pub fn set_color(&self, value: Option<PropertyInput>) -> Result<(), RuntimeError> {
        self.properties.set("color", value)
    }
    pub fn definition_changes(&self) -> &super::ChangeEvent<super::PropertyChange> {
        self.properties.changes()
    }
}
impl Default for ColorMaterialProperty {
    fn default() -> Self {
        Self { properties: GraphicsProperties::new(&["color"]) }
    }
}
impl EntityComponent for ColorMaterialProperty {
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn definition_changed(&self) -> &DefinitionChanged {
        self.properties.definition_changed()
    }
    fn is_constant(&self) -> bool {
        self.properties.is_constant()
    }
    fn get_value(
        &self,
        time: &JulianDate,
        result: Option<&mut DynamicValue>,
    ) -> Result<Option<DynamicValue>, RuntimeError> {
        let mut fields = match result.as_ref() {
            Some(DynamicValue::Record(fields)) => fields.clone(),
            _ => Vec::new(),
        };
        let mut previous = fields.iter().find(|(name, _)| name == "color").and_then(|(_, value)| value.clone());
        let color = match self.properties.get("color") {
            Some(PropertyInput::Component(component)) => component
                .get_value(time, previous.as_mut())?
                .filter(|value| !matches!(value, DynamicValue::Null))
                .unwrap_or(DynamicValue::Color(Color::WHITE)),
            Some(PropertyInput::Value(value)) if !matches!(value, DynamicValue::Null) => value,
            _ => DynamicValue::Color(Color::WHITE),
        };
        if let Some((_, value)) = fields.iter_mut().find(|(name, _)| name == "color") {
            *value = Some(color);
        } else {
            fields.push(("color".into(), Some(color)));
        }
        let value = DynamicValue::Record(fields);
        if let Some(result) = result {
            *result = value.clone();
        }
        Ok(Some(value))
    }
    fn equals(&self, other: &dyn EntityComponent) -> Result<bool, RuntimeError> {
        other.as_any().downcast_ref::<Self>().map_or(Ok(false), |other| self.properties.equals(&other.properties))
    }
    fn get_property(&self, name: &str) -> Option<PropertyInput> {
        self.properties.get(name)
    }
    fn get_type(&self, _: &JulianDate) -> Result<Option<String>, RuntimeError> {
        Ok(Some("Color".into()))
    }
}
/// Upstream ColorMaterialProperty has no `merge` or `clone`; report that so
/// Entity and composite composition keep Cesium's share-or-skip behaviour.
pub(crate) fn color_material_component(color: Color) -> Result<ComponentRef, RuntimeError> {
    Ok(std::rc::Rc::new(ColorMaterialProperty::new(Some(PropertyInput::value(color)))?))
}
