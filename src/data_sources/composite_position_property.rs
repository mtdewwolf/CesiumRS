use super::{
    interval_subscriptions::IntervalSubscriptions, position_property::*, DefinitionChanged, Property, PropertyIntervals,
};
use crate::core::*;
use std::{any::Any, cell::Cell};
pub struct CompositePositionProperty {
    intervals: PropertyIntervals<PositionPropertyRef>,
    changed: DefinitionChanged,
    _subscriptions: IntervalSubscriptions,
    frame: Cell<ReferenceFrame>,
    context: PositionContext,
}
impl CompositePositionProperty {
    pub fn new(frame: ReferenceFrame) -> Self {
        Self::with_context(frame, PositionContext::default())
    }
    pub fn with_context(frame: ReferenceFrame, context: PositionContext) -> Self {
        let intervals =
            PropertyIntervals::new(|a: &Option<PositionPropertyRef>, b: &Option<PositionPropertyRef>| same_ref(a, b));
        let changed = DefinitionChanged::new();
        let subscriptions = IntervalSubscriptions::new(&intervals, &changed, |p| p.definition_changed());
        Self { intervals, changed, _subscriptions: subscriptions, frame: Cell::new(frame), context }
    }
    pub fn intervals(&self) -> &PropertyIntervals<PositionPropertyRef> {
        &self.intervals
    }
    /// This metadata setter emits no definition event, matching Cesium.
    pub fn set_reference_frame(&self, frame: ReferenceFrame) {
        self.frame.set(frame);
    }
}
impl PositionProperty for CompositePositionProperty {
    fn reference_frame(&self) -> ReferenceFrame {
        self.frame.get()
    }
    fn get_value_in_reference_frame(
        &self,
        time: &JulianDate,
        frame: ReferenceFrame,
        result: Option<&mut Cartesian3>,
    ) -> Result<Option<Cartesian3>, RuntimeError> {
        match self.intervals.find_data(time) {
            Some(p) => p.get_value_in_reference_frame(time, frame, result),
            None => Ok(None),
        }
    }
}
impl Property<Cartesian3> for CompositePositionProperty {
    fn is_constant(&self) -> bool {
        self.intervals.is_empty()
    }
    fn definition_changed(&self) -> &DefinitionChanged {
        &self.changed
    }
    fn get_value(
        &self,
        time: Option<&JulianDate>,
        result: Option<&mut Cartesian3>,
    ) -> Result<Option<Cartesian3>, RuntimeError> {
        self.get_value_in_reference_frame(&self.context.time(time), ReferenceFrame::Fixed, result)
    }
    fn equals(&self, other: &dyn Property<Cartesian3>) -> Result<bool, RuntimeError> {
        let Some(o) = other.as_any().downcast_ref::<Self>() else { return Ok(false) };
        Ok(self.frame.get() == o.frame.get()
            && self.intervals.equals_by(&o.intervals, |a, b| position_equals(a.as_ref(), b.as_ref()))?)
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl Default for CompositePositionProperty {
    fn default() -> Self {
        Self::new(ReferenceFrame::Fixed)
    }
}
