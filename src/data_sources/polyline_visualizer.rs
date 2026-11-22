//! CesiumJS `PolylineVisualizer`: routes [`PolylineGeometryUpdater`] instances
//! into static color, material, ground polyline, and dynamic polyline batches.
use super::{
    geometry_updater::{
        eval_classification_type, is_color_material_component, GeometryAppearanceKind, GeometryChangedEvent,
        GeometryScene, GeometryUpdater, GeometryUpdaterRef,
    },
    visualizer_values::*,
    BoundingSphereState, CollectionChange, DynamicGeometryBatch, EntityCollection, EntityRef, PolylineGeometryUpdater,
    StaticGeometryColorBatch, StaticGeometryPerMaterialBatch, StaticGroundPolylinePerMaterialBatch, Subscription,
    Visualizer,
};
use crate::{
    core::{BoundingSphere, JulianDate, RuntimeError},
    scene::{ClassificationType, OrderedGroundPrimitiveCollection, PrimitiveCollection, ShadowMode},
};
use std::{cell::RefCell, collections::HashMap, rc::Rc};

#[derive(Default)]
struct EntityBucket {
    order: Vec<String>,
    map: HashMap<String, EntityRef>,
}

impl EntityBucket {
    fn set(&mut self, id: String, entity: EntityRef) {
        if !self.map.contains_key(&id) {
            self.order.push(id.clone());
        }
        self.map.insert(id, entity);
    }

    fn remove(&mut self, id: &str) -> bool {
        if self.map.remove(id).is_some() {
            self.order.retain(|k| k != id);
            true
        } else {
            false
        }
    }

    fn contains(&self, id: &str) -> bool {
        self.map.contains_key(id)
    }

    fn drain(&mut self) -> Vec<EntityRef> {
        let mut out = Vec::with_capacity(self.order.len());
        for id in self.order.drain(..) {
            if let Some(e) = self.map.remove(&id) {
                out.push(e);
            }
        }
        out
    }

    fn clear(&mut self) {
        self.order.clear();
        self.map.clear();
    }
}

#[derive(Default)]
struct PendingChanges {
    added: EntityBucket,
    removed: EntityBucket,
    changed: EntityBucket,
}

impl PendingChanges {
    fn on_collection_changed(&mut self, added: &[EntityRef], removed: &[EntityRef]) {
        for entity in removed.iter().rev() {
            let id = entity.id().to_owned();
            if !self.added.remove(&id) {
                self.removed.set(id.clone(), Rc::clone(entity));
                self.changed.remove(&id);
            }
        }
        for entity in added.iter().rev() {
            let id = entity.id().to_owned();
            if self.removed.remove(&id) {
                self.changed.set(id, Rc::clone(entity));
            } else {
                self.added.set(id, Rc::clone(entity));
            }
        }
    }

    fn on_geometry_changed(&mut self, entity: &EntityRef) {
        let id = entity.id();
        if !self.removed.contains(id) && !self.changed.contains(id) {
            self.changed.set(id.to_owned(), Rc::clone(entity));
        }
    }

    fn clear(&mut self) {
        self.added.clear();
        self.removed.clear();
        self.changed.clear();
    }
}

/// Port of CesiumJS `PolylineVisualizer`.
pub struct PolylineVisualizer {
    scene: Rc<dyn GeometryScene>,
    primitives: Rc<RefCell<PrimitiveCollection>>,
    ground_primitives: Rc<RefCell<OrderedGroundPrimitiveCollection>>,
    pending: Rc<RefCell<PendingChanges>>,
    color_batches: Vec<StaticGeometryColorBatch>,
    material_batches: Vec<StaticGeometryPerMaterialBatch>,
    dynamic_batch: DynamicGeometryBatch,
    ground_batches: Vec<StaticGroundPolylinePerMaterialBatch>,
    updaters: HashMap<String, GeometryUpdaterRef>,
    subscriptions: HashMap<String, Subscription>,
    collection_subscription: Option<Subscription>,
    destroyed: bool,
}

