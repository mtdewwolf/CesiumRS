//! CesiumJS `StaticOutlineGeometryBatch`: batches static geometry outlines
//! into solid and translucent primitives bucketed by clamped line width.
use super::{
    geometry_updater::{
        bounding_sphere_from_primitive_attr, ddc_equals, eval_ddc_or_default, eval_terrain_offset_or_zero,
        instance_alpha, instance_set_show_value, GeometryAppearanceKind, GeometryBatchPrimitive, GeometryScene,
        GeometryUpdater, GeometryUpdaterRef,
    },
    visualizer_values::*,
    BoundingSphereState, EntityChange, Property, Subscription,
};
use crate::{
    core::{
        BoundingSphere, Color, ColorGeometryInstanceAttribute, DistanceDisplayConditionGeometryInstanceAttribute,
        GeometryInstance, JulianDate, OffsetGeometryInstanceAttribute, RuntimeError, ShowGeometryInstanceAttribute,
    },
    scene::{PrimitiveCollection, PrimitiveHandle, ScenePrimitive, ShadowMode},
};
use std::{cell::RefCell, collections::HashMap, rc::Rc};

struct OutlineBatch {
    translucent: bool,
    width: f64,
    shadows: ShadowMode,
    primitives: Rc<RefCell<PrimitiveCollection>>,
    create_primitive: bool,
    waiting_on_create: bool,
    primitive: Option<(Rc<RefCell<GeometryBatchPrimitive>>, PrimitiveHandle)>,
    old_primitive: Option<(Rc<RefCell<GeometryBatchPrimitive>>, PrimitiveHandle)>,
    order: Vec<String>,
    geometry: HashMap<String, GeometryInstance>,
    updaters: HashMap<String, GeometryUpdaterRef>,
    updaters_with_attributes: Vec<String>,
    items_to_remove: Vec<GeometryUpdaterRef>,
    subscriptions: HashMap<String, Subscription>,
    shows_updated: Rc<RefCell<Vec<String>>>,
}

impl OutlineBatch {
    fn new(primitives: Rc<RefCell<PrimitiveCollection>>, translucent: bool, width: f64, shadows: ShadowMode) -> Self {
        Self {
            translucent,
            width,
            shadows,
            primitives,
            create_primitive: false,
            waiting_on_create: false,
            primitive: None,
            old_primitive: None,
            order: Vec::new(),
            geometry: HashMap::new(),
            updaters: HashMap::new(),
            updaters_with_attributes: Vec::new(),
            items_to_remove: Vec::new(),
            subscriptions: HashMap::new(),
            shows_updated: Rc::new(RefCell::new(Vec::new())),
        }
    }

    fn add(&mut self, updater: GeometryUpdaterRef, instance: GeometryInstance) {
        let u = updater.borrow();
        let id = u.id().to_owned();
        let has_dynamic_attrs = !u.has_constant_outline()
            || !is_slot_constant(u.outline_color_property().as_ref())
            || !is_slot_constant(u.distance_display_condition_property().as_ref())
            || u.terrain_offset_property().is_some_and(|p| !p.is_constant());
        let entity = u.entity();
        drop(u);

        self.create_primitive = true;
        if !self.geometry.contains_key(&id) {
            self.order.push(id.clone());
        }
        self.geometry.insert(id.clone(), instance);
        self.updaters.insert(id.clone(), updater);
        self.updaters_with_attributes.retain(|k| k != &id);
        self.subscriptions.remove(&id);

        if has_dynamic_attrs {
            self.updaters_with_attributes.push(id);
        } else {
            let shows = Rc::clone(&self.shows_updated);
            let sub_id = id.clone();
            let sub = entity.definition_changed().add_listener(move |change: &EntityChange| {
                if change.property.name == "isShowing" {
                    let mut shows = shows.borrow_mut();
                    if !shows.contains(&sub_id) {
                        shows.push(sub_id.clone());
                    }
                }
            });
            self.subscriptions.insert(id, sub);
        }
    }

