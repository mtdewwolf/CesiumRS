//! Observable access to Core intervals. Notify local subscribers after releasing
//! Core's mutable borrow so observers can evaluate the property in their handler.
use super::DefinitionChanged;
use crate::core::{JulianDate, RuntimeError, TimeInterval, TimeIntervalCollection};
use std::{
    cell::{Ref, RefCell},
    rc::{Rc, Weak},
    sync::{
        atomic::{AtomicU64, Ordering},
        Arc,
    },
};
type DataComparer<D> = Rc<dyn Fn(&Option<D>, &Option<D>) -> bool>;
pub struct PropertyIntervals<D: Clone> {
    core: Rc<RefCell<TimeIntervalCollection<D>>>,
    same: DataComparer<D>,
    revision: Arc<AtomicU64>,
    changed: DefinitionChanged,
}
impl<D: Clone> Clone for PropertyIntervals<D> {
    fn clone(&self) -> Self {
        Self {
            core: self.core.clone(),
            same: self.same.clone(),
            revision: self.revision.clone(),
            changed: self.changed.clone(),
        }
    }
}
impl<D: Clone + 'static> PropertyIntervals<D> {
    pub fn new(same: impl Fn(&Option<D>, &Option<D>) -> bool + 'static) -> Self {
        let revision = Arc::new(AtomicU64::new(0));
        let r = revision.clone();
        let mut core = TimeIntervalCollection::new();
        core.changed_event.add_event_listener(move |_| {
            r.fetch_add(1, Ordering::Relaxed);
        });
        Self { core: Rc::new(RefCell::new(core)), same: Rc::new(same), revision, changed: DefinitionChanged::new() }
    }
    /// Read-only Core queries. Drop this guard before mutating the collection.
    pub fn read(&self) -> Ref<'_, TimeIntervalCollection<D>> {
        self.core.borrow()
    }
    pub(crate) fn weak_core(&self) -> Weak<RefCell<TimeIntervalCollection<D>>> {
        Rc::downgrade(&self.core)
    }
    pub fn changed_event(&self) -> &DefinitionChanged {
        &self.changed
    }
    pub fn len(&self) -> usize {
        self.core.borrow().len()
    }
    pub fn is_empty(&self) -> bool {
        self.core.borrow().is_empty()
    }
    pub fn get(&self, index: usize) -> Option<TimeInterval<D>> {
        self.core.borrow().get(index).cloned()
    }
    pub fn find_data(&self, time: &JulianDate) -> Option<D> {
        self.core.borrow().find_data_for_interval_containing_date(time).cloned()
    }
    fn change<R>(&self, f: impl FnOnce(&mut TimeIntervalCollection<D>) -> R) -> R {
        let before = self.revision.load(Ordering::Relaxed);
        let result = f(&mut self.core.borrow_mut());
        if self.revision.load(Ordering::Relaxed) != before {
            self.changed.raise();
        }
        result
    }
    pub fn add_interval(&self, interval: TimeInterval<D>) {
        self.change(|c| c.add_interval_with(interval, |a, b| (self.same)(a, b)));
    }
    pub fn remove_interval(&self, interval: &TimeInterval<D>) -> bool {
        self.change(|c| c.remove_interval_with(interval, |a, b| (self.same)(a, b)))
    }
    pub fn remove_all(&self) {
        self.change(TimeIntervalCollection::remove_all);
    }
    pub fn equals_by(
        &self,
        other: &Self,
        same: impl Fn(&Option<D>, &Option<D>) -> Result<bool, RuntimeError>,
    ) -> Result<bool, RuntimeError> {
        if Rc::ptr_eq(&self.core, &other.core) {
            return Ok(true);
        }
        let a = self.core.borrow().intervals().to_vec();
        let b = other.core.borrow().intervals().to_vec();
        if a.len() != b.len() {
            return Ok(false);
        }
        for (a, b) in a.iter().zip(&b) {
            if !(a.is_empty() && b.is_empty())
                && (a.start != b.start
                    || a.stop != b.stop
                    || a.is_start_included != b.is_start_included
                    || a.is_stop_included != b.is_stop_included
                    || !same(&a.data, &b.data)?)
            {
                return Ok(false);
            }
        }
        Ok(true)
    }
}
