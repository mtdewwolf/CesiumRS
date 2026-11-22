//! CesiumJS `GeometryUpdaterSet`: instantiates and coordinates all registered
//! [`GeometryUpdater`](super::GeometryUpdater)s for a single [`Entity`](super::Entity).
use super::{
    geometry_updater::{GeometryChangedEvent, GeometryScene, GeometryUpdaterRef},
    BoxGeometryUpdater, ChangeEvent, CorridorGeometryUpdater, CylinderGeometryUpdater, EllipseGeometryUpdater,
    EllipsoidGeometryUpdater, EntityChange, EntityRef, PlaneGeometryUpdater, PolygonGeometryUpdater,
    PolylineVolumeGeometryUpdater, RectangleGeometryUpdater, Subscription, WallGeometryUpdater,
};
use std::{cell::RefCell, rc::Rc};

/// Factory creating a concrete [`GeometryUpdater`](super::GeometryUpdater) for
/// an `(entity, scene)` pair.
pub type GeometryUpdaterFactory = Rc<dyn Fn(EntityRef, Rc<dyn GeometryScene>) -> GeometryUpdaterRef>;

/// Named registration entry in [`GeometryUpdaterSet`]'s registry.
#[derive(Clone)]
pub struct GeometryUpdaterRegistration {
    pub name: String,
    pub factory: GeometryUpdaterFactory,
}

impl GeometryUpdaterRegistration {
    pub fn new(name: impl Into<String>, factory: GeometryUpdaterFactory) -> Self {
        Self { name: name.into(), factory }
    }
}

fn default_registrations() -> Vec<GeometryUpdaterRegistration> {
    vec![
        GeometryUpdaterRegistration::new(
            "BoxGeometryUpdater",
            Rc::new(|e, s| Rc::new(RefCell::new(BoxGeometryUpdater::new(e, s)))),
        ),
        GeometryUpdaterRegistration::new(
            "CylinderGeometryUpdater",
            Rc::new(|e, s| Rc::new(RefCell::new(CylinderGeometryUpdater::new(e, s)))),
        ),
        GeometryUpdaterRegistration::new(
            "CorridorGeometryUpdater",
            Rc::new(|e, s| Rc::new(RefCell::new(CorridorGeometryUpdater::new(e, s)))),
        ),
        GeometryUpdaterRegistration::new(
            "EllipseGeometryUpdater",
            Rc::new(|e, s| Rc::new(RefCell::new(EllipseGeometryUpdater::new(e, s)))),
        ),
        GeometryUpdaterRegistration::new(
            "EllipsoidGeometryUpdater",
            Rc::new(|e, s| Rc::new(RefCell::new(EllipsoidGeometryUpdater::new(e, s)))),
        ),
        GeometryUpdaterRegistration::new(
            "PlaneGeometryUpdater",
            Rc::new(|e, s| Rc::new(RefCell::new(PlaneGeometryUpdater::new(e, s)))),
        ),
        GeometryUpdaterRegistration::new(
            "PolygonGeometryUpdater",
            Rc::new(|e, s| Rc::new(RefCell::new(PolygonGeometryUpdater::new(e, s)))),
        ),
        GeometryUpdaterRegistration::new(
            "PolylineVolumeGeometryUpdater",
            Rc::new(|e, s| Rc::new(RefCell::new(PolylineVolumeGeometryUpdater::new(e, s)))),
        ),
        GeometryUpdaterRegistration::new(
            "RectangleGeometryUpdater",
            Rc::new(|e, s| Rc::new(RefCell::new(RectangleGeometryUpdater::new(e, s)))),
        ),
        GeometryUpdaterRegistration::new(
            "WallGeometryUpdater",
            Rc::new(|e, s| Rc::new(RefCell::new(WallGeometryUpdater::new(e, s)))),
        ),
    ]
}

thread_local! {
    static REGISTERED_UPDATERS: RefCell<Vec<GeometryUpdaterRegistration>> = RefCell::new(default_registrations());
}

