//! Owned property slots replace JavaScript accessor descriptors.
use super::{
    ChangeEvent, ComponentRef, ConstantProperty, DynamicValue, PropertyComponent, PropertyInput, Subscription,
    WeakChangeEvent,
};
use crate::core::RuntimeError;
use std::{
    borrow::Cow,
    cell::{Cell, RefCell},
    rc::Rc,
};
pub type ComponentFactory = Rc<dyn Fn(DynamicValue) -> Result<ComponentRef, RuntimeError>>;
#[derive(Clone)]
pub struct PropertyChange {
    pub name: String,
    pub new_value: Option<PropertyInput>,
    pub old_value: Option<PropertyInput>,
    pub child_changed: bool,
}
pub fn constant_component(value: DynamicValue) -> Result<ComponentRef, RuntimeError> {
    Ok(PropertyComponent::wrap(Rc::new(ConstantProperty::new(Some(value)))))
}
enum Factory {
    None,
    Constant,
    Custom(ComponentFactory),
}
struct Slot {
    name: Cow<'static, str>,
    configurable: bool,
    factory: Factory,
    value: RefCell<Option<PropertyInput>>,
    subscription: RefCell<Option<Subscription>>,
    event: WeakChangeEvent<PropertyChange>,
    generation: Cell<u64>,
    active: Cell<bool>,
}
#[derive(Clone)]
pub struct PropertyDescriptor(Rc<Slot>);
impl PropertyDescriptor {
    pub fn new(
        name: impl Into<String>,
        configurable: bool,
        factory: Option<ComponentFactory>,
        event: &ChangeEvent<PropertyChange>,
    ) -> Self {
        Self::new_inner(
            Cow::Owned(name.into()),
            configurable,
            match factory {
                Some(f) => Factory::Custom(f),
                None => Factory::None,
            },
            event,
        )
    }
    pub(crate) fn new_constant(
        name: impl Into<Cow<'static, str>>,
        configurable: bool,
        factory: Option<ComponentFactory>,
        event: &ChangeEvent<PropertyChange>,
    ) -> Self {
        Self::new_inner(
            name.into(),
            configurable,
            match factory {
                Some(f) => Factory::Custom(f),
                None => Factory::Constant,
            },
            event,
        )
    }
    fn new_inner(
        name: Cow<'static, str>,
        configurable: bool,
        factory: Factory,
        event: &ChangeEvent<PropertyChange>,
    ) -> Self {
        Self(Rc::new(Slot {
            name,
            configurable,
            factory,
            value: RefCell::new(None),
            subscription: RefCell::new(None),
            event: event.downgrade(),
            generation: Cell::new(0),
            active: Cell::new(true),
        }))
    }
    pub fn name(&self) -> &str {
        &self.0.name
    }
    pub fn configurable(&self) -> bool {
        self.0.configurable
    }
    pub fn get(&self) -> Option<PropertyInput> {
        self.0.value.borrow().clone()
    }
    pub fn deactivate(&self) {
        self.0.active.set(false);
        let old = self.0.subscription.borrow_mut().take();
        drop(old);
    }
    pub fn set(&self, value: Option<PropertyInput>) -> Result<(), RuntimeError> {
        if !self.0.active.get() {
            return Err(RuntimeError::new("property has been removed"));
        }
        let value = match (value, &self.0.factory) {
            (Some(PropertyInput::Value(v)), Factory::Constant) => Some(PropertyInput::Component(constant_component(v)?)),
            (Some(PropertyInput::Value(v)), Factory::Custom(factory)) => Some(PropertyInput::Component(factory(v)?)),
            (value, _) => value,
        };
        let old_subscription = self.0.subscription.borrow_mut().take();
        drop(old_subscription);
        let generation = self.0.generation.get().wrapping_add(1);
        self.0.generation.set(generation);
        let changed = {
            let old = self.0.value.borrow();
            match (&*old, &value) {
                (None, None) => false,
                (Some(a), Some(b)) => !a.strict_equals(b),
                _ => true,
            }
        };
        if changed {
            let old = self.0.value.replace(value.clone());
            if let Some(event) = self.0.event.upgrade() {
                if event.has_listeners() {
                    event.raise(PropertyChange {
                        name: self.0.name.as_ref().to_owned(),
                        new_value: value,
                        old_value: old,
                        child_changed: false,
                    });
                }
            }
        }
        // A handler can replace/remove this slot. Never reinstall an obsolete child.
        if self.0.active.get() && self.0.generation.get() == generation {
            let component = match &*self.0.value.borrow() {
                Some(PropertyInput::Component(c)) => Some(c.clone()),
                _ => None,
            };
            if let Some(component) = component {
                let event = self.0.event.clone();
                let name = self.0.name.clone();
                let weak_slot = Rc::downgrade(&self.0);
                let weak_component = Rc::downgrade(&component);
                let subscription = component.definition_changed().add_listener(move |_| {
                    if let (Some(slot), Some(event), Some(component)) =
                        (weak_slot.upgrade(), event.upgrade(), weak_component.upgrade())
                    {
                        if !slot.active.get() {
                            return;
                        }
                        let matches = match &*slot.value.borrow() {
                            Some(PropertyInput::Component(c)) => c.identity() == component.identity(),
                            _ => false,
                        };
                        if !matches {
                            return;
                        }
                        if event.has_listeners() {
                            let value = Some(PropertyInput::Component(component));
                            event.raise(PropertyChange {
                                name: name.as_ref().to_owned(),
                                new_value: value.clone(),
                                old_value: value,
                                child_changed: true,
                            });
                        }
                    }
                });
                *self.0.subscription.borrow_mut() = Some(subscription);
            }
        }
        Ok(())
    }
}
pub fn create_property_descriptor(
    name: impl Into<String>,
    configurable: bool,
    factory: Option<ComponentFactory>,
    event: &ChangeEvent<PropertyChange>,
) -> PropertyDescriptor {
    PropertyDescriptor::new_constant(Cow::Owned(name.into()), configurable, factory, event)
}
