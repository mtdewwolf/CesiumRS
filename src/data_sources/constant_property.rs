use super::{property::write_result, DefinitionChanged, Property, PropertyValue};
use crate::core::{JulianDate, RuntimeError};
use std::{any::Any, cell::RefCell};
pub struct ConstantProperty<T: PropertyValue> {
    value: RefCell<Option<T>>,
    changed: DefinitionChanged,
}
impl<T: PropertyValue> ConstantProperty<T> {
    pub fn new(value: Option<T>) -> Self {
        Self { value: RefCell::new(value), changed: DefinitionChanged::new() }
    }
    pub fn value_of(&self) -> Option<T> {
        self.value.borrow().clone()
    }
    pub fn set_value(&self, value: Option<T>) {
        let changed = match (&*self.value.borrow(), &value) {
            (None, None) => false,
            (Some(a), Some(b)) => !b.property_equals(a),
            _ => true,
        };
        if changed {
            *self.value.borrow_mut() = value;
            self.changed.raise();
        }
    }
}
impl<T: PropertyValue> std::fmt::Display for ConstantProperty<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.value.borrow().as_ref().map_or_else(|| "undefined".into(), PropertyValue::property_string))
    }
}
impl<T: PropertyValue> Property<T> for ConstantProperty<T> {
    fn is_constant(&self) -> bool {
        true
    }
    fn definition_changed(&self) -> &DefinitionChanged {
        &self.changed
    }
    fn get_value(&self, _: Option<&JulianDate>, result: Option<&mut T>) -> Result<Option<T>, RuntimeError> {
        Ok(write_result(self.value_of(), result))
    }
    fn equals(&self, other: &dyn Property<T>) -> Result<bool, RuntimeError> {
        Ok(other.as_any().downcast_ref::<Self>().is_some_and(|o| {
            std::ptr::eq(self, o)
                || match (&*self.value.borrow(), &*o.value.borrow()) {
                    (None, None) => true,
                    (Some(a), Some(b)) => a.property_equals(b),
                    _ => false,
                }
        }))
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}