    fn remove(&mut self, updater_id: &str) -> bool {
        let removed_geom = self.geometry.remove(updater_id).is_some();
        self.create_primitive = removed_geom || self.create_primitive;
        if self.updaters.remove(updater_id).is_some() {
            self.order.retain(|k| k != updater_id);
            self.updaters_with_attributes.retain(|k| k != updater_id);
            self.subscriptions.remove(updater_id);
            self.shows_updated.borrow_mut().retain(|k| k != updater_id);
            return true;
        }
        false
    }

    fn update(&mut self, time: &JulianDate) -> Result<bool, RuntimeError> {
        let mut is_updated = true;
        self.items_to_remove.clear();

        if self.create_primitive {
            let instances: Vec<GeometryInstance> =
                self.order.iter().filter_map(|id| self.geometry.get(id).cloned()).collect();
            if !instances.is_empty() {
                if let Some(prev) = self.primitive.take() {
                    if self.old_primitive.is_none() {
                        self.old_primitive = Some(prev);
                    } else {
                        let _ = self.primitives.borrow_mut().remove(&prev.1);
                    }
                }

                let appearance = GeometryAppearanceKind::PerInstanceColor.build(
                    self.translucent,
                    false,
                    true,
                    Some(self.width),
                    None,
                );
                let prim = Rc::new(RefCell::new(GeometryBatchPrimitive::new(
                    instances,
                    Some(appearance),
                    None,
                    false,
                    true,
                    self.shadows,
                )));
                let handle = PrimitiveHandle::from_rc(Rc::clone(&prim) as Rc<RefCell<dyn ScenePrimitive>>);
                self.primitives.borrow_mut().add(handle.clone(), None)?;
                self.primitive = Some((prim, handle));
                is_updated = false;
            } else {
                if let Some((_, handle)) = self.primitive.take() {
                    let _ = self.primitives.borrow_mut().remove(&handle);
                }
                if let Some((_, handle)) = self.old_primitive.take() {
                    let _ = self.primitives.borrow_mut().remove(&handle);
                }
            }
            self.create_primitive = false;
            self.waiting_on_create = true;
        } else if let Some((prim_rc, _)) = &self.primitive {
            if prim_rc.borrow().is_ready() {
                prim_rc.borrow_mut().show = true;
                if let Some((_, old_handle)) = self.old_primitive.take() {
                    let _ = self.primitives.borrow_mut().remove(&old_handle);
                }

                let waiting_on_create = self.waiting_on_create;
                let mut prim = prim_rc.borrow_mut();
                for id in &self.updaters_with_attributes {
                    let Some(updater_rc) = self.updaters.get(id) else { continue };
                    let updater = updater_rc.borrow();
                    let entity = updater.entity();
                    let Some(attr) = prim.get_attributes_mut(entity.id()) else { continue };

                    let outline_color_prop = updater.outline_color_property();
                    if !is_slot_constant(outline_color_prop.as_ref()) || waiting_on_create {
                        let outline_color = eval_color(outline_color_prop.as_ref(), time, Color::WHITE)?;
                        if attr.last_color != Some(outline_color) {
                            attr.last_color = Some(outline_color);
                            let bytes = ColorGeometryInstanceAttribute::to_value(&outline_color);
                            attr.color = Some(bytes);
                            if (self.translucent && bytes[3] == 255) || (!self.translucent && bytes[3] != 255) {
                                self.items_to_remove.push(Rc::clone(updater_rc));
                            }
                        }
                    }

                    let show =
                        entity.is_showing() && (updater.has_constant_outline() || updater.is_outline_visible(time)?);
                    if show != (attr.show[0] == 1) {
                        attr.show = ShowGeometryInstanceAttribute::to_value(show);
                    }

                    let ddc_prop = updater.distance_display_condition_property();
                    if !is_slot_constant(ddc_prop.as_ref()) {
                        let ddc = eval_ddc_or_default(ddc_prop.as_ref(), time)?;
                        if !ddc_equals(attr.last_distance_display_condition, ddc) {
                            attr.last_distance_display_condition = Some(ddc);
                            attr.distance_display_condition =
                                Some(DistanceDisplayConditionGeometryInstanceAttribute::to_value(&ddc));
                        }
                    }

                    let offset_prop = updater.terrain_offset_property();
                    if offset_prop.as_ref().is_some_and(|p| !p.is_constant()) {
                        let offset = eval_terrain_offset_or_zero(offset_prop.as_ref(), time)?;
                        if attr.last_offset != Some(offset) {
                            attr.last_offset = Some(offset);
                            attr.offset = Some(OffsetGeometryInstanceAttribute::to_value(&offset));
                        }
                    }
                }
                drop(prim);
                self.update_shows();
                self.waiting_on_create = false;
            } else {
                is_updated = false;
            }
        }

        Ok(is_updated)
    }

