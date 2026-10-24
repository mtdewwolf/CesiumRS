use super::{
    property::{self, system_clock, write_result},
    DefinitionChanged, Property, PropertyClock, PropertyListener, PropertyRef, PropertyValue, Subscription,
};
use crate::core::{JulianDate, RuntimeError};
use std::{any::Any, cell::RefCell, rc::Rc};
pub struct PropertyArray<T: PropertyValue> {
    value: RefCell<Option<Vec<PropertyRef<T>>>>,
    subscriptions: RefCell<Vec<Subscription>>,
    bridge: PropertyListener,
    changed: DefinitionChanged,
    clock: PropertyClock,
}
impl<T: PropertyValue> PropertyArray<T> {
    pub fn new(value: Option<Vec<PropertyRef<T>>>) -> Self {
        Self::with_clock(value, system_clock())
    }
    pub fn with_clock(value: Option<Vec<PropertyRef<T>>>, clock: PropertyClock) -> Self {
        let changed = DefinitionChanged::new();
        let weak = changed.downgrade();
        let result = Self {
            value: RefCell::new(None),
            subscriptions: RefCell::new(vec![]),
            bridge: Rc::new(move |_| {
                if let Some(event) = weak.upgrade() {
                    event.raise();
                }
            }),
            changed,
            clock,
        };
        result.set_value(value);
        result
    }
    pub fn set_value(&self, value: Option<Vec<PropertyRef<T>>>) {
        let old = std::mem::take(&mut *self.subscriptions.borrow_mut());
        drop(old);
        *self.value.borrow_mut() = value;
        let properties = self.value.borrow().clone();
        let mut subscribed = Vec::<DefinitionChanged>::new();
        let mut subscriptions = vec![];
        for p in properties.iter().flatten() {
            let event = p.definition_changed();
            if !subscribed.iter().any(|s| s.ptr_eq(event)) {
                subscriptions.push(event.subscribe(self.bridge.clone()));
                subscribed.push(event.clone());
            }
        }
        *self.subscriptions.borrow_mut() = subscriptions;
        self.changed.raise();
    }
}
impl<T: PropertyValue> Property<Vec<T>> for PropertyArray<T> {
    fn is_constant(&self) -> bool {
        let properties = self.value.borrow().clone();
        properties.iter().flatten().all(|p| p.is_constant())
    }
    fn definition_changed(&self) -> &DefinitionChanged {
        &self.changed
    }
    fn get_value(
        &self,
        time: Option<&JulianDate>,
        mut result: Option<&mut Vec<T>>,
    ) -> Result<Option<Vec<T>>, RuntimeError> {
        let time = time.copied().unwrap_or_else(|| (self.clock)());
        // A callback may replace this array's definition. Keep its original
        // children alive and release the borrow before evaluating any of them.
        let Some(properties) = self.value.borrow().clone() else { return Ok(None) };
        let mut values = Vec::with_capacity(properties.len());
        for (i, property) in properties.iter().enumerate() {
            if let Some(value) = property
                .get_value(Some(&time), result.as_deref_mut().and_then(|r| r.get_mut(i)))?
                .filter(PropertyValue::is_defined)
            {
                values.push(value);
            }
        }
        Ok(write_result(Some(values), result))
    }
    fn equals(&self, other: &dyn Property<Vec<T>>) -> Result<bool, RuntimeError> {
        let Some(other) = other.as_any().downcast_ref::<Self>() else { return Ok(false) };
        if std::ptr::eq(self, other) {
            return Ok(true);
        }
        let (a, b) = (self.value.borrow().clone(), other.value.borrow().clone());
        property::array_equals(a.as_deref(), b.as_deref())
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}
