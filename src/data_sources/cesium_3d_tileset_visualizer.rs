//! CesiumJS `Cesium3DTilesetVisualizer`: maps `Entity.tileset` (`Cesium3DTilesetGraphics`)
//! to [`Cesium3DTileset`] primitives.
use super::{
    visualizer_values::*, BoundingSphereState, Cesium3DTilesetGraphics, CollectionChange, EntityCollection, EntityRef,
    PropertyInput, Subscription, Visualizer,
};
use crate::{
    core::{BoundingSphere, JulianDate, Resource, RuntimeError},
    scene::Cesium3DTileset,
};
use futures_util::{future::LocalBoxFuture, FutureExt};
use std::{cell::RefCell, collections::HashMap, rc::Rc};

/// Async factory loading a [`Cesium3DTileset`] from a [`Resource`].
pub type TilesetFactory =
    Rc<dyn Fn(Resource) -> LocalBoxFuture<'static, Result<Rc<RefCell<Cesium3DTileset>>, RuntimeError>>>;

struct TilesetData {
    url: String,
    load_fail: bool,
    pending: Option<LocalBoxFuture<'static, Result<Rc<RefCell<Cesium3DTileset>>, RuntimeError>>>,
    tileset_primitive: Option<Rc<RefCell<Cesium3DTileset>>>,
}

#[derive(Default)]
struct TilesetRegistry {
    entities_order: Vec<String>,
    entities_map: HashMap<String, EntityRef>,
    tilesets: HashMap<String, TilesetData>,
    primitives: Vec<Rc<RefCell<Cesium3DTileset>>>,
}

impl TilesetRegistry {
    fn insert_entity(&mut self, entity: EntityRef) {
        let id = entity.id().to_owned();
        if !self.entities_map.contains_key(&id) {
            self.entities_order.push(id.clone());
        }
        self.entities_map.insert(id, entity);
    }

    fn remove_entity(&mut self, id: &str) {
        self.remove_tileset(id);
        if self.entities_map.remove(id).is_some() {
            self.entities_order.retain(|k| k != id);
        }
    }

    fn remove_tileset(&mut self, id: &str) {
        if let Some(mut data) = self.tilesets.remove(id) {
            data.pending = None;
            if let Some(tileset) = data.tileset_primitive.take() {
                self.primitives.retain(|p| !Rc::ptr_eq(p, &tileset));
                let _ = tileset.borrow_mut().destroy();
            }
        }
    }
}

/// Port of CesiumJS `Cesium3DTilesetVisualizer`.
pub struct Cesium3DTilesetVisualizer {
    entity_collection: Rc<EntityCollection>,
    factory: TilesetFactory,
    registry: Rc<RefCell<TilesetRegistry>>,
    subscription: Option<Subscription>,
    destroyed: bool,
}

impl Cesium3DTilesetVisualizer {
    pub fn new(entity_collection: Rc<EntityCollection>, factory: TilesetFactory) -> Self {
        let registry = Rc::new(RefCell::new(TilesetRegistry::default()));
        let reg_cb = Rc::clone(&registry);
        let sub = entity_collection.collection_changed().add_listener(move |change: &CollectionChange| {
            on_collection_changed(&reg_cb, &change.added, &change.removed, &change.changed);
        });

        let initial = entity_collection.values();
        on_collection_changed(&registry, &initial, &[], &[]);

        Self { entity_collection, factory, registry, subscription: Some(sub), destroyed: false }
    }

    /// Returns the loaded [`Cesium3DTileset`] for `entity_id`, if available.
    pub fn tileset_for(&self, entity_id: &str) -> Option<Rc<RefCell<Cesium3DTileset>>> {
        self.registry.borrow().tilesets.get(entity_id).and_then(|d| d.tileset_primitive.clone())
    }

    /// Returns all active tileset primitives managed by this visualizer.
    pub fn primitives(&self) -> Vec<Rc<RefCell<Cesium3DTileset>>> {
        self.registry.borrow().primitives.clone()
    }

    /// Non-blockingly polls pending async tileset loads.
    pub fn poll_loads(&mut self) {
        let mut reg = self.registry.borrow_mut();
        let keys: Vec<String> = reg.tilesets.keys().cloned().collect();
        for key in keys {
            let resolved = {
                let Some(data) = reg.tilesets.get_mut(&key) else { continue };
                let Some(fut) = data.pending.as_mut() else { continue };
                fut.now_or_never()
            };
            let Some(result) = resolved else { continue };
            match result {
                Ok(tileset) => {
                    if let Some(data) = reg.tilesets.get_mut(&key) {
                        data.pending = None;
                        data.tileset_primitive = Some(Rc::clone(&tileset));
                        reg.primitives.push(tileset);
                    }
                }
                Err(_) => {
                    if let Some(data) = reg.tilesets.get_mut(&key) {
                        data.pending = None;
                        data.load_fail = true;
                    }
                }
            }
        }
    }
}