impl PolylineVisualizer {
    pub fn new(
        scene: Rc<dyn GeometryScene>,
        entity_collection: &EntityCollection,
        primitives: Option<Rc<RefCell<PrimitiveCollection>>>,
        ground_primitives: Option<Rc<RefCell<OrderedGroundPrimitiveCollection>>>,
    ) -> Self {
        let primitives = primitives.unwrap_or_else(|| scene.primitives());
        let ground_primitives = ground_primitives.unwrap_or_else(|| scene.ground_primitives());

        let n_shadows = ShadowMode::NUMBER_OF_SHADOW_MODES as usize;
        let mut color_batches = Vec::with_capacity(n_shadows * 3);
        let mut material_batches = Vec::with_capacity(n_shadows * 3);

        // Tier 0: no depth-fail; Tier 1: PolylineColor depth-fail; Tier 2: PolylineMaterial depth-fail.
        let depth_fail_tiers =
            [None, Some(GeometryAppearanceKind::PolylineColor), Some(GeometryAppearanceKind::PolylineMaterial)];
        for depth_fail in depth_fail_tiers {
            for i in 0..n_shadows {
                let shadows = ShadowMode::from_value(i as i32).unwrap_or(ShadowMode::Disabled);
                color_batches.push(StaticGeometryColorBatch::new(
                    Rc::clone(&primitives),
                    GeometryAppearanceKind::PolylineColor,
                    depth_fail,
                    false,
                    shadows,
                ));
                material_batches.push(StaticGeometryPerMaterialBatch::new(
                    Rc::clone(&primitives),
                    GeometryAppearanceKind::PolylineMaterial,
                    depth_fail,
                    false,
                    shadows,
                ));
            }
        }

        let dynamic_batch = DynamicGeometryBatch::new(Rc::clone(&primitives), Rc::clone(&ground_primitives));

        let n_class = ClassificationType::NUMBER_OF_CLASSIFICATION_TYPES as usize;
        let mut ground_batches = Vec::with_capacity(n_class);
        for i in 0..n_class {
            let ct = ClassificationType::from_value(i as i32).unwrap_or(ClassificationType::Both);
            ground_batches.push(StaticGroundPolylinePerMaterialBatch::new(Rc::clone(&ground_primitives), ct, None));
        }

        let pending = Rc::new(RefCell::new(PendingChanges::default()));
        let pending_cb = Rc::clone(&pending);
        let collection_subscription =
            entity_collection.collection_changed().add_listener(move |change: &CollectionChange| {
                pending_cb.borrow_mut().on_collection_changed(&change.added, &change.removed);
            });

        let initial_values = entity_collection.values();
        pending.borrow_mut().on_collection_changed(&initial_values, &[]);

        Self {
            scene,
            primitives,
            ground_primitives,
            pending,
            color_batches,
            material_batches,
            dynamic_batch,
            ground_batches,
            updaters: HashMap::new(),
            subscriptions: HashMap::new(),
            collection_subscription: Some(collection_subscription),
            destroyed: false,
        }
    }

    pub fn scene(&self) -> &Rc<dyn GeometryScene> {
        &self.scene
    }

    pub fn primitives(&self) -> &Rc<RefCell<PrimitiveCollection>> {
        &self.primitives
    }

    pub fn ground_primitives(&self) -> &Rc<RefCell<OrderedGroundPrimitiveCollection>> {
        &self.ground_primitives
    }

    pub fn updater_count(&self) -> usize {
        self.updaters.len()
    }

    pub fn total_batch_count(&self) -> usize {
        self.color_batches.len() + self.material_batches.len() + 1 + self.ground_batches.len()
    }

    pub fn mark_all_primitives_ready(&self) {
        for b in &self.color_batches {
            b.mark_all_ready();
        }
        for b in &self.material_batches {
            b.mark_all_ready();
        }
        for b in &self.ground_batches {
            b.mark_all_ready();
        }
    }

    fn remove_updater(&mut self, updater: &dyn GeometryUpdater) {
        for b in &mut self.color_batches {
            b.remove(updater);
        }
        for b in &mut self.material_batches {
            b.remove(updater);
        }
        self.dynamic_batch.remove(updater);
        for b in &mut self.ground_batches {
            b.remove(updater);
        }
    }

    fn insert_updater_into_batch(
        &mut self,
        time: &JulianDate,
        updater: GeometryUpdaterRef,
    ) -> Result<(), RuntimeError> {
        let (fill_enabled, clamp, class_idx, is_color, batch_idx) = {
            let u = updater.borrow();
            if u.is_dynamic() {
                return self.dynamic_batch.add(time, &*u);
            }
            let fill = u.fill_enabled();
            if !fill {
                return Ok(());
            }
            let clamp = u.clamp_to_ground();
            if clamp {
                let cls = eval_classification_type(
                    u.classification_type_property().as_ref(),
                    time,
                    ClassificationType::Both,
                )?
                .value() as usize;
                (fill, true, cls, false, 0)
            } else {
                let shadows =
                    eval_shadow_mode(u.shadows_property().as_ref(), time, ShadowMode::Disabled)?.value() as usize;
                let multiplier = match u.depth_fail_material_property() {
                    None => 0,
                    Some(ref df) if is_color_material_component(df) => 1,
                    Some(_) => 2,
                };
                let idx = shadows + multiplier * (ShadowMode::NUMBER_OF_SHADOW_MODES as usize);
                let is_col = u.fill_material_property().as_ref().is_some_and(is_color_material_component);
                (fill, false, 0, is_col, idx)
            }
        };

        if !fill_enabled {
            return Ok(());
        }
        if clamp {
            self.ground_batches[class_idx].add(time, updater)?;
        } else if is_color {
            self.color_batches[batch_idx].add(time, updater)?;
        } else {
            self.material_batches[batch_idx].add(time, updater)?;
        }
        Ok(())
    }
}

