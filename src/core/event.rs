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

pub type ListenerId = u64;

type Listener<A> = Box<dyn FnMut(&A) + Send>;

pub struct Event<A> {
    listeners: Vec<(ListenerId, Listener<A>)>,
    next_id: ListenerId,
}

impl<A> Default for Event<A> {
    fn default() -> Self {
        Self { listeners: Vec::new(), next_id: 0 }
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
        self.listeners.len()
    }

    pub fn add_event_listener(&mut self, listener: impl FnMut(&A) + Send + 'static) -> ListenerId {
        let id = self.next_id;
        self.next_id += 1;
        self.listeners.push((id, Box::new(listener)));
        id
    }

    /// `true` if the listener was registered.
    pub fn remove_event_listener(&mut self, id: ListenerId) -> bool {
        let before = self.listeners.len();
        self.listeners.retain(|(i, _)| *i != id);
        self.listeners.len() != before
    }

    /// Calls every listener in the order added.
    pub fn raise_event(&mut self, args: &A) {
        for (_, l) in &mut self.listeners {
            l(args);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Event;

    #[test]
    fn events_are_send() {
        fn assert_send<T: Send>() {}
        assert_send::<Event<String>>();
        assert_send::<Event<crate::core::JulianDate>>();
    }
}
