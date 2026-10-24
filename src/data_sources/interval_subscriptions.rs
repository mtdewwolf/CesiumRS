//! Shared weak subscription wiring for property and position composites.
use super::{DefinitionChanged, PropertyIntervals, PropertyListener, Subscription};
use std::{cell::RefCell, rc::Rc};
pub(crate) struct IntervalSubscriptions {
    _children: Rc<RefCell<Vec<Subscription>>>,
    _intervals: Subscription,
}
impl IntervalSubscriptions {
    pub(crate) fn new<D: Clone + 'static>(
        intervals: &PropertyIntervals<D>,
        changed: &DefinitionChanged,
        event_of: fn(&D) -> &DefinitionChanged,
    ) -> Self {
        let children = Rc::new(RefCell::new(Vec::<Subscription>::new()));
        let weak = changed.downgrade();
        let core = intervals.weak_core();
        let weak_children = Rc::downgrade(&children);
        let subscription = intervals.changed_event().add_listener(move |_| {
            let (Some(core), Some(children), Some(event)) = (core.upgrade(), weak_children.upgrade(), weak.upgrade())
            else {
                return;
            };
            let old = std::mem::take(&mut *children.borrow_mut());
            drop(old);
            let properties = core.borrow().intervals().iter().filter_map(|i| i.data.clone()).collect::<Vec<_>>();
            let forward = event.downgrade();
            let bridge: PropertyListener = Rc::new(move |_| {
                if let Some(event) = forward.upgrade() {
                    event.raise();
                }
            });
            let mut subscribed = Vec::<DefinitionChanged>::new();
            let mut subscriptions = vec![];
            for p in properties {
                let e = event_of(&p);
                if !subscribed.iter().any(|v| v.ptr_eq(e)) {
                    subscriptions.push(e.subscribe(bridge.clone()));
                    subscribed.push(e.clone());
                }
            }
            *children.borrow_mut() = subscriptions;
            event.raise();
        });
        Self { _children: children, _intervals: subscription }
    }
}
