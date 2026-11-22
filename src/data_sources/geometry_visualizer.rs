//! CesiumJS `GeometryVisualizer`: routes entity geometry updaters into static
//! color, material, outline, ground, and dynamic geometry batches.
use super::{
    geometry_updater::{
        eval_classification_type, is_color_material_component, GeometryAppearanceKind, GeometryChangedEvent,
        GeometryScene, GeometryUpdater, GeometryUpdaterRef,
    },
    visualizer_values::*,
    BoundingSphereState, CollectionChange, DynamicGeometryBatch, EntityCollection, EntityRef,
    GeometryUpdaterRegistration, GeometryUpdaterSet, StaticGeometryColorBatch, StaticGeometryPerMaterialBatch,
    StaticGroundGeometryColorBatch, StaticGroundGeometryPerMaterialBatch, StaticOutlineGeometryBatch, Subscription,
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

/// Port of CesiumJS `GeometryVisualizer`.
pub struct GeometryVisualizer {
    scene: Rc<dyn GeometryScene>,
    primitives: Rc<RefCell<PrimitiveCollection>>,
    ground_primitives: Rc<RefCell<OrderedGroundPrimitiveCollection>>,
    pending: Rc<RefCell<PendingChanges>>,
    outline_batches: Vec<StaticOutlineGeometryBatch>,
    closed_color_batches: Vec<StaticGeometryColorBatch>,
    closed_material_batches: Vec<StaticGeometryPerMaterialBatch>,
    open_color_batches: Vec<StaticGeometryColorBatch>,
    open_material_batches: Vec<StaticGeometryPerMaterialBatch>,
    ground_color_batches: Vec<StaticGroundGeometryColorBatch>,
    ground_material_batches: Vec<StaticGroundGeometryPerMaterialBatch>,
    dynamic_batch: DynamicGeometryBatch,
    updater_sets: HashMap<String, GeometryUpdaterSet>,
    subscriptions: HashMap<String, Subscription>,
    collection_subscription: Option<Subscription>,
    destroyed: bool,
}

impl GeometryVisualizer {
    pub fn new(
        scene: Rc<dyn GeometryScene>,
        entity_collection: &EntityCollection,
        primitives: Option<Rc<RefCell<PrimitiveCollection>>>,
        ground_primitives: Option<Rc<RefCell<OrderedGroundPrimitiveCollection>>>,
    ) -> Self {
        let primitives = primitives.unwrap_or_else(|| scene.primitives());
        let ground_primitives = ground_primitives.unwrap_or_else(|| scene.ground_primitives());

        let n_shadows = ShadowMode::NUMBER_OF_SHADOW_MODES as usize;
        let mut outline_batches = Vec::with_capacity(n_shadows * 2);
        let mut closed_color_batches = Vec::with_capacity(n_shadows * 2);
        let mut closed_material_batches = Vec::with_capacity(n_shadows * 2);
        let mut open_color_batches = Vec::with_capacity(n_shadows * 2);
        let mut open_material_batches = Vec::with_capacity(n_shadows * 2);

        // Two passes: [0..4] without terrain offset, [4..8] with terrain offset.
        for _tier in 0..2 {
            for i in 0..n_shadows {
                let shadows = ShadowMode::from_value(i as i32).unwrap_or(ShadowMode::Disabled);
                outline_batches.push(StaticOutlineGeometryBatch::new(
                    Rc::clone(&primitives),
                    Rc::clone(&scene),
                    shadows,
                ));
                closed_color_batches.push(StaticGeometryColorBatch::new(
                    Rc::clone(&primitives),
                    GeometryAppearanceKind::PerInstanceColor,
                    None,
                    true,
                    shadows,
                ));
                closed_material_batches.push(StaticGeometryPerMaterialBatch::new(
                    Rc::clone(&primitives),
                    GeometryAppearanceKind::Material,
                    None,
                    true,
                    shadows,
                ));
                open_color_batches.push(StaticGeometryColorBatch::new(
                    Rc::clone(&primitives),
                    GeometryAppearanceKind::PerInstanceColor,
                    None,
                    false,
                    shadows,
                ));
                open_material_batches.push(StaticGeometryPerMaterialBatch::new(
                    Rc::clone(&primitives),
                    GeometryAppearanceKind::Material,
                    None,
                    false,
                    shadows,
                ));
            }
        }

        let n_class = ClassificationType::NUMBER_OF_CLASSIFICATION_TYPES as usize;
        let supports_materials = scene.supports_materials_for_entities_on_terrain();
        let mut ground_color_batches = Vec::with_capacity(n_class);
        let mut ground_material_batches = Vec::with_capacity(if supports_materials { n_class } else { 0 });
        for i in 0..n_class {
            let ct = ClassificationType::from_value(i as i32).unwrap_or(ClassificationType::Both);
            ground_color_batches.push(StaticGroundGeometryColorBatch::new(Rc::clone(&ground_primitives), ct));
            if supports_materials {
                ground_material_batches.push(StaticGroundGeometryPerMaterialBatch::new(
                    Rc::clone(&ground_primitives),
                    ct,
                    GeometryAppearanceKind::Material,
                ));
            }
        }

        let dynamic_batch = DynamicGeometryBatch::new(Rc::clone(&primitives), Rc::clone(&ground_primitives));

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
            outline_batches,
            closed_color_batches,
            closed_material_batches,
            open_color_batches,
            open_material_batches,
            ground_color_batches,
            ground_material_batches,
            dynamic_batch,
            updater_sets: HashMap::new(),
            subscriptions: HashMap::new(),
            collection_subscription: Some(collection_subscription),
            destroyed: false,
        }
    }

    pub fn register_updater(registration: GeometryUpdaterRegistration) {
        GeometryUpdaterSet::register_updater(registration);
    }

    pub fn unregister_updater(name: &str) {
        GeometryUpdaterSet::unregister_updater(name);
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

    pub fn updater_set_count(&self) -> usize {
        self.updater_sets.len()
    }

    pub fn total_batch_count(&self) -> usize {
        self.outline_batches.len()
            + self.closed_color_batches.len()
            + self.closed_material_batches.len()
            + self.open_color_batches.len()
            + self.open_material_batches.len()
            + self.ground_color_batches.len()
            + self.ground_material_batches.len()
            + 1
    }

    /// Marks all asynchronous batch primitives ready (for headless stepping in
    /// hosts/tests without a full GPU `FrameState` loop).
    pub fn mark_all_primitives_ready(&self) {
        for b in &self.outline_batches {
            b.mark_all_ready();
        }
        for b in &self.closed_color_batches {
            b.mark_all_ready();
        }
        for b in &self.closed_material_batches {
            b.mark_all_ready();
        }
        for b in &self.open_color_batches {
            b.mark_all_ready();
        }
        for b in &self.open_material_batches {
            b.mark_all_ready();
        }
        for b in &self.ground_color_batches {
            b.mark_all_ready();
        }
        for b in &self.ground_material_batches {
            b.mark_all_ready();
        }
    }

    fn remove_updater(&mut self, updater: &dyn GeometryUpdater) {
        for b in &mut self.outline_batches {
            b.remove(updater);
        }
        for b in &mut self.closed_color_batches {
            b.remove(updater);
        }
        for b in &mut self.closed_material_batches {
            b.remove(updater);
        }
        for b in &mut self.open_color_batches {
            b.remove(updater);
        }
        for b in &mut self.open_material_batches {
            b.remove(updater);
        }
        for b in &mut self.ground_color_batches {
            b.remove(updater);
        }
        for b in &mut self.ground_material_batches {
            b.remove(updater);
        }
        self.dynamic_batch.remove(updater);
    }

    fn insert_updater_into_batch(
        &mut self,
        time: &JulianDate,
        updater: GeometryUpdaterRef,
    ) -> Result<(), RuntimeError> {
        let (
            is_dynamic,
            outline_enabled,
            fill_enabled,
            on_terrain,
            is_closed,
            has_offset,
            is_color,
            shadows,
            class_idx,
        ) = {
            let u = updater.borrow();
            let is_dyn = u.is_dynamic();
            if is_dyn {
                return self.dynamic_batch.add(time, &*u);
            }
            let outline = u.outline_enabled();
            let fill = u.fill_enabled();
            let shadows = if outline || fill {
                eval_shadow_mode(u.shadows_property().as_ref(), time, ShadowMode::Disabled)?.value() as usize
            } else {
                0
            };
            let on_t = u.on_terrain();
            let cls = if fill && on_t {
                eval_classification_type(u.classification_type_property().as_ref(), time, ClassificationType::Both)?
                    .value() as usize
            } else {
                0
            };
            let has_off = u.terrain_offset_property().is_some();
            let is_col = u.fill_material_property().as_ref().is_some_and(is_color_material_component);
            (is_dyn, outline, fill, on_t, u.is_closed(), has_off, is_col, shadows, cls)
        };
        let _ = is_dynamic;

        let n_shadows = ShadowMode::NUMBER_OF_SHADOW_MODES as usize;
        let idx = if has_offset { n_shadows + shadows } else { shadows };

        if outline_enabled {
            self.outline_batches[idx].add(time, Rc::clone(&updater))?;
        }

        if fill_enabled {
            if on_terrain {
                if is_color {
                    self.ground_color_batches[class_idx].add(time, updater)?;
                } else if let Some(batch) = self.ground_material_batches.get_mut(class_idx) {
                    batch.add(time, updater)?;
                }
            } else if is_closed {
                if is_color {
                    self.closed_color_batches[idx].add(time, updater)?;
                } else {
                    self.closed_material_batches[idx].add(time, updater)?;
                }
            } else if is_color {
                self.open_color_batches[idx].add(time, updater)?;
            } else {
                self.open_material_batches[idx].add(time, updater)?;
            }
        }
        Ok(())
    }
}

impl Visualizer for GeometryVisualizer {
    fn update(&mut self, time: &JulianDate) -> Result<bool, RuntimeError> {
        let (mut added, mut removed, changed) = {
            let mut pending = self.pending.borrow_mut();
            (pending.added.drain(), pending.removed.drain(), pending.changed.drain())
        };

        for entity in changed.into_iter().rev() {
            let id = entity.id().to_owned();
            let same_entity = self.updater_sets.get(&id).is_some_and(|set| Rc::ptr_eq(set.entity(), &entity));
            if same_entity {
                let updaters: Vec<GeometryUpdaterRef> =
                    self.updater_sets.get(&id).map(|set| set.updaters().to_vec()).unwrap_or_default();
                for updater in updaters {
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
            if let Some(mut set) = self.updater_sets.remove(id) {
                let updaters: Vec<GeometryUpdaterRef> = set.updaters().to_vec();
                for updater in updaters {
                    self.remove_updater(&*updater.borrow());
                }
                set.destroy();
            }
            self.subscriptions.remove(id);
        }

        for entity in added.into_iter().rev() {
            let id = entity.id().to_owned();
            let set = GeometryUpdaterSet::new(Rc::clone(&entity), Rc::clone(&self.scene));
            let updaters: Vec<GeometryUpdaterRef> = set.updaters().to_vec();
            let pending_cb = Rc::clone(&self.pending);
            let sub = set.geometry_changed().add_listener(move |ev: &GeometryChangedEvent| {
                pending_cb.borrow_mut().on_geometry_changed(&ev.entity);
            });
            self.updater_sets.insert(id.clone(), set);
            self.subscriptions.insert(id, sub);
            for updater in updaters {
                self.insert_updater_into_batch(time, updater)?;
            }
        }

        let mut is_updated = true;
        for b in &mut self.outline_batches {
            is_updated = b.update(time)? && is_updated;
        }
        for b in &mut self.closed_color_batches {
            is_updated = b.update(time)? && is_updated;
        }
        for b in &mut self.closed_material_batches {
            is_updated = b.update(time)? && is_updated;
        }
        for b in &mut self.open_color_batches {
            is_updated = b.update(time)? && is_updated;
        }
        for b in &mut self.open_material_batches {
            is_updated = b.update(time)? && is_updated;
        }
        for b in &mut self.ground_color_batches {
            is_updated = b.update(time)? && is_updated;
        }
        for b in &mut self.ground_material_batches {
            is_updated = b.update(time)? && is_updated;
        }
        is_updated = self.dynamic_batch.update(time)? && is_updated;

        Ok(is_updated)
    }

    fn get_bounding_sphere(&self, entity: &EntityRef, result: &mut BoundingSphere) -> BoundingSphereState {
        let Some(set) = self.updater_sets.get(entity.id()) else {
            return BoundingSphereState::Failed;
        };

        let mut spheres = Vec::new();
        let mut tmp = BoundingSphere::default();

        for updater_rc in set.updaters() {
            let updater = updater_rc.borrow();

            macro_rules! check_batches {
                ($batches:expr) => {
                    for b in $batches {
                        match b.get_bounding_sphere(&*updater, &mut tmp) {
                            BoundingSphereState::Pending => return BoundingSphereState::Pending,
                            BoundingSphereState::Done => spheres.push(tmp),
                            BoundingSphereState::Failed => {}
                        }
                    }
                };
            }

            check_batches!(&self.outline_batches);
            check_batches!(&self.closed_color_batches);
            check_batches!(&self.closed_material_batches);
            check_batches!(&self.open_color_batches);
            check_batches!(&self.open_material_batches);
            check_batches!(&self.ground_color_batches);
            check_batches!(&self.ground_material_batches);
            match self.dynamic_batch.get_bounding_sphere(&*updater, &mut tmp) {
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

            for b in &mut self.outline_batches {
                b.remove_all_primitives();
            }
            for b in &mut self.closed_color_batches {
                b.remove_all_primitives();
            }
            for b in &mut self.closed_material_batches {
                b.remove_all_primitives();
            }
            for b in &mut self.open_color_batches {
                b.remove_all_primitives();
            }
            for b in &mut self.open_material_batches {
                b.remove_all_primitives();
            }
            for b in &mut self.ground_color_batches {
                b.remove_all_primitives();
            }
            for b in &mut self.ground_material_batches {
                b.remove_all_primitives();
            }
            self.dynamic_batch.remove_all_primitives();

            self.subscriptions.clear();
            for (_, mut set) in self.updater_sets.drain() {
                set.destroy();
            }
            self.destroyed = true;
        }
    }
}
