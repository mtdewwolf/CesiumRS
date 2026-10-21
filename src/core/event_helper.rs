//! CesiumJS `EventHelper`: tracks listeners added to events so they can all
//! be removed at once.
//!
//! Events and the helper itself are shared as `Arc<Mutex<_>>`, so a helper
//! and the events it tracks can cross threads — unlike Cesium's JS version,
//! nothing here is tied to `Rc` or a single thread.

use std::sync::{Arc, Mutex};

use super::event::{Event, ListenerId};

type Removal = Box<dyn FnOnce() + Send>;

fn lock<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

#[derive(Default)]
pub struct EventHelper {
    removal_functions: Arc<Mutex<Vec<(u64, Removal)>>>,
    next: u64,
}

impl EventHelper {
    pub fn new() -> Self {
        Self::default()
    }

    /// Adds `listener` to `event`. The returned function removes just this
    /// listener.
    pub fn add<A: 'static>(
        &mut self,
        event: &Arc<Mutex<Event<A>>>,
        listener: impl FnMut(&A) + Send + 'static,
    ) -> impl FnOnce() {
        let (id, remover): (ListenerId, _) = {
            let mut event = lock(event);
            (event.add_event_listener(listener), event.remover())
        };
        let key = self.next;
        self.next += 1;
        // Through the remover, not the event's mutex: removal may run from
        // inside a listener of this event, while the raise holds the lock.
        let remove = move || remover.remove(id);
        lock(&self.removal_functions).push((key, Box::new(remove)));
        let list = Arc::downgrade(&self.removal_functions);
        move || {
            if let Some(list) = list.upgrade() {
                let found = {
                    let mut l = lock(&list);
                    l.iter().position(|(k, _)| *k == key).map(|i| l.remove(i).1)
                };
                if let Some(remove) = found {
                    remove();
                }
            }
        }
    }

    pub fn len(&self) -> usize {
        lock(&self.removal_functions).len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Removes every listener this helper added.
    pub fn remove_all(&mut self) {
        let all = std::mem::take(&mut *lock(&self.removal_functions));
        for (_, remove) in all {
            remove();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    type Slot = Arc<Mutex<Option<Box<dyn FnOnce() + Send>>>>;

    #[test]
    fn listener_can_remove_itself_through_the_helper() {
        let event = Arc::new(Mutex::new(Event::<u32>::new()));
        let mut helper = EventHelper::new();
        let slot: Slot = Arc::new(Mutex::new(None));
        let calls = Arc::new(Mutex::new(0));
        let (s, c) = (slot.clone(), calls.clone());
        let remove = helper.add(&event, move |_| {
            *c.lock().unwrap() += 1;
            if let Some(remove) = s.lock().unwrap().take() {
                remove();
            }
        });
        *slot.lock().unwrap() = Some(Box::new(remove));
        // Before the fix this deadlocked: removal locked the event's mutex,
        // which the raise below already holds.
        lock(&event).raise_event(&1);
        lock(&event).raise_event(&2);
        assert_eq!(*calls.lock().unwrap(), 1);
        assert_eq!(lock(&event).number_of_listeners(), 0);
        assert!(helper.is_empty());
    }

    #[test]
    fn remove_all_from_inside_a_listener() {
        let event = Arc::new(Mutex::new(Event::<u32>::new()));
        let helper = Arc::new(Mutex::new(EventHelper::new()));
        let h = helper.clone();
        let _remove = lock(&helper).add(&event, move |_| lock(&h).remove_all());
        let _remove_other = lock(&helper).add(&event, |_| {});
        lock(&event).raise_event(&0);
        assert_eq!(lock(&event).number_of_listeners(), 0);
    }
}
