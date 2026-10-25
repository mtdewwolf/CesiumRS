use super::{
    interval_subscriptions::IntervalSubscriptions,
    property::{self, system_clock},
    DefinitionChanged, Property, PropertyClock, PropertyIntervals, PropertyRef, PropertyValue,
};
use crate::core::{JulianDate, RuntimeError};
use std::{any::Any, rc::Rc};
pub struct CompositeProperty<T: PropertyValue> {
    intervals: PropertyIntervals<PropertyRef<T>>,
    changed: DefinitionChanged,
    _subscriptions: IntervalSubscriptions,
    clock: PropertyClock,
}
impl<T: PropertyValue> Default for CompositeProperty<T> {
    fn default() -> Self {
        Self::new()
    }
}
impl<T: PropertyValue> CompositeProperty<T> {
    pub fn new() -> Self {
        Self::with_clock(system_clock())
    }
    pub fn with_clock(clock: PropertyClock) -> Self {
        let intervals = PropertyIntervals::new(|a: &Option<PropertyRef<T>>, b: &Option<PropertyRef<T>>| match (a, b) {
            (None, None) => true,
            (Some(a), Some(b)) => Rc::ptr_eq(a, b),
            _ => false,
        });
        let changed = DefinitionChanged::new();
        let subscriptions = IntervalSubscriptions::new(&intervals, &changed, |p| p.definition_changed());
        Self { intervals, changed, _subscriptions: subscriptions, clock }
    }
    pub fn intervals(&self) -> &PropertyIntervals<PropertyRef<T>> {
        &self.intervals
    }
}
impl<T: PropertyValue> Property<T> for CompositeProperty<T> {
    fn is_constant(&self) -> bool {
        self.intervals.is_empty()
    }
    fn definition_changed(&self) -> &DefinitionChanged {
        &self.changed
    }
    fn get_value(&self, time: Option<&JulianDate>, result: Option<&mut T>) -> Result<Option<T>, RuntimeError> {
        let time = time.copied().unwrap_or_else(|| (self.clock)());
        match self.intervals.find_data(&time) {
            Some(p) => p.get_value(Some(&time), result),
            None => Ok(None),
        }
    }
    fn equals(&self, other: &dyn Property<T>) -> Result<bool, RuntimeError> {
        let Some(o) = other.as_any().downcast_ref::<Self>() else { return Ok(false) };
        self.intervals.equals_by(&o.intervals, |a, b| property::equals(a.as_ref(), b.as_ref()))
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}