    fn update_shows(&mut self) {
        let Some((prim_rc, _)) = &self.primitive else {
            self.shows_updated.borrow_mut().clear();
            return;
        };
        let ids: Vec<String> = self.shows_updated.borrow_mut().drain(..).collect();
        let mut prim = prim_rc.borrow_mut();
        for id in ids {
            let Some(updater_rc) = self.updaters.get(&id) else { continue };
            let entity = updater_rc.borrow().entity();
            let show = entity.is_showing();
            if let Some(attr) = prim.get_attributes_mut(entity.id()) {
                if show != (attr.show[0] == 1) {
                    attr.show = ShowGeometryInstanceAttribute::to_value(show);
                    if let Some(inst) = self.geometry.get_mut(&id) {
                        instance_set_show_value(inst, attr.show[0]);
                    }
                }
            }
        }
    }

    fn contains(&self, updater_id: &str) -> bool {
        self.updaters.contains_key(updater_id)
    }

    fn get_bounding_sphere(&self, updater: &dyn GeometryUpdater, result: &mut BoundingSphere) -> BoundingSphereState {
        let Some((prim_rc, _)) = &self.primitive else {
            return BoundingSphereState::Pending;
        };
        bounding_sphere_from_primitive_attr(&prim_rc.borrow(), updater.entity().id(), true, result)
    }

    fn mark_ready(&self) {
        if let Some((prim, _)) = &self.primitive {
            prim.borrow_mut().mark_ready();
        }
    }

    fn remove_all_primitives(&mut self) {
        if let Some((_, handle)) = self.primitive.take() {
            let _ = self.primitives.borrow_mut().remove(&handle);
            self.geometry.clear();
            self.updaters.clear();
            self.order.clear();
        }
        if let Some((_, handle)) = self.old_primitive.take() {
            let _ = self.primitives.borrow_mut().remove(&handle);
        }
        self.subscriptions.clear();
        self.shows_updated.borrow_mut().clear();
    }
}

/// Port of CesiumJS `StaticOutlineGeometryBatch`.
pub struct StaticOutlineGeometryBatch {
    primitives: Rc<RefCell<PrimitiveCollection>>,
    scene: Rc<dyn GeometryScene>,
    shadows: ShadowMode,
    solid_order: Vec<u64>,
    solid_batches: HashMap<u64, OutlineBatch>,
    translucent_order: Vec<u64>,
    translucent_batches: HashMap<u64, OutlineBatch>,
}

impl StaticOutlineGeometryBatch {
    pub fn new(
        primitives: Rc<RefCell<PrimitiveCollection>>,
        scene: Rc<dyn GeometryScene>,
        shadows: ShadowMode,
    ) -> Self {
        Self {
            primitives,
            scene,
            shadows,
            solid_order: Vec::new(),
            solid_batches: HashMap::new(),
            translucent_order: Vec::new(),
            translucent_batches: HashMap::new(),
        }
    }

    pub fn solid_batch_count(&self) -> usize {
        self.solid_batches.len()
    }

    pub fn translucent_batch_count(&self) -> usize {
        self.translucent_batches.len()
    }

