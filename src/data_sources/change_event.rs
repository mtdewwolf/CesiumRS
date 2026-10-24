//! Payload-bearing local events built on the validated definition-event dispatcher.
use super::{DefinitionChanged, PropertyListener, Subscription, WeakDefinitionChanged};
use std::{
    cell::RefCell,
    rc::{Rc, Weak},
};
pub type ChangeListener<T> = Rc<dyn Fn(&T)>;
type CachedBridge<T> = (Weak<dyn Fn(&T)>, Weak<dyn Fn(&DefinitionChanged)>);
struct Payload<T> {
    frames: RefCell<Vec<Rc<T>>>,
    bridges: RefCell<Vec<CachedBridge<T>>>,
}
pub struct ChangeEvent<T: 'static> {
    base: DefinitionChanged,
    payload: Rc<Payload<T>>,
}
pub struct WeakChangeEvent<T: 'static> {
    base: WeakDefinitionChanged,
    payload: Weak<Payload<T>>,
}
impl<T: 'static> Clone for ChangeEvent<T> {
    fn clone(&self) -> Self {
        Self { base: self.base.clone(), payload: self.payload.clone() }
    }
}
impl<T: 'static> Clone for WeakChangeEvent<T> {
    fn clone(&self) -> Self {
        Self { base: self.base.clone(), payload: self.payload.clone() }
    }
}
impl<T: 'static> Default for ChangeEvent<T> {
    fn default() -> Self {
        Self {
            base: DefinitionChanged::new(),
            payload: Rc::new(Payload { frames: RefCell::new(vec![]), bridges: RefCell::new(vec![]) }),
        }
    }
}
impl<T: 'static> WeakChangeEvent<T> {
    pub fn upgrade(&self) -> Option<ChangeEvent<T>> {
        Some(ChangeEvent { base: self.base.upgrade()?, payload: self.payload.upgrade()? })
    }
}
impl<T: 'static> ChangeEvent<T> {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn downgrade(&self) -> WeakChangeEvent<T> {
        WeakChangeEvent { base: self.base.downgrade(), payload: Rc::downgrade(&self.payload) }
    }
    pub fn number_of_listeners(&self) -> usize {
        self.base.number_of_listeners()
    }
    pub fn subscribe(&self, listener: ChangeListener<T>) -> Subscription {
        let mut bridges = self.payload.bridges.borrow_mut();
        bridges.retain(|(_, bridge)| bridge.strong_count() > 0);
        let bridge = bridges.iter().find_map(|(old, bridge)| {
            old.upgrade().filter(|old| Rc::ptr_eq(old, &listener)).and_then(|_| bridge.upgrade())
        });
        let bridge = bridge.unwrap_or_else(|| {
            let weak = Rc::downgrade(&self.payload);
            let callback = listener.clone();
            let bridge: PropertyListener = Rc::new(move |_| {
                if let Some(payload) = weak.upgrade() {
                    let frame = payload.frames.borrow().last().cloned();
                    if let Some(frame) = frame {
                        callback(&frame);
                    }
                }
            });
            bridges.push((Rc::downgrade(&listener), Rc::downgrade(&bridge)));
            bridge
        });
        drop(bridges);
        self.base.subscribe(bridge)
    }
    pub fn add_listener(&self, listener: impl Fn(&T) + 'static) -> Subscription {
        self.subscribe(Rc::new(listener))
    }
    pub fn raise(&self, value: T) {
        self.payload.frames.borrow_mut().push(Rc::new(value));
        self.base.raise();
        let frame = self.payload.frames.borrow_mut().pop();
        drop(frame);
    }
}