impl Visualizer for Cesium3DTilesetVisualizer {
    fn update(&mut self, time: &JulianDate) -> Result<bool, RuntimeError> {
        self.poll_loads();

        let ids: Vec<String> = self.registry.borrow().entities_order.clone();
        for id in ids {
            let entity = match self.registry.borrow().entities_map.get(&id) {
                Some(e) => Rc::clone(e),
                None => continue,
            };
            let Some(PropertyInput::Component(comp)) = entity.get_property("tileset") else { continue };
            let Some(graphics) = comp.as_any().downcast_ref::<Cesium3DTilesetGraphics>() else { continue };

            let show =
                entity.is_showing() && entity.is_available(time) && eval_bool(graphics.show().as_ref(), time, true)?;

            let mut model_matrix = None;
            let mut resource = None;
            if show {
                model_matrix = entity.compute_model_matrix(time, None)?;
                resource = eval_resource_opt(graphics.uri().as_ref(), time)?;
            }

            let existing = self.registry.borrow().tilesets.get(&id).and_then(|d| d.tileset_primitive.clone());

            if !show {
                if let Some(tileset) = existing {
                    tileset.borrow_mut().options.show = false;
                }
                continue;
            }

            let Some(resource) = resource else {
                if let Some(tileset) = existing {
                    tileset.borrow_mut().options.show = false;
                }
                continue;
            };
            let url = resource.url();

            let needs_create = {
                let reg = self.registry.borrow();
                reg.tilesets.get(&id).is_none_or(|d| d.url != url)
            };

            if needs_create {
                self.registry.borrow_mut().remove_tileset(&id);
                let fut = (self.factory)(resource);
                self.registry.borrow_mut().tilesets.insert(
                    id.clone(),
                    TilesetData { url, load_fail: false, pending: Some(fut), tileset_primitive: None },
                );
                self.poll_loads();
            }

            let tileset = self.registry.borrow().tilesets.get(&id).and_then(|d| d.tileset_primitive.clone());
            let Some(tileset) = tileset else { continue };

            let mut ts = tileset.borrow_mut();
            ts.options.show = true;
            if let Some(mm) = model_matrix {
                ts.model_matrix = mm;
            }
            if let Some(sse) = eval_f64_opt(graphics.maximum_screen_space_error().as_ref(), time)? {
                ts.set_maximum_screen_space_error(sse)?;
            }
        }
        Ok(true)
    }

    fn get_bounding_sphere(&self, entity: &EntityRef, result: &mut BoundingSphere) -> BoundingSphereState {
        let reg = self.registry.borrow();
        let Some(data) = reg.tilesets.get(entity.id()) else {
            return BoundingSphereState::Failed;
        };
        if data.load_fail {
            return BoundingSphereState::Failed;
        }
        let Some(primitive) = data.tileset_primitive.as_ref() else {
            return BoundingSphereState::Pending;
        };
        let mut ts = primitive.borrow_mut();
        if !ts.options.show {
            return BoundingSphereState::Failed;
        }
        match ts.bounding_sphere() {
            Ok(Some(sphere)) => {
                *result = sphere;
                BoundingSphereState::Done
            }
            _ => BoundingSphereState::Pending,
        }
    }

    fn is_destroyed(&self) -> bool {
        self.destroyed
    }

    fn destroy(&mut self) {
        if !self.destroyed {
            self.destroyed = true;
            self.subscription = None;
            let mut reg = self.registry.borrow_mut();
            let ids: Vec<String> = reg.entities_order.clone();
            for id in ids.into_iter().rev() {
                reg.remove_tileset(&id);
            }
            reg.entities_order.clear();
            reg.entities_map.clear();
            let _ = &self.entity_collection;
        }
    }
}

fn on_collection_changed(
    registry: &RefCell<TilesetRegistry>,
    added: &[EntityRef],
    removed: &[EntityRef],
    changed: &[EntityRef],
) {
    let mut reg = registry.borrow_mut();
    for entity in added.iter().rev() {
        if entity.get_property("tileset").is_some() {
            reg.insert_entity(Rc::clone(entity));
        }
    }
    for entity in changed.iter().rev() {
        if entity.get_property("tileset").is_some() {
            reg.insert_entity(Rc::clone(entity));
        } else {
            reg.remove_entity(entity.id());
        }
    }
    for entity in removed.iter().rev() {
        reg.remove_entity(entity.id());
    }
}