    pub fn add(&mut self, time: &JulianDate, updater: GeometryUpdaterRef) -> Result<(), RuntimeError> {
        let (instance, width) = {
            let u = updater.borrow();
            let inst = u.create_outline_geometry_instance(time)?;
            let w = self.scene.clamp_line_width(u.outline_width());
            (inst, w)
        };
        let key = width.to_bits();
        let translucent = instance_alpha(&instance) != 255;

        if !translucent {
            if !self.solid_batches.contains_key(&key) {
                self.solid_order.push(key);
                self.solid_batches
                    .insert(key, OutlineBatch::new(Rc::clone(&self.primitives), false, width, self.shadows));
            }
            self.solid_batches.get_mut(&key).unwrap().add(updater, instance);
        } else {
            if !self.translucent_batches.contains_key(&key) {
                self.translucent_order.push(key);
                self.translucent_batches
                    .insert(key, OutlineBatch::new(Rc::clone(&self.primitives), true, width, self.shadows));
            }
            self.translucent_batches.get_mut(&key).unwrap().add(updater, instance);
        }
        Ok(())
    }

    pub fn remove(&mut self, updater: &dyn GeometryUpdater) {
        let id = updater.id();
        for key in &self.solid_order {
            if let Some(batch) = self.solid_batches.get_mut(key) {
                if batch.remove(id) {
                    return;
                }
            }
        }
        for key in &self.translucent_order {
            if let Some(batch) = self.translucent_batches.get_mut(key) {
                if batch.remove(id) {
                    return;
                }
            }
        }
    }

    pub fn update(&mut self, time: &JulianDate) -> Result<bool, RuntimeError> {
        let mut is_updated = true;

        loop {
            let mut moved = Vec::new();
            let solid_keys = self.solid_order.clone();
            for key in solid_keys {
                if let Some(batch) = self.solid_batches.get_mut(&key) {
                    is_updated = batch.update(time)? && is_updated;
                    let to_move: Vec<GeometryUpdaterRef> = std::mem::take(&mut batch.items_to_remove);
                    for u in to_move {
                        let id = u.borrow().id().to_owned();
                        batch.remove(&id);
                        moved.push(u);
                    }
                }
            }
            let translucent_keys = self.translucent_order.clone();
            for key in translucent_keys {
                if let Some(batch) = self.translucent_batches.get_mut(&key) {
                    is_updated = batch.update(time)? && is_updated;
                    let to_move: Vec<GeometryUpdaterRef> = std::mem::take(&mut batch.items_to_remove);
                    for u in to_move {
                        let id = u.borrow().id().to_owned();
                        batch.remove(&id);
                        moved.push(u);
                    }
                }
            }
            if moved.is_empty() {
                break;
            }
            for u in moved {
                self.add(time, u)?;
            }
        }

        Ok(is_updated)
    }

    pub fn get_bounding_sphere(
        &self,
        updater: &dyn GeometryUpdater,
        result: &mut BoundingSphere,
    ) -> BoundingSphereState {
        let id = updater.id();
        for key in &self.solid_order {
            if let Some(batch) = self.solid_batches.get(key) {
                if batch.contains(id) {
                    return batch.get_bounding_sphere(updater, result);
                }
            }
        }
        for key in &self.translucent_order {
            if let Some(batch) = self.translucent_batches.get(key) {
                if batch.contains(id) {
                    return batch.get_bounding_sphere(updater, result);
                }
            }
        }
        BoundingSphereState::Failed
    }

    pub fn mark_all_ready(&self) {
        for batch in self.solid_batches.values() {
            batch.mark_ready();
        }
        for batch in self.translucent_batches.values() {
            batch.mark_ready();
        }
    }

    pub fn remove_all_primitives(&mut self) {
        for batch in self.solid_batches.values_mut() {
            batch.remove_all_primitives();
        }
        for batch in self.translucent_batches.values_mut() {
            batch.remove_all_primitives();
        }
    }
}
