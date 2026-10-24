use super::{
    property::{system_clock, PropertyClock},
    DefinitionChanged, Property, PropertyValue,
};
use crate::core::{JulianDate, RuntimeError};
use std::{any::Any, cell::RefCell, rc::Rc};
pub type PropertyCallback<T> = Rc<dyn Fn(&JulianDate, Option<&mut T>) -> Result<Option<T>, RuntimeError>>;
pub struct CallbackProperty<T: PropertyValue> {
    definition: RefCell<(PropertyCallback<T>, bool)>,
    changed: DefinitionChanged,
    clock: PropertyClock,
}
impl<T: PropertyValue> CallbackProperty<T> {
    pub fn new(callback: PropertyCallback<T>, is_constant: bool) -> Self {
        Self::with_clock(callback, is_constant, system_clock())
    }
    pub fn with_clock(callback: PropertyCallback<T>, is_constant: bool, clock: PropertyClock) -> Self {
        Self { definition: RefCell::new((callback, is_constant)), changed: DefinitionChanged::new(), clock }
    }
    pub fn set_callback(&self, callback: PropertyCallback<T>, is_constant: bool) {
        let changed = {
            let old = self.definition.borrow();
            !Rc::ptr_eq(&old.0, &callback) || old.1 != is_constant
        };
        *self.definition.borrow_mut() = (callback, is_constant);
        if changed {
            self.changed.raise();
        }
    }
}
impl<T: PropertyValue> Property<T> for CallbackProperty<T> {
    fn is_constant(&self) -> bool {
        self.definition.borrow().1
    }
    fn definition_changed(&self) -> &DefinitionChanged {
        &self.changed
    }
    fn get_value(&self, time: Option<&JulianDate>, result: Option<&mut T>) -> Result<Option<T>, RuntimeError> {
        let time = time.copied().unwrap_or_else(|| (self.clock)());
        let callback = self.definition.borrow().0.clone();
        callback(&time, result)
    }
    fn equals(&self, other: &dyn Property<T>) -> Result<bool, RuntimeError> {
        Ok(other.as_any().downcast_ref::<Self>().is_some_and(|o| {
            let a = self.definition.borrow();
            let b = o.definition.borrow();
            Rc::ptr_eq(&a.0, &b.0) && a.1 == b.1
        }))
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}
