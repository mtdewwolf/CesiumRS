//! Shared ownership, notifications and loading transitions for entity data sources.
use super::{ChangeEvent, DataSourceClock, EntityCollection};
use crate::core::{JulianDate, RuntimeError};
use std::{
    cell::{Cell, RefCell},
    rc::{Rc, Weak},
};

pub type DataSourceRef = Rc<dyn DataSource>;

pub struct DataSourceState {
    owner: Weak<dyn DataSource>,
    name: RefCell<Option<String>>,
    entities: Rc<EntityCollection>,
    loading: Cell<bool>,
    changed_event: ChangeEvent<DataSourceRef>,
    error_event: ChangeEvent<(DataSourceRef, RuntimeError)>,
    loading_event: ChangeEvent<(DataSourceRef, bool)>,
}

impl DataSourceState {
    /// Construct inside `Rc::new_cyclic` so events and entities refer to their actual source.
    pub fn new<T: DataSource + 'static>(owner: Weak<T>, name: Option<String>) -> Self {
        Self {
            entities: EntityCollection::with_owner(Some(owner.clone())),
            owner,
            name: RefCell::new(name),
            loading: Cell::new(false),
            changed_event: ChangeEvent::new(),
            error_event: ChangeEvent::new(),
            loading_event: ChangeEvent::new(),
        }
    }

    pub fn set_name(&self, name: Option<String>) {
        if *self.name.borrow() != name {
            self.name.replace(name);
            self.notify_changed();
        }
    }
    pub(crate) fn set_name_silent(&self, name: Option<String>) {
        self.name.replace(name);
    }
    pub fn notify_changed(&self) {
        if let Some(source) = self.owner.upgrade() {
            self.changed_event.raise(source);
        }
    }
    pub fn notify_error(&self, error: RuntimeError) {
        if let Some(source) = self.owner.upgrade() {
            self.error_event.raise((source, error));
        }
    }
}

pub trait DataSource {
    fn state(&self) -> &DataSourceState;
    fn name(&self) -> Option<String> {
        self.state().name.borrow().clone()
    }
    fn clock(&self) -> Option<Rc<DataSourceClock>> {
        None
    }
    fn entities(&self) -> &Rc<EntityCollection> {
        &self.state().entities
    }
    fn is_loading(&self) -> bool {
        self.state().loading.get()
    }
    fn changed_event(&self) -> &ChangeEvent<DataSourceRef> {
        &self.state().changed_event
    }
    fn error_event(&self) -> &ChangeEvent<(DataSourceRef, RuntimeError)> {
        &self.state().error_event
    }
    fn loading_event(&self) -> &ChangeEvent<(DataSourceRef, bool)> {
        &self.state().loading_event
    }
    fn show(&self) -> bool {
        self.entities().show()
    }
    fn set_show(&self, show: bool) -> Result<(), RuntimeError> {
        self.entities().set_show(show)
    }
    fn update(&self, _time: &JulianDate) -> Result<bool, RuntimeError> {
        Ok(true)
    }
    fn destroy(&self) {}

    fn set_loading(&self, loading: bool) -> Result<(), RuntimeError> {
        if self.is_loading() != loading {
            if loading {
                self.entities().suspend_events();
            } else {
                self.entities().resume_events()?;
            }
            self.state().loading.set(loading);
            if let Some(source) = self.state().owner.upgrade() {
                self.loading_event().raise((source, loading));
            }
        }
        Ok(())
    }
}