impl Visualizer for PolylineVisualizer {
    fn update(&mut self, time: &JulianDate) -> Result<bool, RuntimeError> {
        let (mut added, mut removed, changed) = {
            let mut pending = self.pending.borrow_mut();
            (pending.added.drain(), pending.removed.drain(), pending.changed.drain())
        };

        for entity in changed.into_iter().rev() {
            let id = entity.id().to_owned();
            let same_entity = self.updaters.get(&id).is_some_and(|u| Rc::ptr_eq(&u.borrow().entity(), &entity));
            if same_entity {
                if let Some(updater) = self.updaters.get(&id).cloned() {
                    self.remove_updater(&*updater.borrow());
                    self.insert_updater_into_batch(time, updater)?;
                }
            } else {
                removed.push(Rc::clone(&entity));
                added.push(entity);
            }
        }

        for entity in removed.into_iter().rev() {
            let id = entity.id();
            if let Some(updater) = self.updaters.remove(id) {
                self.remove_updater(&*updater.borrow());
                updater.borrow_mut().destroy();
            }
            self.subscriptions.remove(id);
        }

        for entity in added.into_iter().rev() {
            let id = entity.id().to_owned();
            let updater: GeometryUpdaterRef =
                Rc::new(RefCell::new(PolylineGeometryUpdater::new(Rc::clone(&entity), Rc::clone(&self.scene))));
            let pending_cb = Rc::clone(&self.pending);
            let sub = updater.borrow().geometry_changed().add_listener(move |ev: &GeometryChangedEvent| {
                pending_cb.borrow_mut().on_geometry_changed(&ev.entity);
            });
            self.updaters.insert(id.clone(), Rc::clone(&updater));
            self.subscriptions.insert(id, sub);
            self.insert_updater_into_batch(time, updater)?;
        }

        let mut is_updated = true;
        for b in &mut self.color_batches {
            is_updated = b.update(time)? && is_updated;
        }
        for b in &mut self.material_batches {
            is_updated = b.update(time)? && is_updated;
        }
        is_updated = self.dynamic_batch.update(time)? && is_updated;
        for b in &mut self.ground_batches {
            is_updated = b.update(time)? && is_updated;
        }

        Ok(is_updated)
    }

    fn get_bounding_sphere(&self, entity: &EntityRef, result: &mut BoundingSphere) -> BoundingSphereState {
        let Some(updater_rc) = self.updaters.get(entity.id()) else {
            return BoundingSphereState::Failed;
        };
        let updater = updater_rc.borrow();

        let mut spheres = Vec::new();
        let mut tmp = BoundingSphere::default();

        for b in &self.color_batches {
            match b.get_bounding_sphere(&*updater, &mut tmp) {
                BoundingSphereState::Pending => return BoundingSphereState::Pending,
                BoundingSphereState::Done => spheres.push(tmp),
                BoundingSphereState::Failed => {}
            }
        }
        for b in &self.material_batches {
            match b.get_bounding_sphere(&*updater, &mut tmp) {
                BoundingSphereState::Pending => return BoundingSphereState::Pending,
                BoundingSphereState::Done => spheres.push(tmp),
                BoundingSphereState::Failed => {}
            }
        }
        match self.dynamic_batch.get_bounding_sphere(&*updater, &mut tmp) {
            BoundingSphereState::Pending => return BoundingSphereState::Pending,
            BoundingSphereState::Done => spheres.push(tmp),
            BoundingSphereState::Failed => {}
        }
        for b in &self.ground_batches {
            match b.get_bounding_sphere(&*updater, &mut tmp) {
                BoundingSphereState::Pending => return BoundingSphereState::Pending,
                BoundingSphereState::Done => spheres.push(tmp),
                BoundingSphereState::Failed => {}
            }
        }

        if spheres.is_empty() {
            return BoundingSphereState::Failed;
        }

        *result = BoundingSphere::from_bounding_spheres(&spheres);
        BoundingSphereState::Done
    }

    fn is_destroyed(&self) -> bool {
        self.destroyed
    }

    fn destroy(&mut self) {
        if !self.destroyed {
            self.collection_subscription = None;
            self.pending.borrow_mut().clear();

            for b in &mut self.color_batches {
                b.remove_all_primitives();
            }
            for b in &mut self.material_batches {
                b.remove_all_primitives();
            }
            self.dynamic_batch.remove_all_primitives();
            for b in &mut self.ground_batches {
                b.remove_all_primitives();
            }

            self.subscriptions.clear();
            for (_, updater) in self.updaters.drain() {
                updater.borrow_mut().destroy();
            }
            self.destroyed = true;
        }
    }
}
