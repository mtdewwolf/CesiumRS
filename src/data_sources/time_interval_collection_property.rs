use super::{
    property::{system_clock, write_result},
    DefinitionChanged, Property, PropertyClock, PropertyIntervals, PropertyValue, Subscription,
};
use crate::core::{JulianDate, RuntimeError};
use std::{any::Any, rc::Rc};
pub struct TimeIntervalCollectionProperty<T: PropertyValue> {
    intervals: PropertyIntervals<Rc<T>>,
    changed: DefinitionChanged,
    _subscription: Subscription,
    clock: PropertyClock,
}
impl<T: PropertyValue> Default for TimeIntervalCollectionProperty<T> {
    fn default() -> Self {
        Self::new()
    }
}
impl<T: PropertyValue> TimeIntervalCollectionProperty<T> {
    pub fn new() -> Self {
        Self::with_clock(system_clock())
    }
    pub fn with_clock(clock: PropertyClock) -> Self {
        let intervals = PropertyIntervals::new(|a: &Option<Rc<T>>, b: &Option<Rc<T>>| match (a, b) {
            (None, None) => true,
            (Some(a), Some(b)) => T::strict_equals(a.as_ref(), b.as_ref()),
            _ => false,
        });
        let changed = DefinitionChanged::new();
        let weak = changed.downgrade();
        let subscription = intervals.changed_event().add_listener(move |_| {
            if let Some(e) = weak.upgrade() {
                e.raise();
            }
        });
        Self { intervals, changed, _subscription: subscription, clock }
    }
    pub fn intervals(&self) -> &PropertyIntervals<Rc<T>> {
        &self.intervals
    }
}
impl<T: PropertyValue> Property<T> for TimeIntervalCollectionProperty<T> {
    fn is_constant(&self) -> bool {
        self.intervals.is_empty()
    }
    fn definition_changed(&self) -> &DefinitionChanged {
        &self.changed
    }
    fn get_value(&self, time: Option<&JulianDate>, result: Option<&mut T>) -> Result<Option<T>, RuntimeError> {
        let time = time.copied().unwrap_or_else(|| (self.clock)());
        Ok(write_result(self.intervals.find_data(&time).map(|v| (*v).clone()), result))
    }
    fn equals(&self, other: &dyn Property<T>) -> Result<bool, RuntimeError> {
        let Some(o) = other.as_any().downcast_ref::<Self>() else { return Ok(false) };
        self.intervals.equals_by(&o.intervals, |a, b| match (a, b) {
            (None, None) => Ok(true),
            (Some(a), Some(b)) => {
                if T::strict_equals(a.as_ref(), b.as_ref()) {
                    Ok(true)
                } else if T::is_defined(a.as_ref()) {
                    T::equals_method(a.as_ref(), b.as_ref())
                } else {
                    Ok(false)
                }
            }
            _ => Ok(false),
        })
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}
