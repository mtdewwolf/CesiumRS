use super::{
    position_property::{forward, PositionContext},
    property::write_result,
    DefinitionChanged, Property, PropertyRef, Subscription, VelocityVectorProperty,
};
use crate::core::*;
use std::{any::Any, cell::RefCell, rc::Rc};
pub struct VelocityOrientationProperty {
    velocity: VelocityVectorProperty,
    ellipsoid: RefCell<Rc<Ellipsoid>>,
    changed: DefinitionChanged,
    _subscription: Subscription,
    context: PositionContext,
}
impl VelocityOrientationProperty {
    pub fn new(position: Option<PropertyRef<Cartesian3>>, ellipsoid: Option<Rc<Ellipsoid>>) -> Self {
        Self::with_context(position, ellipsoid, PositionContext::default())
    }
    pub fn with_context(
        position: Option<PropertyRef<Cartesian3>>,
        ellipsoid: Option<Rc<Ellipsoid>>,
        context: PositionContext,
    ) -> Self {
        let velocity = VelocityVectorProperty::with_context(position, true, context.clone());
        let changed = DefinitionChanged::new();
        let subscription = forward(velocity.definition_changed(), &changed);
        Self {
            velocity,
            ellipsoid: RefCell::new(ellipsoid.unwrap_or_else(|| Rc::new((context.ellipsoid)()))),
            changed,
            _subscription: subscription,
            context,
        }
    }
    pub fn position(&self) -> Option<PropertyRef<Cartesian3>> {
        self.velocity.position()
    }
    pub fn set_position(&self, position: Option<PropertyRef<Cartesian3>>) {
        self.velocity.set_position(position);
    }
    pub fn ellipsoid(&self) -> Rc<Ellipsoid> {
        self.ellipsoid.borrow().clone()
    }
    pub fn set_ellipsoid(&self, ellipsoid: Rc<Ellipsoid>) {
        let changed = !Rc::ptr_eq(&self.ellipsoid.borrow(), &ellipsoid);
        *self.ellipsoid.borrow_mut() = ellipsoid;
        if changed {
            self.changed.raise();
        }
    }
}
impl Property<Quaternion> for VelocityOrientationProperty {
    fn is_constant(&self) -> bool {
        self.velocity.is_constant()
    }
    fn definition_changed(&self) -> &DefinitionChanged {
        &self.changed
    }
    fn get_value(
        &self,
        time: Option<&JulianDate>,
        result: Option<&mut Quaternion>,
    ) -> Result<Option<Quaternion>, RuntimeError> {
        let time = self.context.time(time);
        let mut position = Cartesian3::ZERO;
        let Some(velocity) = self.velocity.evaluate(Some(&time), None, Some(&mut position))? else { return Ok(None) };
        let matrix = Transforms::rotation_matrix_from_position_velocity(position, velocity, &self.ellipsoid.borrow());
        Ok(write_result(Some(Quaternion::from_rotation_matrix(&matrix)), result))
    }
    fn equals(&self, other: &dyn Property<Quaternion>) -> Result<bool, RuntimeError> {
        let Some(o) = other.as_any().downcast_ref::<Self>() else { return Ok(false) };
        if std::ptr::eq(self, o) {
            return Ok(true);
        }
        let (a, b) = (self.ellipsoid(), o.ellipsoid());
        Ok(self.velocity.equals(&o.velocity)? && (Rc::ptr_eq(&a, &b) || a.radii == b.radii))
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}

impl Default for VelocityOrientationProperty {
    fn default() -> Self {
        Self::new(None, None)
    }
}
