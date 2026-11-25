//! Ordered entity membership with coalesced, reentrant collection notifications.
use super::{
    ChangeEvent, ChangeListener, Entity, EntityChange, EntityOptions, EntityRef, EntityTypeRegistry, PositionContext,
    Subscription,
};
use crate::core::{create_guid, iso8601, JulianDate, RuntimeError, TimeInterval};
use std::{
    any::Any,
    cell::{Cell, RefCell},
    collections::{HashMap, HashSet},
    rc::{Rc, Weak},
};
#[derive(Clone)]
pub struct CollectionChange {
    pub collection: Weak<EntityCollection>,
    pub added: Vec<EntityRef>,
    pub removed: Vec<EntityRef>,
    pub changed: Vec<EntityRef>,
}
#[derive(Default)]
struct PendingEntities {
    list: Vec<EntityRef>,
    ids: HashSet<String>,
}
impl PendingEntities {
    fn is_empty(&self) -> bool {
        self.list.is_empty()
    }
    fn contains(&self, id: &str) -> bool {
        self.ids.contains(id)
    }
    fn put(&mut self, entity: EntityRef) {
        if self.ids.insert(entity.id().to_owned()) {
            self.list.push(entity);
        } else if let Some(i) = self.list.iter().position(|e| e.id() == entity.id()) {
            self.list[i] = entity;
        }
    }
    fn take_id(&mut self, id: &str) -> Option<EntityRef> {
        if self.ids.remove(id) {
            self.list.iter().position(|e| e.id() == id).map(|i| self.list.remove(i))
        } else {
            None
        }
    }
    fn take_vec(&mut self) -> Vec<EntityRef> {
        self.ids.clear();
        std::mem::take(&mut self.list)
    }
}
#[derive(Default)]
struct State {
    entities: Vec<EntityRef>,
    by_id: HashMap<String, EntityRef>,
    added: PendingEntities,
    removed: PendingEntities,
    changed: PendingEntities,
    subscriptions: HashMap<String, Subscription>,
    suspend: usize,
    firing: bool,
    refire: bool,
}
#[derive(Clone, Default)]
pub struct EntityCollectionOptions {
    pub owner: Option<Weak<dyn Any>>,
    pub registry: EntityTypeRegistry,
    pub context: PositionContext,
}
pub struct EntityCollection {
    id: String,
    registry: EntityTypeRegistry,
    context: PositionContext,
    owner: RefCell<Option<Weak<dyn Any>>>,
    state: RefCell<State>,
    show: Cell<bool>,
    changed: ChangeEvent<CollectionChange>,
    listener: ChangeListener<EntityChange>,
    weak: Weak<EntityCollection>,
}
fn take_id(values: &mut Vec<EntityRef>, id: &str) -> Option<EntityRef> {
    values.iter().position(|e| e.id() == id).map(|i| values.remove(i))
}
impl EntityCollection {
    pub fn new() -> Rc<Self> {
        Self::with_options(EntityCollectionOptions::default())
    }
    pub fn with_owner(owner: Option<Weak<dyn Any>>) -> Rc<Self> {
        Self::with_options(EntityCollectionOptions { owner, ..Default::default() })
    }
    pub fn with_options(options: EntityCollectionOptions) -> Rc<Self> {
        Rc::new_cyclic(|weak: &Weak<Self>| {
            let target = weak.clone();
            let listener = Rc::new(move |change: &EntityChange| {
                if let (Some(collection), Some(entity)) = (target.upgrade(), change.entity.upgrade()) {
                    collection.on_entity_changed(entity);
                }
            });
            Self {
                id: create_guid::create_guid(),
                owner: RefCell::new(options.owner),
                registry: options.registry,
                context: options.context,
                state: RefCell::new(State::default()),
                show: Cell::new(true),
                changed: ChangeEvent::new(),
                listener,
                weak: weak.clone(),
            }
        })
    }
    pub fn id(&self) -> &str {
        &self.id
    }
    pub fn owner(&self) -> Option<Rc<dyn Any>> {
        self.owner.borrow().as_ref().and_then(Weak::upgrade)
    }
    pub fn collection_changed(&self) -> &ChangeEvent<CollectionChange> {
        &self.changed
    }
    pub fn values(&self) -> Vec<EntityRef> {
        self.state.borrow().entities.clone()
    }
    pub fn len(&self) -> usize {
        self.state.borrow().entities.len()
    }
    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
    pub fn show(&self) -> bool {
        self.show.get()
    }
    pub fn set_show(&self, value: bool) -> Result<(), RuntimeError> {
        if value == self.show.get() {
            return Ok(());
        }
        self.suspend_events();
        let entities = self.values();
        let old = entities.iter().map(|e| e.is_showing()).collect::<Vec<_>>();
        self.show.set(value);
        for (entity, old) in entities.iter().zip(old) {
            let new = entity.is_showing();
            if old != new {
                entity.notify_is_showing(new, old);
            }
        }
        self.resume_events()
    }
    pub fn suspend_events(&self) {
        self.state.borrow_mut().suspend += 1;
    }
    pub fn resume_events(&self) -> Result<(), RuntimeError> {
        if self.state.borrow().suspend == 0 {
            return Err(RuntimeError::new("resumeEvents can not be called before suspendEvents."));
        }
        self.state.borrow_mut().suspend -= 1;
        self.fire_changed();
        Ok(())
    }
    pub fn get_by_id(&self, id: &str) -> Option<EntityRef> {
        self.state.borrow().by_id.get(id).cloned()
    }
    pub fn get_or_create_entity(&self, id: impl Into<String>) -> EntityRef {
        let id = id.into();
        if let Some(entity) = self.get_by_id(&id) {
            return entity;
        }
        let entity = Entity::new(EntityOptions {
            id: Some(id),
            registry: self.registry.clone(),
            context: self.context.clone(),
            ..Default::default()
        })
        .expect("default entity options");
        self.add(entity.clone()).expect("fresh entity id");
        entity
    }
    pub fn contains(&self, entity: &EntityRef) -> bool {
        self.get_by_id(entity.id()).is_some_and(|e| Rc::ptr_eq(&e, entity))
    }
    pub fn add(&self, entity: EntityRef) -> Result<EntityRef, RuntimeError> {
        if self.get_by_id(entity.id()).is_some() {
            return Err(RuntimeError::new(format!(
                "An entity with id {} already exists in this collection.",
                entity.id()
            )));
        }
        entity.set_entity_collection(self.weak.clone());
        let subscription = entity.definition_changed().subscribe(self.listener.clone());
        {
            let mut state = self.state.borrow_mut();
            let id = entity.id().to_owned();
            state.by_id.insert(id.clone(), entity.clone());
            state.entities.push(entity.clone());
            if state.removed.take_id(&id).is_none() {
                state.added.put(entity.clone());
            }
            state.subscriptions.insert(id, subscription);
        }
        self.fire_changed();
        Ok(entity)
    }
    pub fn remove(&self, entity: Option<&EntityRef>) -> bool {
        entity.is_some_and(|e| self.remove_by_id(e.id()))
    }
    pub fn remove_by_id(&self, id: &str) -> bool {
        let (subscription, entity) = {
            let mut state = self.state.borrow_mut();
            if state.by_id.remove(id).is_none() {
                return false;
            }
            let entity = take_id(&mut state.entities, id).unwrap();
            if state.added.take_id(id).is_none() {
                state.removed.put(entity.clone());
                state.changed.take_id(id);
            }
            (state.subscriptions.remove(id), entity)
        };
        drop(subscription);
        drop(entity);
        self.fire_changed();
        true
    }
    pub fn remove_all(&self) {
        let retired = {
            let mut state = self.state.borrow_mut();
            let entities = std::mem::take(&mut state.entities);
            for entity in &entities {
                if !state.added.contains(entity.id()) {
                    state.removed.put(entity.clone());
                }
            }
            (
                entities,
                std::mem::take(&mut state.by_id),
                state.added.take_vec(),
                state.changed.take_vec(),
                std::mem::take(&mut state.subscriptions),
            )
        };
        drop(retired);
        self.fire_changed();
    }
    fn on_entity_changed(&self, entity: EntityRef) {
        {
            let mut state = self.state.borrow_mut();
            if !state.by_id.get(entity.id()).is_some_and(|e| Rc::ptr_eq(e, &entity)) {
                return;
            }
            if !state.added.contains(entity.id()) {
                state.changed.put(entity);
            }
        }
        self.fire_changed();
    }
    fn fire_changed(&self) {
        {
            let mut state = self.state.borrow_mut();
            if state.suspend != 0 || state.added.is_empty() && state.removed.is_empty() && state.changed.is_empty() {
                return;
            }
            if state.firing {
                state.refire = true;
                return;
            }
            state.firing = true;
        }
        loop {
            let event = {
                let mut state = self.state.borrow_mut();
                state.refire = false;
                CollectionChange {
                    collection: self.weak.clone(),
                    added: state.added.take_vec(),
                    removed: state.removed.take_vec(),
                    changed: state.changed.take_vec(),
                }
            };
            self.changed.raise(event);
            let mut state = self.state.borrow_mut();
            if !state.refire
                || state.suspend != 0
                || state.added.is_empty() && state.removed.is_empty() && state.changed.is_empty()
            {
                state.firing = false;
                break;
            }
        }
    }
    pub fn compute_availability(&self) -> TimeInterval<()> {
        let (mut start, mut stop) = (iso8601::maximum_value(), iso8601::minimum_value());
        for entity in self.values() {
            if let Some(availability) = entity.availability() {
                let dates = availability.read();
                if let Some(a) = dates.start() {
                    if a != iso8601::minimum_value() && JulianDate::less_than(&a, &start) {
                        start = a;
                    }
                }
                if let Some(b) = dates.stop() {
                    if b != iso8601::maximum_value() && JulianDate::greater_than(&b, &stop) {
                        stop = b;
                    }
                }
            }
        }
        if start == iso8601::maximum_value() {
            start = iso8601::minimum_value();
        }
        if stop == iso8601::minimum_value() {
            stop = iso8601::maximum_value();
        }
        TimeInterval::new(start, stop, true, true, None)
    }
}

pub trait EntitySource: Any {
    fn id(&self) -> &str;
    fn values(&self) -> Vec<EntityRef>;
    fn get_by_id(&self, id: &str) -> Option<EntityRef>;
    fn collection_changed(&self) -> &ChangeEvent<CollectionChange>;
    fn contains_source(&self, id: &str) -> bool {
        self.id() == id
    }
}
impl EntitySource for EntityCollection {
    fn id(&self) -> &str {
        self.id()
    }
    fn values(&self) -> Vec<EntityRef> {
        self.values()
    }
    fn get_by_id(&self, id: &str) -> Option<EntityRef> {
        self.get_by_id(id)
    }
    fn collection_changed(&self) -> &ChangeEvent<CollectionChange> {
        self.collection_changed()
    }
}
