use super::{
    position_property::{forward, same_ref, PositionContext},
    property::{self, write_result},
    DefinitionChanged, Property, PropertyRef, Subscription,
};
use crate::core::*;
use std::{
    any::Any,
    cell::{Cell, RefCell},
};
const STEP: f64 = 1.0 / 60.0;
pub struct VelocityVectorProperty {
    position: RefCell<Option<PropertyRef<Cartesian3>>>,
    subscription: RefCell<Option<Subscription>>,
    changed: DefinitionChanged,
    normalize: Cell<bool>,
    context: PositionContext,
}
impl VelocityVectorProperty {
    pub fn new(position: Option<PropertyRef<Cartesian3>>, normalize: bool) -> Self {
        Self::with_context(position, normalize, PositionContext::default())
    }
    pub fn with_context(position: Option<PropertyRef<Cartesian3>>, normalize: bool, context: PositionContext) -> Self {
        let p = Self {
            position: RefCell::new(None),
            subscription: RefCell::new(None),
            changed: DefinitionChanged::new(),
            normalize: Cell::new(normalize),
            context,
        };
        p.set_position(position);
        p
    }
    pub fn position(&self) -> Option<PropertyRef<Cartesian3>> {
        self.position.borrow().clone()
    }
    pub fn set_position(&self, position: Option<PropertyRef<Cartesian3>>) {
        if same_ref(&self.position.borrow(), &position) {
            return;
        }
        let old = self.subscription.borrow_mut().take();
        drop(old);
        *self.position.borrow_mut() = position.clone();
        *self.subscription.borrow_mut() = position.as_ref().map(|p| forward(p.definition_changed(), &self.changed));
        self.changed.raise();
    }
    pub fn normalize(&self) -> bool {
        self.normalize.get()
    }
    pub fn set_normalize(&self, value: bool) {
        if self.normalize.replace(value) != value {
            self.changed.raise();
        }
    }
    pub(crate) fn evaluate(
        &self,
        time: Option<&JulianDate>,
        velocity_result: Option<&mut Cartesian3>,
        position_result: Option<&mut Cartesian3>,
    ) -> Result<Option<Cartesian3>, RuntimeError> {
        let time = self.context.time(time);
        let property = self.position();
        if property.as_ref().is_none_or(|p| p.is_constant()) {
            return Ok(write_result((!self.normalize.get()).then_some(Cartesian3::ZERO), velocity_result));
        }
        let property = property.unwrap();
        // Evaluate both dates even when the current position is missing, as in Cesium.
        let mut first_result = Cartesian3::ZERO;
        let mut second_result = Cartesian3::ZERO;
        let current = property.get_value(Some(&time), Some(&mut first_result))?;
        let next = property.get_value(Some(&time.add_seconds(STEP)), Some(&mut second_result))?;
        let Some(mut first) = current else { return Ok(None) };
        let second = if let Some(next) = next {
            next
        } else {
            let Some(previous) = property.get_value(Some(&time.add_seconds(-STEP)), Some(&mut second_result))? else {
                return Ok(None);
            };
            let next = first;
            first = previous;
            next
        };
        if first == second {
            return Ok(write_result((!self.normalize.get()).then_some(Cartesian3::ZERO), velocity_result));
        }
        write_result(Some(first), position_result);
        let difference = second.subtract(first);
        let velocity = if self.normalize.get() { difference.normalize() } else { difference.divide_by_scalar(STEP) };
        Ok(write_result(Some(velocity), velocity_result))
    }
}
impl Property<Cartesian3> for VelocityVectorProperty {
    fn is_constant(&self) -> bool {
        self.position().is_none_or(|p| p.is_constant())
    }
    fn definition_changed(&self) -> &DefinitionChanged {
        &self.changed
    }
    fn get_value(
        &self,
        time: Option<&JulianDate>,
        result: Option<&mut Cartesian3>,
    ) -> Result<Option<Cartesian3>, RuntimeError> {
        self.evaluate(time, result, None)
    }
    fn equals(&self, other: &dyn Property<Cartesian3>) -> Result<bool, RuntimeError> {
        let Some(o) = other.as_any().downcast_ref::<Self>() else { return Ok(false) };
        if std::ptr::eq(self, o) {
            return Ok(true);
        }
        property::equals(self.position().as_ref(), o.position().as_ref())
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl Default for VelocityVectorProperty {
    fn default() -> Self {
        Self::new(None, true)
    }
}
