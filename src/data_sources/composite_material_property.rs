//! Selects a material property by time interval.
use super::{
    interval_subscriptions::IntervalSubscriptions, ComponentRef, DefinitionChanged, DynamicValue, EntityComponent,
    PropertyIntervals,
};
use crate::core::{JulianDate, RuntimeError};
use std::{any::Any, rc::Rc};

pub struct CompositeMaterialProperty {
    intervals: PropertyIntervals<ComponentRef>,
    changed: DefinitionChanged,
    _subscriptions: IntervalSubscriptions,
}
impl Default for CompositeMaterialProperty {
    fn default() -> Self {
        Self::new()
    }
}
impl CompositeMaterialProperty {
    pub fn new() -> Self {
        let intervals = PropertyIntervals::new(|a: &Option<ComponentRef>, b: &Option<ComponentRef>| match (a, b) {
            (Some(a), Some(b)) => Rc::ptr_eq(a, b),
            (None, None) => true,
            _ => false,
        });
        let changed = DefinitionChanged::new();
        let subscriptions = IntervalSubscriptions::new(&intervals, &changed, |p| p.definition_changed());
        Self { intervals, changed, _subscriptions: subscriptions }
    }
    pub fn get_value<'a>(
        &self,
        time: impl Into<Option<&'a JulianDate>>,
        result: Option<&mut DynamicValue>,
    ) -> Result<Option<DynamicValue>, RuntimeError> {
        let time = time.into().copied().unwrap_or_else(|| (super::property::system_clock())());
        EntityComponent::get_value(self, &time, result)
    }
    pub fn intervals(&self) -> &PropertyIntervals<ComponentRef> {
        &self.intervals
    }
}
impl EntityComponent for CompositeMaterialProperty {
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn definition_changed(&self) -> &DefinitionChanged {
        &self.changed
    }
    fn is_constant(&self) -> bool {
        self.intervals.is_empty()
    }
    fn get_type(&self, time: &JulianDate) -> Result<Option<String>, RuntimeError> {
        self.intervals.find_data(time).map_or(Ok(None), |p| p.get_type(time))
    }
    fn get_value(
        &self,
        time: &JulianDate,
        result: Option<&mut DynamicValue>,
    ) -> Result<Option<DynamicValue>, RuntimeError> {
        self.intervals.find_data(time).map_or(Ok(None), |p| p.get_value(time, result))
    }
    fn equals(&self, other: &dyn EntityComponent) -> Result<bool, RuntimeError> {
        let Some(other) = other.as_any().downcast_ref::<Self>() else { return Ok(false) };
        self.intervals.equals_by(&other.intervals, |a, b| match (a, b) {
            (Some(a), Some(b)) => {
                if Rc::ptr_eq(a, b) {
                    Ok(true)
                } else {
                    a.equals(&**b)
                }
            }
            (None, None) => Ok(true),
            _ => Ok(false),
        })
    }
}
