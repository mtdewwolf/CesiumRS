//! Drives host-supplied visualizers in collection order, followed by the default source.
use super::{
    data_source_collection::Member, BoundingSphereState, CustomDataSource, DataSource, DataSourceCollection,
    DataSourceRef, EntityRef, Subscription, Visualizer,
};
use crate::core::{ApproximateTerrainHeights, BoundingSphere, JulianDate, RuntimeError};
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    rc::Rc,
};

pub type VisualizerFactory = Rc<dyn Fn(&DataSourceRef) -> Vec<Box<dyn Visualizer>>>;

struct Visualizers(RefCell<Vec<Box<dyn Visualizer>>>);
impl Drop for Visualizers {
    fn drop(&mut self) {
        for item in self.0.get_mut() {
            item.destroy();
        }
    }
}

pub struct DataSourceDisplay {
    collection: Rc<DataSourceCollection>,
    default: Rc<CustomDataSource>,
    factory: VisualizerFactory,
    request_render: Rc<dyn Fn()>,
    // Each insertion has its own identity, including repeated adds of the same source.
    groups: RefCell<HashMap<*const Member, Rc<Visualizers>>>,
    default_visualizers: RefCell<Option<Rc<Visualizers>>>,
    subscriptions: RefCell<Vec<Subscription>>,
    ready: Cell<bool>,
    destroyed: Cell<bool>,
}
impl DataSourceDisplay {
    pub fn new(collection: Rc<DataSourceCollection>, factory: VisualizerFactory) -> Rc<Self> {
        Self::with_request_render(collection, factory, Rc::new(|| {}))
    }
    pub fn with_request_render(
        collection: Rc<DataSourceCollection>,
        factory: VisualizerFactory,
        request_render: Rc<dyn Fn()>,
    ) -> Rc<Self> {
        ApproximateTerrainHeights::bundled();
        let display = Rc::new(Self {
            collection: collection.clone(),
            default: CustomDataSource::new(None),
            factory,
            request_render,
            groups: RefCell::default(),
            default_visualizers: RefCell::default(),
            subscriptions: RefCell::default(),
            ready: Cell::new(false),
            destroyed: Cell::new(false),
        });
        let weak = Rc::downgrade(&display);
        let add = collection.data_source_added().add_listener(move |e| {
            if let Some(display) = weak.upgrade() {
                display.attach(&e.member);
            }
        });
        let weak = Rc::downgrade(&display);
        let remove = collection.data_source_removed().add_listener(move |e| {
            if let Some(display) = weak.upgrade() {
                display.detach(&e.member);
            }
        });
        display.subscriptions.replace(vec![add, remove]);
        for member in collection.members() {
            display.attach(&member);
        }
        let default = display.group(&(display.default.clone() as DataSourceRef));
        display.default_visualizers.replace(Some(default));
        display
    }
    fn group(&self, source: &DataSourceRef) -> Rc<Visualizers> {
        Rc::new(Visualizers(RefCell::new((self.factory)(source))))
    }
    fn attach(&self, member: &Rc<Member>) {
        if member.removing.get() || self.is_destroyed() {
            return;
        }
        let group = self.group(&member.source);
        // A factory may synchronously remove the source or destroy the display.
        if !member.removing.get() && !self.is_destroyed() {
            self.groups.borrow_mut().insert(Rc::as_ptr(member), group);
        }
    }
    fn detach(&self, member: &Rc<Member>) {
        let retired = self.groups.borrow_mut().remove(&Rc::as_ptr(member));
        drop(retired);
    }
    fn visualizers(&self, member: &Rc<Member>) -> Option<Rc<Visualizers>> {
        self.groups.borrow().get(&Rc::as_ptr(member)).cloned()
    }
    pub fn data_sources(&self) -> &Rc<DataSourceCollection> {
        &self.collection
    }
    pub fn default_data_source(&self) -> &Rc<CustomDataSource> {
        &self.default
    }
    pub fn ready(&self) -> bool {
        self.ready.get()
    }
    pub fn update(&self, time: &JulianDate) -> Result<bool, RuntimeError> {
        if self.is_destroyed() {
            return Err(RuntimeError::new("This display was destroyed."));
        }
        let mut ready = true;
        let update = |group: &Visualizers| -> Result<bool, RuntimeError> {
            let mut result = true;
            let mut items =
                group.0.try_borrow_mut().map_err(|_| RuntimeError::new("Visualizer update is already in progress."))?;
            for v in items.iter_mut() {
                result &= v.update(time)?;
            }
            Ok(result)
        };
        for member in self.collection.members() {
            if member.removing.get() || self.is_destroyed() {
                continue;
            }
            ready &= member.source.update(time)?;
            if let Some(group) = self.visualizers(&member) {
                ready &= update(&group)?;
            }
        }
        let default = self.default_visualizers.borrow().clone();
        if let Some(group) = default {
            ready &= update(&group)?;
        }
        if ready && !self.ready.replace(true) {
            (self.request_render)();
        }
        Ok(ready)
    }
    pub fn get_bounding_sphere(
        &self,
        entity: &EntityRef,
        allow_partial: bool,
        result: &mut BoundingSphere,
    ) -> BoundingSphereState {
        if !self.ready() {
            return BoundingSphereState::Pending;
        }
        let group = if self.default.entities().contains(entity) {
            self.default_visualizers.borrow().clone()
        } else {
            self.collection
                .members()
                .iter()
                .find(|m| m.source.entities().contains(entity))
                .and_then(|m| self.visualizers(m))
        };
        let Some(group) = group else {
            return BoundingSphereState::Failed;
        };
        let Ok(items) = group.0.try_borrow() else {
            return BoundingSphereState::Pending;
        };
        let mut spheres = Vec::new();
        for v in items.iter() {
            let mut sphere = BoundingSphere::default();
            match v.get_bounding_sphere(entity, &mut sphere) {
                BoundingSphereState::Done => spheres.push(sphere),
                BoundingSphereState::Pending if !allow_partial => return BoundingSphereState::Pending,
                _ => {}
            }
        }
        if spheres.is_empty() {
            return BoundingSphereState::Failed;
        }
        *result = BoundingSphere::from_bounding_spheres(&spheres);
        BoundingSphereState::Done
    }
    pub fn is_destroyed(&self) -> bool {
        self.destroyed.get()
    }
    pub fn destroy(&self) {
        if !self.destroyed.replace(true) {
            self.subscriptions.borrow_mut().clear();
            for member in self.collection.members() {
                self.detach(&member);
            }
            drop(self.groups.take());
            self.default_visualizers.take();
        }
    }
}
