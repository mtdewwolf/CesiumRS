//! Owned property slots replace JavaScript accessor descriptors.
use super::{
    ChangeEvent, ComponentRef, ConstantProperty, DynamicValue, PropertyComponent, PropertyInput, Subscription,
    WeakChangeEvent,
};
use crate::core::RuntimeError;
use std::{
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
struct Slot {
    name: String,
    configurable: bool,
    factory: Option<ComponentFactory>,
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
        Self(Rc::new(Slot {
            name: name.into(),
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
            (Some(PropertyInput::Value(value)), Some(factory)) => Some(PropertyInput::Component(factory(value)?)),
            (value, _) => value,
        };
        let old_subscription = self.0.subscription.borrow_mut().take();
        drop(old_subscription);
        let generation = self.0.generation.get().wrapping_add(1);
        self.0.generation.set(generation);
        let old = self.get();
        let changed = match (&old, &value) {
            (None, None) => false,
            (Some(a), Some(b)) => !a.strict_equals(b),
            _ => true,
        };
        if changed {
            *self.0.value.borrow_mut() = value.clone();
            if let Some(event) = self.0.event.upgrade() {
                event.raise(PropertyChange {
                    name: self.0.name.clone(),
                    new_value: value.clone(),
                    old_value: old,
                    child_changed: false,
                });
            }
        }
        // A handler can replace/remove this slot. Never reinstall an obsolete child.
        if self.0.active.get() && self.0.generation.get() == generation {
            if let Some(PropertyInput::Component(component)) = self.get() {
                let event = self.0.event.clone();
                let name = self.0.name.clone();
                let weak_slot = Rc::downgrade(&self.0);
                let weak_component = Rc::downgrade(&component);
                let subscription = component.definition_changed().add_listener(move |_| {
                    if let (Some(slot), Some(event), Some(component)) =
                        (weak_slot.upgrade(), event.upgrade(), weak_component.upgrade())
                    {
                        let current = slot.value.borrow().clone();
                        if !slot.active.get()
                            || !current
                                .and_then(|v| v.component())
                                .is_some_and(|v| v.identity() == component.identity())
                        {
                            return;
                        }
                        let value = Some(PropertyInput::Component(component));
                        event.raise(PropertyChange {
                            name: name.clone(),
                            new_value: value.clone(),
                            old_value: value,
                            child_changed: true,
                        });
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
    PropertyDescriptor::new(name, configurable, Some(factory.unwrap_or_else(|| Rc::new(constant_component))), event)
}
