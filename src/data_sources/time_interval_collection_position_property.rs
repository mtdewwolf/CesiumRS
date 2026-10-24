use super::{position_property::*, DefinitionChanged, Property, TimeIntervalCollectionProperty};
use crate::core::*;
use std::{any::Any, ops::Deref};
pub struct TimeIntervalCollectionPositionProperty {
    value: TimeIntervalCollectionProperty<Cartesian3>,
    frame: ReferenceFrame,
    context: PositionContext,
}
impl TimeIntervalCollectionPositionProperty {
    pub fn new(frame: ReferenceFrame) -> Self {
        Self::with_context(frame, PositionContext::default())
    }
    pub fn with_context(frame: ReferenceFrame, context: PositionContext) -> Self {
        Self { value: TimeIntervalCollectionProperty::with_clock(context.clock.clone()), frame, context }
    }
}
impl Deref for TimeIntervalCollectionPositionProperty {
    type Target = TimeIntervalCollectionProperty<Cartesian3>;
    fn deref(&self) -> &Self::Target {
        &self.value
    }
}
impl PositionProperty for TimeIntervalCollectionPositionProperty {
    fn reference_frame(&self) -> ReferenceFrame {
        self.frame
    }
    fn get_value_in_reference_frame(
        &self,
        time: &JulianDate,
        frame: ReferenceFrame,
        result: Option<&mut Cartesian3>,
    ) -> Result<Option<Cartesian3>, RuntimeError> {
        self.context.convert(
            time,
            self.value.get_value(Some(time), None)?,
            FrameConversion { input: self.frame, output: frame },
            result,
        )
    }
}
impl Property<Cartesian3> for TimeIntervalCollectionPositionProperty {
    fn is_constant(&self) -> bool {
        self.value.is_constant()
    }
    fn definition_changed(&self) -> &DefinitionChanged {
        self.value.definition_changed()
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
        Ok(self.frame == o.frame && self.value.equals(&o.value)?)
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl Default for TimeIntervalCollectionPositionProperty {
    fn default() -> Self {
        Self::new(ReferenceFrame::Fixed)
    }
}
