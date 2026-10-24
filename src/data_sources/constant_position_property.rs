use super::{position_property::*, ConstantProperty, DefinitionChanged, Property};
use crate::core::*;
use std::{any::Any, cell::Cell};
pub struct ConstantPositionProperty {
    value: ConstantProperty<Cartesian3>,
    frame: Cell<ReferenceFrame>,
    context: PositionContext,
}
impl ConstantPositionProperty {
    pub fn new(value: Option<Cartesian3>, frame: ReferenceFrame) -> Self {
        Self::with_context(value, frame, PositionContext::default())
    }
    pub fn with_context(value: Option<Cartesian3>, frame: ReferenceFrame, context: PositionContext) -> Self {
        Self { value: ConstantProperty::new(value), frame: Cell::new(frame), context }
    }
    pub fn set_value(&self, value: Option<Cartesian3>, frame: Option<ReferenceFrame>) {
        let frame_changed = frame.is_some_and(|frame| self.frame.replace(frame) != frame);
        let value_changed = self.value.value_of() != value;
        self.value.set_value(value);
        if frame_changed && !value_changed {
            self.definition_changed().raise();
        }
    }
}
impl PositionProperty for ConstantPositionProperty {
    fn reference_frame(&self) -> ReferenceFrame {
        self.frame.get()
    }
    fn get_value_in_reference_frame(
        &self,
        time: &JulianDate,
        frame: ReferenceFrame,
        result: Option<&mut Cartesian3>,
    ) -> Result<Option<Cartesian3>, RuntimeError> {
        self.context.convert(
            time,
            self.value.value_of(),
            FrameConversion { input: self.frame.get(), output: frame },
            result,
        )
    }
}
impl Property<Cartesian3> for ConstantPositionProperty {
    fn is_constant(&self) -> bool {
        self.value.value_of().is_none() || self.frame.get() == ReferenceFrame::Fixed
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
        Ok(std::ptr::eq(self, o) || (self.frame.get() == o.frame.get() && self.value.equals(&o.value)?))
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl Default for ConstantPositionProperty {
    fn default() -> Self {
        Self::new(None, ReferenceFrame::Fixed)
    }
}
