//! Local definition-change events. Listeners may capture Rc-based properties;
//! subscriptions remove themselves on drop and forwarding uses weak owners.
use std::{
    cell::{Cell, RefCell},
    collections::HashSet,
    rc::{Rc, Weak},
};
pub type PropertyListener = Rc<dyn Fn(&DefinitionChanged)>;
#[derive(Clone)]
struct Slot {
    id: u64,
    listener: PropertyListener,
}
#[derive(Default)]
struct Inner {
    slots: RefCell<Vec<Option<Slot>>>,
    pending: RefCell<Vec<Slot>>,
    removed: RefCell<HashSet<u64>>,
    next: Cell<u64>,
    count: Cell<usize>,
    invoking: Cell<bool>,
    depth: Cell<usize>,
}
#[derive(Clone, Default)]
/// A single-threaded definition event; callbacks receive the emitting event.
pub struct DefinitionChanged(Rc<Inner>);
/// Removing or dropping this handle unsubscribes the listener. Duplicate handles
/// address the same registration, so dropping either removes that registration.
#[must_use = "keep the subscription alive to receive definition changes"]
pub struct Subscription {
    event: Weak<Inner>,
    id: Option<u64>,
}
#[derive(Clone)]
pub struct WeakDefinitionChanged(Weak<Inner>);
impl WeakDefinitionChanged {
    pub fn upgrade(&self) -> Option<DefinitionChanged> {
        self.0.upgrade().map(DefinitionChanged)
    }
}
impl DefinitionChanged {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn downgrade(&self) -> WeakDefinitionChanged {
        WeakDefinitionChanged(Rc::downgrade(&self.0))
    }
    pub fn ptr_eq(&self, other: &Self) -> bool {
        Rc::ptr_eq(&self.0, &other.0)
    }
    pub fn number_of_listeners(&self) -> usize {
        self.0.count.get()
    }
    pub fn subscribe(&self, listener: PropertyListener) -> Subscription {
        let inner = &self.0;
        let existing = if inner.invoking.get() {
            inner.pending.borrow().iter().find(|s| Rc::ptr_eq(&s.listener, &listener)).map(|s| s.id)
        } else {
            inner.slots.borrow().iter().flatten().find(|s| Rc::ptr_eq(&s.listener, &listener)).map(|s| s.id)
        };
        if let Some(id) = existing {
            return Subscription { event: Rc::downgrade(inner), id: Some(id) };
        }
        let id = inner
            .slots
            .borrow()
            .iter()
            .flatten()
            .find(|s| Rc::ptr_eq(&s.listener, &listener))
            .map(|s| s.id)
            .unwrap_or_else(|| {
                let id = inner.next.get();
                inner.next.set(id + 1);
                id
            });
        let slot = Slot { id, listener };
        inner.count.set(inner.count.get() + 1);
        if inner.invoking.get() {
            inner.pending.borrow_mut().push(slot);
        } else {
            if inner.depth.get() == 0 {
                inner.slots.borrow_mut().retain(Option::is_some);
            }
            inner.slots.borrow_mut().push(Some(slot));
        }
        Subscription { event: Rc::downgrade(inner), id: Some(id) }
    }
    pub fn add_listener(&self, listener: impl Fn(&DefinitionChanged) + 'static) -> Subscription {
        self.subscribe(Rc::new(listener))
    }
    /// Additions/removals during dispatch settle after the current raise,
    /// matching the pinned Event's deferred changes. No borrow spans a callback.
    pub fn raise(&self) {
        let inner = &self.0;
        inner.depth.set(inner.depth.get() + 1);
        inner.invoking.set(true);
        let mut index = 0;
        loop {
            let slot = inner.slots.borrow().get(index).cloned();
            let Some(slot) = slot else { break };
            index += 1;
            if let Some(slot) = slot {
                (slot.listener)(self);
            }
        }
        inner.invoking.set(false);
        let pending = std::mem::take(&mut *inner.pending.borrow_mut());
        for slot in pending {
            if !inner.slots.borrow().iter().flatten().any(|s| Rc::ptr_eq(&s.listener, &slot.listener)) {
                inner.slots.borrow_mut().push(Some(slot));
            }
        }
        let removed = std::mem::take(&mut *inner.removed.borrow_mut());
        let old = {
            let mut slots = inner.slots.borrow_mut();
            slots
                .iter_mut()
                .filter(|s| s.as_ref().is_some_and(|s| removed.contains(&s.id)))
                .filter_map(Option::take)
                .collect::<Vec<_>>()
        };
        drop(old);
        inner.depth.set(inner.depth.get() - 1);
        if inner.depth.get() == 0 {
            inner.slots.borrow_mut().retain(Option::is_some);
        }
    }
}
impl Subscription {
    pub fn remove(&mut self) -> bool {
        let (Some(inner), Some(id)) = (self.event.upgrade(), self.id.take()) else { return false };
        let exists = inner.slots.borrow().iter().flatten().any(|s| s.id == id)
            || inner.pending.borrow().iter().any(|s| s.id == id);
        if !exists || inner.removed.borrow().contains(&id) {
            return false;
        }
        inner.count.set(inner.count.get().saturating_sub(1));
        if inner.invoking.get() {
            inner.removed.borrow_mut().insert(id);
        } else {
            let removed = {
                let mut slots = inner.slots.borrow_mut();
                slots
                    .iter_mut()
                    .filter(|s| s.as_ref().is_some_and(|s| s.id == id))
                    .filter_map(Option::take)
                    .collect::<Vec<_>>()
            };
            drop(removed);
            let pending = {
                let mut p = inner.pending.borrow_mut();
                let removed = p.iter().filter(|s| s.id == id).cloned().collect::<Vec<_>>();
                p.retain(|s| s.id != id);
                removed
            };
            drop(pending);
        }
        true
    }
}
impl Drop for Subscription {
    fn drop(&mut self) {
        self.remove();
    }
}
