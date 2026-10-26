//! CesiumJS `createMaterialPropertyDescriptor`: infer material components.
//!
//! This batch supports the Color material. Image/Resource/HTML video and
//! canvas inputs need `ImageMaterialProperty`, which is still unported.
use super::{
    color_material_property::color_material_component, create_property_descriptor, ChangeEvent, ComponentFactory,
    ComponentRef, DynamicValue, PropertyChange, PropertyDescriptor,
};
use crate::core::RuntimeError;
use std::rc::Rc;
/// Cesium's `createMaterialProperty` coercion for the supported material set.
pub(crate) fn create_material_component(value: DynamicValue) -> Result<ComponentRef, RuntimeError> {
    match value {
        DynamicValue::Color(color) => color_material_component(color),
        DynamicValue::String(_) => Err(RuntimeError::new(
            "ImageMaterialProperty is not ported; pass a ColorMaterialProperty component instead",
        )),
        _ => Err(RuntimeError::new("Unable to infer material type")),
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
