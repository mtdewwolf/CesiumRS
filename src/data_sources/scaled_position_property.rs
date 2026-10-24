use super::{position_property::*, property::write_result, DefinitionChanged, Property, Subscription};
use crate::core::*;
use std::{any::Any, cell::RefCell};
pub struct ScaledPositionProperty {
    value: RefCell<Option<PositionPropertyRef>>,
    subscription: RefCell<Option<Subscription>>,
    changed: DefinitionChanged,
    context: PositionContext,
}
impl ScaledPositionProperty {
    pub fn new(value: Option<PositionPropertyRef>) -> Self {
        Self::with_context(value, PositionContext::default())
    }
    pub fn with_context(value: Option<PositionPropertyRef>, context: PositionContext) -> Self {
        let p = Self {
            value: RefCell::new(None),
            subscription: RefCell::new(None),
            changed: DefinitionChanged::new(),
            context,
        };
        p.set_value(value);
        p
    }
    pub fn set_value(&self, value: Option<PositionPropertyRef>) {
        if same_ref(&self.value.borrow(), &value) {
            return;
        }
        let old = self.subscription.borrow_mut().take();
        drop(old);
        *self.value.borrow_mut() = value.clone();
        *self.subscription.borrow_mut() = value.as_ref().map(|p| forward(p.definition_changed(), &self.changed));
        self.changed.raise();
    }
}
impl PositionProperty for ScaledPositionProperty {
    fn reference_frame(&self) -> ReferenceFrame {
        self.value.borrow().as_ref().map_or(ReferenceFrame::Fixed, |p| p.reference_frame())
    }
    fn get_value_in_reference_frame(
        &self,
        time: &JulianDate,
        frame: ReferenceFrame,
        mut result: Option<&mut Cartesian3>,
    ) -> Result<Option<Cartesian3>, RuntimeError> {
        let property = self.value.borrow().clone();
        let Some(p) = property else { return Ok(None) };
        let value = p.get_value_in_reference_frame(time, frame, result.as_deref_mut())?;
        let value = value.and_then(|v| (self.context.ellipsoid)().scale_to_geodetic_surface(v));
        Ok(write_result(value, result))
    }
}
impl Property<Cartesian3> for ScaledPositionProperty {
    fn is_constant(&self) -> bool {
        let value = self.value.borrow().clone();
        value.is_none_or(|p| p.is_constant())
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
        Ok(other.as_any().downcast_ref::<Self>().is_some_and(|o| same_ref(&self.value.borrow(), &o.value.borrow())))
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl Default for ScaledPositionProperty {
    fn default() -> Self {
        Self::new(None)
    }
}
