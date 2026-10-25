//! Priority-ordered entity layers with owned subscriptions and serialized updates.
use super::{
    ChangeEvent, CollectionChange, DynamicValue, Entity, EntityCollection, EntityCollectionOptions, EntityRef,
    EntitySource, PropertyInput, Subscription,
};
use crate::core::{create_guid, RuntimeError, TimeInterval, TrackingReferenceFrame};
use std::{
    any::Any,
    cell::{Cell, RefCell},
    collections::{HashMap, HashSet, VecDeque},
    rc::{Rc, Weak},
};
pub type EntitySourceRef = Rc<dyn EntitySource>;
struct CollectionBinding {
    source: EntitySourceRef,
    generation: u64,
    _subscription: Subscription,
    entities: HashMap<String, usize>,
}
struct EntityBinding {
    _subscription: Subscription,
    sources: HashSet<String>,
    generation: u64,
}
#[derive(Default)]
struct Bindings {
    collections: HashMap<String, CollectionBinding>,
    entities: HashMap<usize, EntityBinding>,
}
enum Update {
    Recompose,
    Membership { source: EntitySourceRef, generation: u64, added: Vec<EntityRef>, removed: Vec<EntityRef> },
    Definition { entity: EntityRef, generation: u64, name: String },
}
pub struct CompositeEntityCollection {
    id: String,
    owner: Option<Weak<dyn Any>>,
    composite: Rc<EntityCollection>,
    options: EntityCollectionOptions,
    collections: RefCell<Vec<EntitySourceRef>>,
    committed: RefCell<Vec<EntitySourceRef>>,
    bindings: RefCell<Bindings>,
    generation: Cell<u64>,
    suspended: Cell<usize>,
    dirty: Cell<bool>,
    recompose_queued: Cell<bool>,
    processing: Cell<bool>,
    updates: RefCell<VecDeque<Update>>,
    last_error: RefCell<Option<RuntimeError>>,
    errors: ChangeEvent<RuntimeError>,
    weak: Weak<Self>,
}
impl CompositeEntityCollection {
    pub fn new(collections: Vec<EntitySourceRef>) -> Result<Rc<Self>, RuntimeError> {
        Self::with_options(collections, EntityCollectionOptions::default())
    }
    pub fn with_options(
        collections: Vec<EntitySourceRef>,
        options: EntityCollectionOptions,
    ) -> Result<Rc<Self>, RuntimeError> {
        let result = Rc::new_cyclic(|weak: &Weak<Self>| {
            let mut inner_options = options.clone();
            inner_options.owner = Some(weak.clone());
            Self {
                id: create_guid::create_guid(),
                owner: options.owner.clone(),
                composite: EntityCollection::with_options(inner_options),
                options: options.clone(),
                collections: RefCell::new(collections),
                committed: RefCell::new(vec![]),
                bindings: RefCell::new(Bindings::default()),
                generation: Cell::new(0),
                suspended: Cell::new(0),
                dirty: Cell::new(false),
                recompose_queued: Cell::new(false),
                processing: Cell::new(false),
                updates: RefCell::new(VecDeque::new()),
                last_error: RefCell::new(None),
                errors: ChangeEvent::new(),
                weak: weak.clone(),
            }
        });
        result.recompose()?;
        Ok(result)
    }
    pub fn id(&self) -> &str {
        &self.id
    }
    pub fn owner(&self) -> Option<Rc<dyn Any>> {
        self.owner.as_ref().and_then(Weak::upgrade)
    }
    pub fn collection_changed(&self) -> &ChangeEvent<CollectionChange> {
        self.composite.collection_changed()
    }
    /// Failures from automatic source callbacks are retained here and emitted by `errors`.
    pub fn take_error(&self) -> Option<RuntimeError> {
        self.last_error.borrow_mut().take()
    }
    pub fn errors(&self) -> &ChangeEvent<RuntimeError> {
        &self.errors
    }
    pub fn values(&self) -> Vec<EntityRef> {
        self.composite.values()
    }
    pub fn len(&self) -> usize {
        self.composite.len()
    }
    pub fn is_empty(&self) -> bool {
        self.composite.is_empty()
    }
    pub fn contains(&self, entity: &EntityRef) -> bool {
        self.composite.contains(entity)
    }
    pub fn get_by_id(&self, id: &str) -> Option<EntityRef> {
        self.composite.get_by_id(id)
    }
    pub fn compute_availability(&self) -> TimeInterval<()> {
        self.composite.compute_availability()
    }
    pub fn collections(&self) -> Vec<EntitySourceRef> {
        self.collections.borrow().clone()
    }
    pub fn get_collection(&self, index: usize) -> Option<EntitySourceRef> {
        self.collections.borrow().get(index).cloned()
    }
    pub fn collections_len(&self) -> usize {
        self.collections.borrow().len()
    }
    pub fn index_of_collection(&self, source: &EntitySourceRef) -> Option<usize> {
        self.collections.borrow().iter().position(|s| Rc::ptr_eq(s, source))
    }
    pub fn contains_collection(&self, source: &EntitySourceRef) -> bool {
        self.index_of_collection(source).is_some()
    }
    pub fn add_collection(&self, source: EntitySourceRef, index: Option<usize>) -> Result<(), RuntimeError> {
        if source.contains_source(self.id()) {
            return Err(RuntimeError::new("collection would create a cycle"));
        }
        {
            let mut collections = self.collections.borrow_mut();
            let index = index.unwrap_or(collections.len());
            if index > collections.len() {
                return Err(RuntimeError::new("index exceeds the number of collections"));
            }
            collections.insert(index, source);
        }
        self.recompose()
    }
    pub fn remove_collection(&self, source: &EntitySourceRef) -> Result<bool, RuntimeError> {
        let Some(index) = self.index_of_collection(source) else { return Ok(false) };
        let removed = self.collections.borrow_mut().remove(index);
        drop(removed);
        self.recompose()?;
        Ok(true)
    }
    pub fn remove_all_collections(&self) -> Result<(), RuntimeError> {
        let old = std::mem::take(&mut *self.collections.borrow_mut());
        drop(old);
        self.recompose()
    }
    fn collection_index(&self, source: &EntitySourceRef) -> Result<usize, RuntimeError> {
        self.index_of_collection(source).ok_or_else(|| RuntimeError::new("collection is not in this composite."))
    }
    fn swap(&self, a: usize, b: usize) -> Result<(), RuntimeError> {
        if a != b {
            self.collections.borrow_mut().swap(a, b);
            self.recompose()?;
        }
        Ok(())
    }
    pub fn raise_collection(&self, source: &EntitySourceRef) -> Result<(), RuntimeError> {
        let i = self.collection_index(source)?;
        self.swap(i, (i + 1).min(self.collections_len() - 1))
    }
    pub fn lower_collection(&self, source: &EntitySourceRef) -> Result<(), RuntimeError> {
        let i = self.collection_index(source)?;
        self.swap(i, i.saturating_sub(1))
    }
    pub fn raise_collection_to_top(&self, source: &EntitySourceRef) -> Result<(), RuntimeError> {
        let i = self.collection_index(source)?;
        if i != self.collections_len() - 1 {
            let source = self.collections.borrow_mut().remove(i);
            self.collections.borrow_mut().push(source);
            self.recompose()?;
        }
        Ok(())
    }
    pub fn lower_collection_to_bottom(&self, source: &EntitySourceRef) -> Result<(), RuntimeError> {
        let i = self.collection_index(source)?;
        if i != 0 {
            let source = self.collections.borrow_mut().remove(i);
            self.collections.borrow_mut().insert(0, source);
            self.recompose()?;
        }
        Ok(())
    }
    pub fn suspend_events(&self) {
        self.suspended.set(self.suspended.get() + 1);
        self.composite.suspend_events();
    }
    pub fn resume_events(&self) -> Result<(), RuntimeError> {
        if self.suspended.get() == 0 {
            return Err(RuntimeError::new("resumeEvents can not be called before suspendEvents."));
        }
        self.suspended.set(self.suspended.get() - 1);
        let update = if self.suspended.get() == 0 && self.dirty.get() { self.recompose() } else { Ok(()) };
        let resume = self.composite.resume_events();
        update.and(resume)
    }
    /// Rebuild all layers; also retries after a fallible application component fails.
    pub fn recompose(&self) -> Result<(), RuntimeError> {
        self.dirty.set(true);
        if self.suspended.get() != 0 || self.recompose_queued.replace(true) {
            return Ok(());
        }
        self.enqueue(Update::Recompose)
    }
    fn enqueue(&self, update: Update) -> Result<(), RuntimeError> {
        self.updates.borrow_mut().push_back(update);
        if self.processing.replace(true) {
            return Ok(());
        }
        let mut first_error = None;
        loop {
            let update = self.updates.borrow_mut().pop_front();
            let Some(update) = update else { break };
            let result = match update {
                Update::Recompose => {
                    self.recompose_queued.set(false);
                    self.dirty.set(false);
                    self.rebuild()
                }
                Update::Membership { source, generation, added, removed } => {
                    if self.source_is_bound(&source, generation) {
                        self.update_membership(source, added, removed)
                    } else {
                        Ok(())
                    }
                }
                Update::Definition { entity, generation, name } => {
                    let current = self
                        .bindings
                        .borrow()
                        .entities
                        .get(&(Rc::as_ptr(&entity) as usize))
                        .is_some_and(|b| b.generation == generation);
                    if current {
                        self.update_definition(&entity, &name)
                    } else {
                        Ok(())
                    }
                }
            };
            if let Err(error) = result {
                self.dirty.set(true);
                self.last_error.replace(Some(error.clone()));
                first_error.get_or_insert(error.clone());
                self.errors.raise(error);
            }
        }
        self.processing.set(false);
        first_error.map_or(Ok(()), Err)
    }
    fn bind_collection(&self, source: EntitySourceRef) {
        if self.bindings.borrow().collections.contains_key(source.id()) {
            return;
        }
        let weak = self.weak.clone();
        let weak_source = Rc::downgrade(&source);
        let generation = self.generation.get().wrapping_add(1);
        self.generation.set(generation);
        let subscription = source.collection_changed().add_listener(move |change| {
            if let (Some(this), Some(source)) = (weak.upgrade(), weak_source.upgrade()) {
                let active = this.source_is_bound(&source, generation);
                if active && (!change.added.is_empty() || !change.removed.is_empty()) {
                    let _ = this.enqueue(Update::Membership {
                        source,
                        generation,
                        added: change.added.clone(),
                        removed: change.removed.clone(),
                    });
                }
            }
        });
        self.bindings.borrow_mut().collections.insert(
            source.id().to_owned(),
            CollectionBinding { source, generation, _subscription: subscription, entities: HashMap::new() },
        );
    }
    fn source_is_bound(&self, source: &EntitySourceRef, generation: u64) -> bool {
        self.bindings
            .borrow()
            .collections
            .get(source.id())
            .is_some_and(|b| b.generation == generation && Rc::ptr_eq(&b.source, source))
    }
    fn bind_entity(&self, source: &EntitySourceRef, entity: &EntityRef) {
        self.unbind_entity(source.id(), entity.id());
        let key = Rc::as_ptr(entity) as usize;
        let exists = self.bindings.borrow().entities.contains_key(&key);
        if !exists {
            let generation = self.generation.get().wrapping_add(1);
            self.generation.set(generation);
            let weak = self.weak.clone();
            let weak_entity = Rc::downgrade(entity);
            let subscription = entity.definition_changed().add_listener(move |change| {
                if let (Some(this), Some(entity)) = (weak.upgrade(), weak_entity.upgrade()) {
                    let sources = {
                        let bindings = this.bindings.borrow();
                        bindings
                            .entities
                            .get(&key)
                            .filter(|b| b.generation == generation)
                            .map(|b| {
                                b.sources
                                    .iter()
                                    .filter_map(|id| bindings.collections.get(id).map(|b| b.source.clone()))
                                    .collect::<Vec<_>>()
                            })
                            .unwrap_or_default()
                    };
                    if sources.iter().any(|s| s.get_by_id(entity.id()).is_some_and(|e| Rc::ptr_eq(&e, &entity))) {
                        let _ =
                            this.enqueue(Update::Definition { entity, generation, name: change.property.name.clone() });
                    }
                }
            });
            self.bindings
                .borrow_mut()
                .entities
                .insert(key, EntityBinding { _subscription: subscription, sources: HashSet::new(), generation });
        }
        let mut bindings = self.bindings.borrow_mut();
        bindings.entities.get_mut(&key).unwrap().sources.insert(source.id().to_owned());
        if let Some(binding) = bindings.collections.get_mut(source.id()) {
            binding.entities.insert(entity.id().to_owned(), key);
        }
    }
    fn unbind_entity(&self, source: &str, id: &str) {
        let removed = {
            let mut bindings = self.bindings.borrow_mut();
            let key = bindings.collections.get_mut(source).and_then(|b| b.entities.remove(id));
            key.and_then(|key| {
                let binding = bindings.entities.get_mut(&key)?;
                binding.sources.remove(source);
                binding.sources.is_empty().then(|| bindings.entities.remove(&key)).flatten()
            })
        };
        drop(removed);
    }
    fn rebuild(&self) -> Result<(), RuntimeError> {
        let sources = self.collections();
        let old = std::mem::take(&mut *self.bindings.borrow_mut());
        drop(old);
        let mut options = self.options.clone();
        options.owner = Some(self.weak.clone());
        let staged = EntityCollection::with_options(options);
        for source in sources.iter().rev() {
            self.bind_collection(source.clone());
            for entity in source.values().iter().rev() {
                self.bind_entity(source, entity);
                let target = match staged.get_by_id(entity.id()) {
                    Some(target) => target,
                    None => {
                        let target = match self.composite.get_by_id(entity.id()) {
                            Some(target) => {
                                target.clean_for_composite()?;
                                target
                            }
                            None => staged.get_or_create_entity(entity.id()),
                        };
                        if !staged.contains(&target) {
                            staged.add(target.clone())?;
                        }
                        target
                    }
                };
                target.merge(entity)?;
            }
        }
        let retired = self.committed.replace(sources);
        drop(retired);
        self.composite.suspend_events();
        self.composite.remove_all();
        let update = staged.values().into_iter().try_for_each(|e| self.composite.add(e).map(|_| ()));
        let resume = self.composite.resume_events();
        update.and(resume)
    }
    fn update_membership(
        &self,
        source: EntitySourceRef,
        added: Vec<EntityRef>,
        removed: Vec<EntityRef>,
    ) -> Result<(), RuntimeError> {
        if !self.bindings.borrow().collections.get(source.id()).is_some_and(|b| Rc::ptr_eq(&b.source, &source)) {
            return Ok(());
        }
        self.composite.suspend_events();
        let result = (|| {
            for entity in removed {
                self.unbind_entity(source.id(), entity.id());
                self.merge_id(entity.id(), false)?;
            }
            for entity in added {
                if !source.get_by_id(entity.id()).is_some_and(|e| Rc::ptr_eq(&e, &entity)) {
                    continue;
                }
                self.bind_entity(&source, &entity);
                self.merge_id(entity.id(), true)?;
            }
            Ok(())
        })();
        let resume = self.composite.resume_events();
        result.and(resume)
    }
    fn merge_id(&self, id: &str, adding: bool) -> Result<(), RuntimeError> {
        let sources = self.committed.borrow().clone();
        let entities = sources.iter().rev().filter_map(|s| s.get_by_id(id)).collect::<Vec<_>>();
        if entities.is_empty() {
            self.composite.remove_by_id(id);
            return Ok(());
        }
        let target = match self.composite.get_by_id(id) {
            Some(entity) => {
                entity.clean_for_composite()?;
                entity
            }
            None if adding => self.composite.get_or_create_entity(id),
            None => return Ok(()),
        };
        for entity in entities {
            target.merge(&entity)?;
        }
        Ok(())
    }
    fn update_definition(&self, entity: &Entity, name: &str) -> Result<(), RuntimeError> {
        // isShowing is derived, read-only entity state, not a composable field.
        if name == "isShowing" {
            return Ok(());
        }
        let Some(target) = self.composite.get_by_id(entity.id()) else { return Ok(()) };
        let mut value = target.get_property(name);
        let mut first = true;
        for source in self.collections().iter().rev() {
            let property =
                source.get_by_id(entity.id()).and_then(|e| e.get_property(name)).filter(PropertyInput::is_defined);
            let Some(property) = property else { continue };
            if first {
                first = false;
                if let Some(component) = property.component().filter(|p| p.supports_merge() && p.supports_clone()) {
                    value =
                        Some(PropertyInput::Component(component.clone_component(value.and_then(|p| p.component()))?));
                } else {
                    value = Some(property);
                    break;
                }
            }
            let (Some(target), Some(source)) =
                (value.as_ref().and_then(PropertyInput::component), property.component())
            else {
                return Err(RuntimeError::new("property.merge is not a function"));
            };
            target.merge_component(&*source)?;
        }
        match name {
            "name" => target.set_name(match value {
                Some(PropertyInput::Value(DynamicValue::String(v))) => Some(v),
                None => None,
                _ => return Err(RuntimeError::new("name requires a string")),
            }),
            "availability" => target.set_availability(match value {
                Some(PropertyInput::Value(DynamicValue::Availability(v))) => v.upgrade(),
                None => None,
                _ => return Err(RuntimeError::new("availability requires intervals")),
            }),
            "parent" => target.set_parent(match value {
                Some(PropertyInput::Value(DynamicValue::Entity(v))) => v.upgrade(),
                None => None,
                _ => return Err(RuntimeError::new("parent requires an entity")),
            })?,
            "show" => {
                if let Some(PropertyInput::Value(DynamicValue::Boolean(v))) = value {
                    target.set_show(v);
                }
            }
            "trackingReferenceFrame" => {
                if let Some(PropertyInput::Value(DynamicValue::Number(v))) = value {
                    target.set_tracking_reference_frame(
                        TrackingReferenceFrame::from_value(v as u8)
                            .ok_or_else(|| RuntimeError::new("invalid tracking frame"))?,
                    );
                }
            }
            _ => {
                if !target.property_names().iter().any(|n| n == name) && !Entity::is_reserved(name) {
                    target.add_property(name)?;
                }
                target.set_property(name, value)?;
            }
        }
        Ok(())
    }
}
impl EntitySource for CompositeEntityCollection {
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
    fn contains_source(&self, id: &str) -> bool {
        self.id() == id || self.collections().iter().any(|s| s.contains_source(id))
    }
}
