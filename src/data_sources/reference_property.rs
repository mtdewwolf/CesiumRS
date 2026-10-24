//! Lazy references into entity/component property paths with weak ownership.
use super::{
    entity_collection::EntitySource,
    property::{system_clock, write_result},
    CollectionChange, DefinitionChanged, DynamicValue, Entity, EntityComponent, PositionProperty, PositionPropertyRef,
    Property, PropertyClock, PropertyInput, PropertyValue, Subscription,
};
use crate::core::{Cartesian3, JulianDate, ReferenceFrame, RuntimeError};
use std::{
    any::Any,
    cell::RefCell,
    rc::{Rc, Weak},
};
#[derive(Default)]
struct Resolution {
    entity: Option<Weak<Entity>>,
    property: Option<PropertyInput>,
    subscription: Option<Subscription>,
}
pub struct ReferenceProperty {
    collection: Weak<dyn EntitySource>,
    id: String,
    names: Vec<String>,
    resolution: RefCell<Resolution>,
    changed: DefinitionChanged,
    _collection_subscription: Subscription,
    weak: Weak<ReferenceProperty>,
    clock: PropertyClock,
}
impl ReferenceProperty {
    pub fn new(
        collection: Rc<dyn EntitySource>,
        id: impl Into<String>,
        names: Vec<String>,
    ) -> Result<Rc<Self>, RuntimeError> {
        Self::with_clock(collection, id, names, system_clock())
    }
    pub fn with_clock(
        collection: Rc<dyn EntitySource>,
        id: impl Into<String>,
        names: Vec<String>,
        clock: PropertyClock,
    ) -> Result<Rc<Self>, RuntimeError> {
        let id = id.into();
        if id.is_empty() {
            return Err(RuntimeError::new("targetId is required."));
        }
        if names.is_empty() {
            return Err(RuntimeError::new("targetPropertyNames is required."));
        }
        if names.iter().any(String::is_empty) {
            return Err(RuntimeError::new("reference contains invalid properties."));
        }
        Ok(Rc::new_cyclic(|weak: &Weak<Self>| {
            let target = weak.clone();
            let subscription = collection.collection_changed().add_listener(move |change| {
                if let Some(reference) = target.upgrade() {
                    reference.on_collection_changed(change);
                }
            });
            Self {
                collection: Rc::downgrade(&collection),
                id,
                names,
                resolution: RefCell::new(Resolution::default()),
                changed: DefinitionChanged::new(),
                _collection_subscription: subscription,
                weak: weak.clone(),
                clock,
            }
        }))
    }
    pub fn from_string(collection: Rc<dyn EntitySource>, reference: &str) -> Result<Rc<Self>, RuntimeError> {
        let (mut identifier, mut names, mut token, mut in_identifier, mut escaped) =
            (None, vec![], String::new(), true, false);
        for c in reference.chars() {
            if escaped {
                token.push(c);
                escaped = false;
            } else if c == '\\' {
                escaped = true;
            } else if in_identifier && c == '#' {
                identifier = Some(std::mem::take(&mut token));
                in_identifier = false;
            } else if !in_identifier && c == '.' {
                names.push(std::mem::take(&mut token));
            } else {
                token.push(c);
            }
        }
        names.push(token);
        Self::new(collection, identifier.ok_or_else(|| RuntimeError::new("targetId is required."))?, names)
    }
    pub fn definition_changed(&self) -> &DefinitionChanged {
        &self.changed
    }
    pub fn is_constant(&self) -> bool {
        <Self as Property<DynamicValue>>::is_constant(self)
    }
    pub fn get_value(
        &self,
        time: Option<&JulianDate>,
        result: Option<&mut DynamicValue>,
    ) -> Result<Option<DynamicValue>, RuntimeError> {
        <Self as Property<DynamicValue>>::get_value(self, time, result)
    }
    pub fn equals(&self, other: &Self) -> bool {
        self.equals_reference(other)
    }
    pub fn target_id(&self) -> &str {
        &self.id
    }
    pub fn target_property_names(&self) -> &[String] {
        &self.names
    }
    pub fn target_collection(&self) -> Option<Rc<dyn EntitySource>> {
        self.collection.upgrade()
    }
    pub fn resolved_property(&self) -> Option<PropertyInput> {
        self.resolve()
    }
    fn resolve(&self) -> Option<PropertyInput> {
        let Some(collection) = self.collection.upgrade() else {
            self.clear_resolution();
            return None;
        };
        let current = collection.get_by_id(&self.id);
        let cached = self.resolution.borrow().entity.as_ref().and_then(Weak::upgrade);
        let same = match (&current, &cached) {
            (Some(a), Some(b)) => Rc::ptr_eq(a, b),
            _ => false,
        };
        if same {
            if let Some(value) = self.resolution.borrow().property.as_ref().filter(|v| v.is_defined()) {
                return Some(value.clone());
            }
        } else {
            self.clear_resolution();
        }
        let entity = current?;
        if self.resolution.borrow().entity.as_ref().is_none_or(|e| e.strong_count() == 0) {
            let weak = self.weak.clone();
            let subscription = entity.definition_changed().add_listener(move |change| {
                if let Some(reference) = weak.upgrade() {
                    let notify = {
                        let mut state = reference.resolution.borrow_mut();
                        if state.property.as_ref().is_some_and(PropertyInput::is_defined)
                            && reference.names[0] == change.property.name
                        {
                            state.property = None;
                            true
                        } else {
                            false
                        }
                    };
                    if notify {
                        reference.changed.raise();
                    }
                }
            });
            let old = self.resolution.borrow_mut().subscription.replace(subscription);
            drop(old);
            self.resolution.borrow_mut().entity = Some(Rc::downgrade(&entity));
        }
        let mut value = entity.get_property(&self.names[0]);
        for name in self.names.iter().skip(1) {
            value = match value {
                Some(PropertyInput::Component(component)) => component.get_property(name),
                Some(PropertyInput::Value(DynamicValue::Entity(entity))) => {
                    entity.upgrade().and_then(|e| e.get_property(name))
                }
                Some(PropertyInput::Value(value)) if value.is_defined() => value.get(name).map(PropertyInput::Value),
                _ => None,
            };
        }
        self.resolution.borrow_mut().property = value.clone();
        value
    }
    fn clear_resolution(&self) {
        let old = {
            let mut state = self.resolution.borrow_mut();
            state.entity = None;
            state.property = None;
            state.subscription.take()
        };
        drop(old);
    }
    fn on_collection_changed(&self, change: &CollectionChange) {
        let entity = self.resolution.borrow().entity.as_ref().and_then(Weak::upgrade);
        if entity.as_ref().is_some_and(|entity| change.removed.iter().any(|e| Rc::ptr_eq(e, entity))) {
            self.clear_resolution();
        } else if entity.is_none() && self.resolve().as_ref().is_some_and(PropertyInput::is_defined) {
            self.changed.raise();
        }
    }
    pub fn reference_frame(&self) -> Option<ReferenceFrame> {
        self.resolve().and_then(|v| v.component()).and_then(|p| p.position_property()).map(|p| p.reference_frame())
    }
    pub fn get_value_in_reference_frame(
        &self,
        time: &JulianDate,
        frame: ReferenceFrame,
        result: Option<&mut Cartesian3>,
    ) -> Result<Option<Cartesian3>, RuntimeError> {
        match self.resolve().filter(PropertyInput::is_defined) {
            None => Ok(None),
            Some(PropertyInput::Component(p)) => p
                .position_property()
                .ok_or_else(|| RuntimeError::new("target.getValueInReferenceFrame is not a function"))?
                .get_value_in_reference_frame(time, frame, result),
            _ => Err(RuntimeError::new("target.getValueInReferenceFrame is not a function")),
        }
    }
    pub fn get_type(&self, time: &JulianDate) -> Result<Option<String>, RuntimeError> {
        match self.resolve().filter(PropertyInput::is_defined) {
            None => Ok(None),
            Some(PropertyInput::Component(p)) => p.get_type(time),
            _ => Err(RuntimeError::new("target.getType is not a function")),
        }
    }
    pub fn equals_reference(&self, other: &Self) -> bool {
        Weak::ptr_eq(&self.collection, &other.collection) && self.id == other.id && self.names == other.names
    }
    pub fn as_position(self: &Rc<Self>) -> PositionPropertyRef {
        Rc::new(ReferencePosition(self.clone()))
    }
}
impl Property<DynamicValue> for ReferenceProperty {
    fn is_constant(&self) -> bool {
        match self.resolve() {
            None => true,
            Some(value) if !value.is_defined() => true,
            Some(PropertyInput::Component(p)) => p.is_constant(),
            _ => false,
        }
    }
    fn definition_changed(&self) -> &DefinitionChanged {
        &self.changed
    }
    fn get_value(
        &self,
        time: Option<&JulianDate>,
        result: Option<&mut DynamicValue>,
    ) -> Result<Option<DynamicValue>, RuntimeError> {
        let target = self.resolve();
        let time = time.copied().unwrap_or_else(|| (self.clock)());
        match target.filter(PropertyInput::is_defined) {
            None => Ok(None),
            Some(PropertyInput::Component(p)) => p.get_value(&time, result),
            _ => Err(RuntimeError::new("target.getValue is not a function")),
        }
    }
    fn equals(&self, other: &dyn Property<DynamicValue>) -> Result<bool, RuntimeError> {
        Ok(other.as_any().downcast_ref::<Self>().is_some_and(|o| self.equals_reference(o)))
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}
impl EntityComponent for ReferenceProperty {
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn definition_changed(&self) -> &DefinitionChanged {
        &self.changed
    }
    fn is_constant(&self) -> bool {
        <Self as Property<DynamicValue>>::is_constant(self)
    }
    fn get_value(
        &self,
        time: &JulianDate,
        result: Option<&mut DynamicValue>,
    ) -> Result<Option<DynamicValue>, RuntimeError> {
        <Self as Property<DynamicValue>>::get_value(self, Some(time), result)
    }
    fn equals(&self, other: &dyn EntityComponent) -> Result<bool, RuntimeError> {
        Ok(other.as_any().downcast_ref::<Self>().is_some_and(|o| self.equals_reference(o)))
    }
    fn get_type(&self, time: &JulianDate) -> Result<Option<String>, RuntimeError> {
        self.get_type(time)
    }
    fn position_property(&self) -> Option<PositionPropertyRef> {
        self.weak.upgrade().map(|p| p.as_position())
    }
}
struct ReferencePosition(Rc<ReferenceProperty>);
impl PositionProperty for ReferencePosition {
    fn reference_frame(&self) -> ReferenceFrame {
        self.0.reference_frame().unwrap_or(ReferenceFrame::Fixed)
    }
    fn get_value_in_reference_frame(
        &self,
        time: &JulianDate,
        frame: ReferenceFrame,
        result: Option<&mut Cartesian3>,
    ) -> Result<Option<Cartesian3>, RuntimeError> {
        self.0.get_value_in_reference_frame(time, frame, result)
    }
}
impl Property<Cartesian3> for ReferencePosition {
    fn is_constant(&self) -> bool {
        <ReferenceProperty as Property<DynamicValue>>::is_constant(&self.0)
    }
    fn definition_changed(&self) -> &DefinitionChanged {
        &self.0.changed
    }
    fn get_value(
        &self,
        time: Option<&JulianDate>,
        result: Option<&mut Cartesian3>,
    ) -> Result<Option<Cartesian3>, RuntimeError> {
        let mut result = result;
        let mut scratch = result.as_deref().map(|v| DynamicValue::Cartesian3(*v));
        let value = <ReferenceProperty as Property<DynamicValue>>::get_value(&self.0, time, scratch.as_mut())?;
        if let (Some(DynamicValue::Cartesian3(value)), Some(out)) = (scratch, result.as_deref_mut()) {
            *out = value;
        }
        let value = match value {
            None | Some(DynamicValue::Null) => None,
            Some(DynamicValue::Cartesian3(v)) => Some(v),
            _ => return Err(RuntimeError::new("reference does not resolve to a Cartesian3")),
        };
        Ok(write_result(value, result))
    }
    fn equals(&self, other: &dyn Property<Cartesian3>) -> Result<bool, RuntimeError> {
        Ok(other.as_any().downcast_ref::<Self>().is_some_and(|o| self.0.equals_reference(&o.0)))
    }
    fn as_any(&self) -> &dyn Any {
        self
    }
}
