//! Optional simulation clock settings supplied by a data source.
use super::{
    create_raw_property_descriptor, ChangeEvent, DefinitionChanged, DynamicValue, PropertyChange, PropertyDescriptor,
    PropertyInput,
};
use crate::core::{Clock, ClockRange, ClockStep, JulianDate};
use std::rc::Rc;

pub struct DataSourceClock {
    start: PropertyDescriptor,
    stop: PropertyDescriptor,
    current: PropertyDescriptor,
    range: PropertyDescriptor,
    step: PropertyDescriptor,
    multiplier: PropertyDescriptor,
    changed: ChangeEvent<PropertyChange>,
}
macro_rules! date_slot {
    ($get:ident, $set:ident, $field:ident) => {
        pub fn $get(&self) -> Option<Rc<JulianDate>> {
            match self.$field.get()? {
                PropertyInput::Value(DynamicValue::Date(date)) => Some(date),
                _ => unreachable!("date slot has a typed setter"),
            }
        }
        pub fn $set(&self, value: Option<Rc<JulianDate>>) {
            self.$field.set(value.map(PropertyInput::value)).expect("live raw date slot");
        }
    };
}
impl Default for DataSourceClock {
    fn default() -> Self {
        Self::new()
    }
}
impl Clone for DataSourceClock {
    fn clone(&self) -> Self {
        let target = Self::new();
        self.clone_into(&target);
        target
    }
}
impl DataSourceClock {
    pub fn new() -> Self {
        let changed = ChangeEvent::new();
        let slot = |name| create_raw_property_descriptor(name, false, &changed);
        Self {
            start: slot("startTime"),
            stop: slot("stopTime"),
            current: slot("currentTime"),
            range: slot("clockRange"),
            step: slot("clockStep"),
            multiplier: slot("multiplier"),
            changed,
        }
    }
    pub fn definition_changed(&self) -> &DefinitionChanged {
        self.changed.definition_changed()
    }
    pub fn definition_changes(&self) -> &ChangeEvent<PropertyChange> {
        &self.changed
    }
    date_slot!(start_time, set_start_time, start);
    date_slot!(stop_time, set_stop_time, stop);
    date_slot!(current_time, set_current_time, current);
    pub fn clock_range(&self) -> Option<ClockRange> {
        Some(match Self::number(&self.range)? {
            0. => ClockRange::Unbounded,
            1. => ClockRange::Clamped,
            2. => ClockRange::LoopStop,
            _ => unreachable!("clockRange has a typed setter"),
        })
    }
    pub fn set_clock_range(&self, value: Option<ClockRange>) {
        Self::set_number(&self.range, value.map(|v| v as i32 as f64));
    }
    pub fn clock_step(&self) -> Option<ClockStep> {
        Some(match Self::number(&self.step)? {
            0. => ClockStep::TickDependent,
            1. => ClockStep::SystemClockMultiplier,
            2. => ClockStep::SystemClock,
            _ => unreachable!("clockStep has a typed setter"),
        })
    }
    pub fn set_clock_step(&self, value: Option<ClockStep>) {
        Self::set_number(&self.step, value.map(|v| v as i32 as f64));
    }
    pub fn multiplier(&self) -> Option<f64> {
        Self::number(&self.multiplier)
    }
    pub fn set_multiplier(&self, value: Option<f64>) {
        Self::set_number(&self.multiplier, value);
    }
    fn number(slot: &PropertyDescriptor) -> Option<f64> {
        match slot.get()? {
            PropertyInput::Value(DynamicValue::Number(n)) => Some(n),
            _ => unreachable!("numeric slot has a typed setter"),
        }
    }
    fn set_number(slot: &PropertyDescriptor, value: Option<f64>) {
        slot.set(value.map(PropertyInput::value)).expect("live raw numeric slot");
    }
    fn slots(&self) -> [&PropertyDescriptor; 6] {
        [&self.start, &self.stop, &self.current, &self.range, &self.step, &self.multiplier]
    }
    pub fn clone_into(&self, target: &Self) {
        for (to, from) in target.slots().into_iter().zip(self.slots()) {
            to.set(from.get()).expect("live raw slot");
        }
    }
    pub fn merge(&self, source: &Self) {
        for (to, from) in self.slots().into_iter().zip(source.slots()) {
            if to.get().is_none() {
                to.set(from.get()).expect("live raw slot");
            }
        }
    }
    pub fn equals(&self, other: &Self) -> bool {
        std::ptr::eq(self, other)
            || (self.start_time() == other.start_time()
                && self.stop_time() == other.stop_time()
                && self.current_time() == other.current_time()
                && self.clock_range() == other.clock_range()
                && self.clock_step() == other.clock_step()
                && self.multiplier() == other.multiplier())
    }
    pub fn get_value(&self, result: Option<Clock>) -> Clock {
        let mut result = result.unwrap_or_else(|| Clock::new(Default::default()).expect("valid default clock"));
        self.get_value_into(&mut result);
        result
    }
    pub fn get_value_into(&self, result: &mut Clock) {
        if let Some(v) = self.start_time() {
            result.start_time = *v;
        }
        if let Some(v) = self.stop_time() {
            result.stop_time = *v;
        }
        if let Some(v) = self.current_time() {
            result.set_current_time(*v);
        }
        if let Some(v) = self.clock_range() {
            result.clock_range = v;
        }
        result.set_multiplier(self.multiplier().unwrap_or(result.multiplier()));
        result.set_clock_step(self.clock_step().unwrap_or(result.clock_step()));
    }
}
