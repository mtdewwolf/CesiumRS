//! CesiumJS `EventHelper`: tracks listeners added to events so they can all
//! be removed at once.
//!
//! Events and the helper itself are shared as `Arc<Mutex<_>>`, so a helper
//! and the events it tracks can cross threads — unlike Cesium's JS version,
//! nothing here is tied to `Rc` or a single thread.

use std::sync::{Arc, Mutex, Weak};

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
        let id: ListenerId = lock(event).add_event_listener(listener);
        let weak: Weak<Mutex<Event<A>>> = Arc::downgrade(event);
        let key = self.next;
        self.next += 1;
        let remove = move || {
            if let Some(event) = weak.upgrade() {
                lock(&event).remove_event_listener(id);
            }
        };
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
