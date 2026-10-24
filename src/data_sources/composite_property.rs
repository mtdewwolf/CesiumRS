use super::{
    property::{self, system_clock},
    DefinitionChanged, Property, PropertyClock, PropertyIntervals, PropertyRef, PropertyValue, Subscription,
};
use crate::core::{JulianDate, RuntimeError};
use std::{any::Any, cell::RefCell, rc::Rc};
pub struct CompositeProperty<T: PropertyValue> {
    intervals: PropertyIntervals<PropertyRef<T>>,
    changed: DefinitionChanged,
    _children: Rc<RefCell<Vec<Subscription>>>,
    _subscription: Subscription,
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
        let children = Rc::new(RefCell::new(Vec::<Subscription>::new()));
        let weak = changed.downgrade();
        let weak_core = intervals.weak_core();
        let weak_children = Rc::downgrade(&children);
        let subscription = intervals.changed_event().add_listener(move |_| {
            let (Some(core), Some(children), Some(event)) =
                (weak_core.upgrade(), weak_children.upgrade(), weak.upgrade())
            else {
                return;
            };
            let old = std::mem::take(&mut *children.borrow_mut());
            drop(old);
            let properties = core.borrow().intervals().iter().filter_map(|i| i.data.clone()).collect::<Vec<_>>();
            let forward = event.downgrade();
            let bridge: super::PropertyListener = Rc::new(move |_| {
                if let Some(event) = forward.upgrade() {
                    event.raise();
                }
            });
            let mut subscribed = Vec::<DefinitionChanged>::new();
            let mut subscriptions = vec![];
            for p in properties {
                let e = p.definition_changed();
                if !subscribed.iter().any(|v| v.ptr_eq(e)) {
                    subscriptions.push(e.subscribe(bridge.clone()));
                    subscribed.push(e.clone());
                }
            }
            *children.borrow_mut() = subscriptions;
            event.raise();
        });
        Self { intervals, changed, _children: children, _subscription: subscription, clock }
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
