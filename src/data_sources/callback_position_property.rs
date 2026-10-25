use super::{position_property::*, CallbackProperty, DefinitionChanged, Property, PropertyCallback};
use crate::core::*;
use std::any::Any;
pub struct CallbackPositionProperty {
    value: CallbackProperty<Cartesian3>,
    frame: ReferenceFrame,
    context: PositionContext,
}
impl CallbackPositionProperty {
    pub fn new(callback: PropertyCallback<Cartesian3>, is_constant: bool, frame: ReferenceFrame) -> Self {
        Self::with_context(callback, is_constant, frame, PositionContext::default())
    }
    pub fn with_context(
        callback: PropertyCallback<Cartesian3>,
        is_constant: bool,
        frame: ReferenceFrame,
        context: PositionContext,
    ) -> Self {
        Self { value: CallbackProperty::with_clock(callback, is_constant, context.clock.clone()), frame, context }
    }
    pub fn set_callback(&self, callback: PropertyCallback<Cartesian3>, is_constant: bool) {
        self.value.set_callback(callback, is_constant);
    }
}
impl PositionProperty for CallbackPositionProperty {
    fn reference_frame(&self) -> ReferenceFrame {
        self.frame
    }
    fn get_value_in_reference_frame(
        &self,
        time: &JulianDate,
        frame: ReferenceFrame,
        mut result: Option<&mut Cartesian3>,
    ) -> Result<Option<Cartesian3>, RuntimeError> {
        let value = self.value.get_value(Some(time), result.as_deref_mut())?;
        self.context.convert(time, value, FrameConversion { input: self.frame, output: frame }, result)
    }
}
impl Property<Cartesian3> for CallbackPositionProperty {
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
