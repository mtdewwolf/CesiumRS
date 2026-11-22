//! An application-owned data source with an optional simulation clock.
use super::{DataSource, DataSourceClock, DataSourceState};
use std::{cell::RefCell, rc::Rc};

pub struct CustomDataSource {
    state: DataSourceState,
    clock: RefCell<Option<Rc<DataSourceClock>>>,
}
impl CustomDataSource {
    pub fn new(name: Option<String>) -> Rc<Self> {
        Rc::new_cyclic(|owner| Self { state: DataSourceState::new(owner.clone(), name), clock: RefCell::default() })
    }
    pub fn set_name(&self, name: Option<String>) {
        self.state.set_name(name);
    }
    pub fn set_clock(&self, clock: Option<Rc<DataSourceClock>>) {
        if self.clock.borrow().as_ref().map(Rc::as_ptr) != clock.as_ref().map(Rc::as_ptr) {
            self.clock.replace(clock);
            self.state.notify_changed();
        }
    }
}
impl DataSource for CustomDataSource {
    fn state(&self) -> &DataSourceState {
        &self.state
    }
    fn clock(&self) -> Option<Rc<DataSourceClock>> {
        self.clock.borrow().clone()
    }
}
