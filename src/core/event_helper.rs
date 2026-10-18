//! CesiumJS `EventHelper`: tracks listeners added to events so they can all
//! be removed at once.

use std::cell::RefCell;
use std::rc::{Rc, Weak};

use super::event::{Event, ListenerId};

type Removal = Box<dyn FnOnce()>;

#[derive(Default)]
pub struct EventHelper {
    removal_functions: Rc<RefCell<Vec<(u64, Removal)>>>,
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
        event: &Rc<RefCell<Event<A>>>,
        listener: impl FnMut(&A) + 'static,
    ) -> impl FnOnce() {
        let id: ListenerId = event.borrow_mut().add_event_listener(listener);
        let weak: Weak<RefCell<Event<A>>> = Rc::downgrade(event);
        let key = self.next;
        self.next += 1;
        let remove = move || {
            if let Some(event) = weak.upgrade() {
                event.borrow_mut().remove_event_listener(id);
            }
        };
        self.removal_functions.borrow_mut().push((key, Box::new(remove)));
        let list = Rc::downgrade(&self.removal_functions);
        move || {
            if let Some(list) = list.upgrade() {
                let found = {
                    let mut l = list.borrow_mut();
                    l.iter().position(|(k, _)| *k == key).map(|i| l.remove(i).1)
                };
                if let Some(remove) = found {
                    remove();
                }
            }
        }
    }

    pub fn len(&self) -> usize {
        self.removal_functions.borrow().len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Removes every listener this helper added.
    pub fn remove_all(&mut self) {
        let all = std::mem::take(&mut *self.removal_functions.borrow_mut());
        for (_, remove) in all {
            remove();
        }
    }
}