/// Port of CesiumJS `GeometryUpdaterSet`.
pub struct GeometryUpdaterSet {
    entity: EntityRef,
    scene: Rc<dyn GeometryScene>,
    updaters: Rc<Vec<GeometryUpdaterRef>>,
    geometry_changed: ChangeEvent<GeometryChangedEvent>,
    updater_subscriptions: Vec<Subscription>,
    entity_subscription: Option<Subscription>,
    destroyed: bool,
}

impl GeometryUpdaterSet {
    pub fn new(entity: EntityRef, scene: Rc<dyn GeometryScene>) -> Self {
        let factories: Vec<GeometryUpdaterFactory> =
            REGISTERED_UPDATERS.with(|r| r.borrow().iter().map(|entry| Rc::clone(&entry.factory)).collect());
        Self::with_factories(entity, scene, &factories)
    }

    pub fn with_factories(
        entity: EntityRef,
        scene: Rc<dyn GeometryScene>,
        factories: &[GeometryUpdaterFactory],
    ) -> Self {
        let geometry_changed = ChangeEvent::new();
        let mut updaters = Vec::with_capacity(factories.len());
        let mut updater_subscriptions = Vec::with_capacity(factories.len());

        for factory in factories {
            let updater = factory(Rc::clone(&entity), Rc::clone(&scene));
            let event = geometry_changed.clone();
            let sub = updater.borrow().geometry_changed().add_listener(move |ev: &GeometryChangedEvent| {
                event.raise(ev.clone());
            });
            updater_subscriptions.push(sub);
            updaters.push(updater);
        }

        let updaters = Rc::new(updaters);
        let weak_updaters = Rc::downgrade(&updaters);
        let entity_subscription = entity.definition_changed().add_listener(move |change: &EntityChange| {
            let Some(updaters) = weak_updaters.upgrade() else { return };
            let Some(changed_entity) = change.entity.upgrade() else { return };
            for updater in updaters.iter() {
                updater.borrow_mut().on_entity_property_changed(&changed_entity, &change.property.name);
            }
        });

        Self {
            entity,
            scene,
            updaters,
            geometry_changed,
            updater_subscriptions,
            entity_subscription: Some(entity_subscription),
            destroyed: false,
        }
    }

    pub fn entity(&self) -> &EntityRef {
        &self.entity
    }

    pub fn scene(&self) -> &Rc<dyn GeometryScene> {
        &self.scene
    }

    pub fn updaters(&self) -> &[GeometryUpdaterRef] {
        &self.updaters
    }

    pub fn geometry_changed(&self) -> &ChangeEvent<GeometryChangedEvent> {
        &self.geometry_changed
    }

    pub fn for_each(&self, mut callback: impl FnMut(&GeometryUpdaterRef)) {
        for updater in self.updaters.iter() {
            callback(updater);
        }
    }

    pub fn is_destroyed(&self) -> bool {
        self.destroyed
    }

    pub fn destroy(&mut self) {
        if !self.destroyed {
            self.updater_subscriptions.clear();
            self.entity_subscription = None;
            for updater in self.updaters.iter() {
                updater.borrow_mut().destroy();
            }
            self.destroyed = true;
        }
    }

    /// Add `registration` to the default list of updaters if not already included.
    pub fn register_updater(registration: GeometryUpdaterRegistration) {
        REGISTERED_UPDATERS.with(|list| {
            let mut list = list.borrow_mut();
            if !list.iter().any(|existing| existing.name == registration.name) {
                list.push(registration);
            }
        });
    }

    /// Remove the updater named `name` from the default list of updaters if included.
    pub fn unregister_updater(name: &str) {
        REGISTERED_UPDATERS.with(|list| {
            let mut list = list.borrow_mut();
            if let Some(pos) = list.iter().position(|existing| existing.name == name) {
                list.remove(pos);
            }
        });
    }

    /// Lists the names of all currently registered geometry updaters in order.
    pub fn registered_updater_names() -> Vec<String> {
        REGISTERED_UPDATERS.with(|list| list.borrow().iter().map(|e| e.name.clone()).collect())
    }

    /// Restores the default built-in geometry updater registry.
    pub fn reset_default_updaters() {
        REGISTERED_UPDATERS.with(|list| {
            *list.borrow_mut() = default_registrations();
        });
    }
}
