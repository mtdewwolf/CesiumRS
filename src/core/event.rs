//! CesiumJS `Event`: a list of listeners raised together.
//!
//! JavaScript identifies a listener by its function and `this`; here
//! [`Event::add_event_listener`] returns an id to remove it with.
//!
//! Listeners must be `Send`, so an `Event` itself is `Send` and can be
//! shared between threads behind an `Arc<Mutex<_>>` — that is how
//! `TerrainProvider::error_event` and [`super::EventHelper`] share events.
//! An `Event` is deliberately not `Sync`: raising one needs `&mut` (or the
//! mutex), because listeners are `FnMut`.
//!
//! A listener cannot reach its own event while it is being raised (the
//! event is borrowed, or its mutex held), so removal from inside a listener
//! goes through an [`EventRemover`]. As in Cesium, a listener removed during
//! a raise is not called for the rest of that raise.

use std::sync::{Arc, Mutex};

pub type ListenerId = u64;

type Listener<A> = Box<dyn FnMut(&A) + Send>;

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

/// Removes listeners from an [`Event`] without borrowing it, so it works
/// from inside a listener of that event.
#[derive(Clone, Debug, Default)]
pub struct EventRemover(Arc<Mutex<Vec<ListenerId>>>);

impl EventRemover {
    /// Takes effect before the event's next listener call or query.
    pub fn remove(&self, id: ListenerId) {
        lock(&self.0).push(id);
    }

    fn is_removed(&self, id: ListenerId) -> bool {
        lock(&self.0).contains(&id)
    }
}

pub struct Event<A> {
    listeners: Vec<(ListenerId, Listener<A>)>,
    next_id: ListenerId,
    pending_removals: EventRemover,
}

impl<A> Default for Event<A> {
    fn default() -> Self {
        Self { listeners: Vec::new(), next_id: 0, pending_removals: EventRemover::default() }
    }
}

impl<A> std::fmt::Debug for Event<A> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Event({} listeners)", self.listeners.len())
    }
}

impl<A> Event<A> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn number_of_listeners(&self) -> usize {
        let pending = lock(&self.pending_removals.0);
        self.listeners.iter().filter(|(id, _)| !pending.contains(id)).count()
    }

    /// A handle that removes listeners from this event, even from inside
    /// one of its listeners.
    pub fn remover(&self) -> EventRemover {
        self.pending_removals.clone()
    }

    fn apply_pending_removals(&mut self) {
        let pending = std::mem::take(&mut *lock(&self.pending_removals.0));
        if !pending.is_empty() {
            self.listeners.retain(|(id, _)| !pending.contains(id));
        }
    }

    pub fn add_event_listener(&mut self, listener: impl FnMut(&A) + Send + 'static) -> ListenerId {
        self.apply_pending_removals();
        let id = self.next_id;
        self.next_id += 1;
        self.listeners.push((id, Box::new(listener)));
        id
    }

    /// `true` if the listener was registered.
    pub fn remove_event_listener(&mut self, id: ListenerId) -> bool {
        self.apply_pending_removals();
        let before = self.listeners.len();
        self.listeners.retain(|(i, _)| *i != id);
        self.listeners.len() != before
    }

    /// Calls every listener in the order added, skipping any removed
    /// through an [`EventRemover`] earlier in the same raise.
    pub fn raise_event(&mut self, args: &A) {
        self.apply_pending_removals();
        for (id, l) in &mut self.listeners {
            if !self.pending_removals.is_removed(*id) {
                l(args);
            }
        }
        self.apply_pending_removals();
    }
}

#[cfg(test)]
mod tests {
    use super::Event;

    #[test]
    fn listener_can_remove_itself_while_raised() {
        use std::sync::{Arc, Mutex};
        let event = Arc::new(Mutex::new(Event::<u32>::new()));
        let calls = Arc::new(Mutex::new(Vec::new()));
        let remover = event.lock().unwrap().remover();
        let id = Arc::new(Mutex::new(None));
        let (c, i) = (calls.clone(), id.clone());
        let first = event.lock().unwrap().add_event_listener(move |v| {
            c.lock().unwrap().push(("once", *v));
            remover.remove(i.lock().unwrap().unwrap());
        });
        *id.lock().unwrap() = Some(first);
        let c = calls.clone();
        event.lock().unwrap().add_event_listener(move |v| c.lock().unwrap().push(("always", *v)));
        event.lock().unwrap().raise_event(&1);
        event.lock().unwrap().raise_event(&2);
        assert_eq!(*calls.lock().unwrap(), [("once", 1), ("always", 1), ("always", 2)]);
        assert_eq!(event.lock().unwrap().number_of_listeners(), 1);
    }

    #[test]
    fn listener_removed_mid_raise_is_skipped() {
        let mut event = Event::<u32>::new();
        let remover = event.remover();
        let calls = std::sync::Arc::new(std::sync::Mutex::new(0));
        event.add_event_listener(move |_| remover.remove(1));
        let c = calls.clone();
        event.add_event_listener(move |_| *c.lock().unwrap() += 1);
        event.raise_event(&0);
        assert_eq!(*calls.lock().unwrap(), 0);
        assert_eq!(event.number_of_listeners(), 1);
    }

    #[test]
    fn events_are_send() {
        fn assert_send<T: Send>() {}
        assert_send::<Event<String>>();
        assert_send::<Event<crate::core::JulianDate>>();
    }
}
