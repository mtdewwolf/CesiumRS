//! CesiumJS `createMaterialPropertyDescriptor`: infer material components.
use super::{
    color_material_property::color_material_component, create_property_descriptor, ChangeEvent, ComponentFactory,
    ComponentRef, DynamicValue, PropertyChange, PropertyDescriptor, PropertyValue,
};
use crate::core::RuntimeError;
use std::rc::Rc;
/// Cesium's `createMaterialProperty` coercion for the supported material set.
pub(crate) fn create_material_component(value: DynamicValue) -> Result<ComponentRef, RuntimeError> {
    match value {
        DynamicValue::Color(color) => color_material_component(color),
        DynamicValue::String(_) | DynamicValue::Resource(_) => {
            let material = super::ImageMaterialProperty::new();
            material.set_image(Some(super::PropertyInput::Value(value)))?;
            Ok(Rc::new(material))
        }
        _ => Err(RuntimeError::new(format!("Unable to infer material type: {}", value.property_string()))),
    }
}
/// Native equivalent of `createMaterialPropertyDescriptor`.
pub fn create_material_property_descriptor(
    name: impl Into<String>,
    configurable: bool,
    event: &ChangeEvent<PropertyChange>,
) -> PropertyDescriptor {
    create_property_descriptor(name, configurable, Some(material_factory()), event)
}
/// Factory used by graphics `material` and `depthFailMaterial` slots.
pub(crate) fn material_factory() -> ComponentFactory {
    Rc::new(create_material_component)
}
