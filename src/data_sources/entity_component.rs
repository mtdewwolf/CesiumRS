//! Typed component contracts replace JavaScript getValue/merge/clone duck typing.
use super::{dynamic_value::*, position_property::PositionPropertyRef, DefinitionChanged, PropertyRef, PropertyValue};
use crate::core::{Cartesian3, JulianDate, RuntimeError};
use std::{any::Any, rc::Rc};
pub type ComponentRef = Rc<dyn EntityComponent>;
pub trait EntityComponent: Any {
    fn as_any(&self) -> &dyn Any;
    fn identity(&self) -> *const () {
        self.as_any() as *const dyn Any as *const ()
    }
    fn definition_changed(&self) -> &DefinitionChanged;
    fn is_constant(&self) -> bool {
        false
    }
    fn get_value(
        &self,
        _time: &JulianDate,
        _result: Option<&mut DynamicValue>,
    ) -> Result<Option<DynamicValue>, RuntimeError> {
        Err(RuntimeError::new("component.getValue is not a function"))
    }
    fn equals(&self, other: &dyn EntityComponent) -> Result<bool, RuntimeError>;
    fn supports_merge(&self) -> bool {
        false
    }
    fn supports_clone(&self) -> bool {
        false
    }
    fn merge_component(&self, _source: &dyn EntityComponent) -> Result<(), RuntimeError> {
        Err(RuntimeError::new("component.merge is not a function"))
    }
    fn merge_dynamic_options(&self, _source: DynamicValue) -> Result<(), RuntimeError> {
        Err(RuntimeError::new("component.merge is not a function"))
    }
    fn clone_component(&self, _result: Option<ComponentRef>) -> Result<ComponentRef, RuntimeError> {
        Err(RuntimeError::new("component.clone is not a function"))
    }
    fn get_property(&self, _name: &str) -> Option<PropertyInput> {
        None
    }
    fn get_type(&self, _time: &JulianDate) -> Result<Option<String>, RuntimeError> {
        Err(RuntimeError::new("component.getType is not a function"))
    }
    fn position_property(&self) -> Option<PositionPropertyRef> {
        None
    }
}
#[derive(Clone)]
pub enum PropertyInput {
    Value(DynamicValue),
    Component(ComponentRef),
}
impl PropertyInput {
    pub fn value(value: impl Into<DynamicValue>) -> Self {
        Self::Value(value.into())
    }
    pub fn is_defined(&self) -> bool {
        !matches!(self, Self::Value(DynamicValue::Null))
    }
    pub fn strict_equals(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Value(a), Self::Value(b)) => a.strict_equals(b),
            (Self::Component(a), Self::Component(b)) => a.identity() == b.identity(),
            _ => false,
        }
    }
    pub fn component(&self) -> Option<ComponentRef> {
        if let Self::Component(c) = self {
            Some(c.clone())
        } else {
            None
        }
    }
}
pub fn component_equals(a: Option<&PropertyInput>, b: Option<&PropertyInput>) -> Result<bool, RuntimeError> {
    match (a, b) {
        (None, None) => Ok(true),
        (Some(a), Some(b)) if a.strict_equals(b) => Ok(true),
        (Some(PropertyInput::Component(a)), Some(PropertyInput::Component(b))) => a.equals(&**b),
        _ => Ok(false),
    }
}
pub struct PropertyComponent<T: ComponentValue> {
    property: PropertyRef<T>,
}
impl<T: ComponentValue> PropertyComponent<T> {
    pub fn wrap(property: PropertyRef<T>) -> ComponentRef {
        Rc::new(Self { property })
    }
    pub fn property(&self) -> &PropertyRef<T> {
        &self.property
    }
}
impl<T: ComponentValue> EntityComponent for PropertyComponent<T> {
    fn identity(&self) -> *const () {
        Rc::as_ptr(&self.property) as *const ()
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn definition_changed(&self) -> &DefinitionChanged {
        self.property.definition_changed()
    }
    fn is_constant(&self) -> bool {
        self.property.is_constant()
    }
    fn get_value(
        &self,
        time: &JulianDate,
        mut result: Option<&mut DynamicValue>,
    ) -> Result<Option<DynamicValue>, RuntimeError> {
        let mut scratch = result.as_deref().and_then(T::from_dynamic);
        let value = self.property.get_value(Some(time), scratch.as_mut())?;
        if let (Some(out), Some(scratch)) = (result.as_deref_mut(), scratch) {
            *out = scratch.into_dynamic();
        }
        let value = value.map(ComponentValue::into_dynamic);
        if let (Some(out), Some(value)) = (result, value.as_ref()) {
            *out = value.clone();
        }
        Ok(value)
    }
    fn equals(&self, other: &dyn EntityComponent) -> Result<bool, RuntimeError> {
        let Some(o) = other.as_any().downcast_ref::<Self>() else { return Ok(false) };
        if Rc::ptr_eq(&self.property, &o.property) {
            Ok(true)
        } else {
            self.property.equals(&*o.property)
        }
    }
}
pub struct PositionComponent {
    property: PositionPropertyRef,
}
impl PositionComponent {
    pub fn wrap(property: PositionPropertyRef) -> ComponentRef {
        Rc::new(Self { property })
    }
}
impl EntityComponent for PositionComponent {
    fn identity(&self) -> *const () {
        Rc::as_ptr(&self.property) as *const ()
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn definition_changed(&self) -> &DefinitionChanged {
        self.property.definition_changed()
    }
    fn is_constant(&self) -> bool {
        self.property.is_constant()
    }
    fn get_value(
        &self,
        time: &JulianDate,
        result: Option<&mut DynamicValue>,
    ) -> Result<Option<DynamicValue>, RuntimeError> {
        let p: PropertyRef<Cartesian3> = self.property.clone();
        PropertyComponent::wrap(p).get_value(time, result)
    }
    fn equals(&self, other: &dyn EntityComponent) -> Result<bool, RuntimeError> {
        let Some(o) = other.as_any().downcast_ref::<Self>() else { return Ok(false) };
        if Rc::ptr_eq(&self.property, &o.property) {
            Ok(true)
        } else {
            self.property.equals(&*o.property)
        }
    }
    fn position_property(&self) -> Option<PositionPropertyRef> {
        Some(self.property.clone())
    }
}
