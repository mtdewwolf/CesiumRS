//! CesiumJS `StaticGroundGeometryColorBatch`: batches non-overlapping
//! color-shaded ground geometries by `zIndex` into
//! [`GroundGeometryPrimitive`]s inside an [`OrderedGroundPrimitiveCollection`].
use super::{
    geometry_updater::{
        bounding_sphere_from_ground_attr, ddc_equals, eval_ddc_or_default, instance_set_show_value, GeometryUpdater,
        GeometryUpdaterRef, GroundGeometryPrimitive, SharedGroundPrimitive,
    },
    visualizer_values::*,
    BoundingSphereState, ColorMaterialProperty, EntityChange, Subscription,
};
use crate::{
    core::{
        BoundingSphere, Color, ColorGeometryInstanceAttribute, DistanceDisplayConditionGeometryInstanceAttribute,
        GeometryInstance, JulianDate, Rectangle, RectangleCollisionChecker, RuntimeError,
        ShowGeometryInstanceAttribute,
    },
    scene::{ClassificationType, OrderedGroundPrimitiveCollection, ShadowMode},
};
use std::{cell::RefCell, collections::HashMap, rc::Rc};

struct GroundColorBatch {
    primitives: Rc<RefCell<OrderedGroundPrimitiveCollection>>,
    z_index: f64,
    classification_type: ClassificationType,
    create_primitive: bool,
    waiting_on_create: bool,
    primitive: Option<(Rc<RefCell<GroundGeometryPrimitive>>, u64)>,
    old_primitive: Option<(Rc<RefCell<GroundGeometryPrimitive>>, u64)>,
    order: Vec<String>,
    geometry: HashMap<String, GeometryInstance>,
    rectangles: HashMap<String, Rectangle>,
    updaters: HashMap<String, GeometryUpdaterRef>,
    updaters_with_attributes: Vec<String>,
    subscriptions: HashMap<String, Subscription>,
    shows_updated: Rc<RefCell<Vec<String>>>,
    is_dirty: bool,
    rectangle_collision_check: RectangleCollisionChecker,
}

impl GroundColorBatch {
    fn new(
        primitives: Rc<RefCell<OrderedGroundPrimitiveCollection>>,
        classification_type: ClassificationType,
        z_index: f64,
    ) -> Self {
        Self {
            primitives,
            z_index,
            classification_type,
            create_primitive: false,
            waiting_on_create: false,
            primitive: None,
            old_primitive: None,
            order: Vec::new(),
            geometry: HashMap::new(),
            rectangles: HashMap::new(),
            updaters: HashMap::new(),
            updaters_with_attributes: Vec::new(),
            subscriptions: HashMap::new(),
            shows_updated: Rc::new(RefCell::new(Vec::new())),
            is_dirty: false,
            rectangle_collision_check: RectangleCollisionChecker::new(),
        }
    }

    fn overlapping(&self, rectangle: &Rectangle) -> bool {
        self.rectangle_collision_check.collides(rectangle)
    }

    fn add(&mut self, updater: GeometryUpdaterRef, instance: GeometryInstance, rectangle: Option<Rectangle>) {
        let u = updater.borrow();
        let id = u.id().to_owned();
        let has_dynamic_attrs = !u.has_constant_fill()
            || !u.fill_material_property().is_none_or(|m| m.is_constant())
            || !is_slot_constant(u.distance_display_condition_property().as_ref());
        let entity = u.entity();
        drop(u);

        self.create_primitive = true;
        if !self.geometry.contains_key(&id) {
            self.order.push(id.clone());
        }
        self.geometry.insert(id.clone(), instance);
        self.updaters.insert(id.clone(), updater);
        if let Some(rect) = rectangle {
            self.rectangle_collision_check.insert(id.clone(), rect);
            self.rectangles.insert(id.clone(), rect);
        }
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
            if let Some(rect) = self.rectangles.remove(updater_id) {
                self.rectangle_collision_check.remove(updater_id, &rect);
            }
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

        if self.create_primitive {
            let instances: Vec<GeometryInstance> =
                self.order.iter().filter_map(|id| self.geometry.get(id).cloned()).collect();
            if !instances.is_empty() {
                if let Some(prev) = self.primitive.take() {
                    if self.old_primitive.is_none() {
                        self.old_primitive = Some(prev);
                    } else {
                        let _ = self.primitives.borrow_mut().remove(prev.1, false);
                    }
                }

                let gp = Rc::new(RefCell::new(GroundGeometryPrimitive::new(
                    instances,
                    None,
                    self.classification_type,
                    ShadowMode::Disabled,
                    false,
                    true,
                    false,
                )));
                let handle =
                    self.primitives.borrow_mut().add(Box::new(SharedGroundPrimitive(Rc::clone(&gp))), self.z_index)?;
                self.primitive = Some((gp, handle));
                is_updated = false;
            } else {
                if let Some((_, handle)) = self.primitive.take() {
                    let _ = self.primitives.borrow_mut().remove(handle, false);
                }
                if let Some((_, handle)) = self.old_primitive.take() {
                    let _ = self.primitives.borrow_mut().remove(handle, false);
                }
            }
            self.create_primitive = false;
            self.waiting_on_create = true;
        } else if let Some((prim_rc, _)) = &self.primitive {
            if prim_rc.borrow().is_ready() {
                prim_rc.borrow_mut().show = true;
                if let Some((_, old_handle)) = self.old_primitive.take() {
                    let _ = self.primitives.borrow_mut().remove(old_handle, false);
                }

                let waiting_on_create = self.waiting_on_create;
                let mut prim = prim_rc.borrow_mut();
                for id in &self.updaters_with_attributes {
                    let Some(updater_rc) = self.updaters.get(id) else { continue };
                    let updater = updater_rc.borrow();
                    let entity = updater.entity();
                    let Some(attr) = prim.get_attributes_mut(entity.id()) else { continue };

                    if let Some(fill_mat) = updater.fill_material_property() {
                        if !fill_mat.is_constant() || waiting_on_create {
                            if let Some(color_mat) = fill_mat.as_any().downcast_ref::<ColorMaterialProperty>() {
                                let fill_color = eval_color(color_mat.color().as_ref(), time, Color::WHITE)?;
                                if attr.last_color != Some(fill_color) {
                                    attr.last_color = Some(fill_color);
                                    attr.color = Some(ColorGeometryInstanceAttribute::to_value(&fill_color));
                                }
                            }
                        }
                    }

                    let show = entity.is_showing() && (updater.has_constant_fill() || updater.is_filled(time)?);
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
        bounding_sphere_from_ground_attr(&prim_rc.borrow(), updater.entity().id(), false, result)
    }

    fn mark_ready(&self) {
        if let Some((prim, _)) = &self.primitive {
            prim.borrow_mut().mark_ready();
        }
    }

    fn remove_all_primitives(&mut self) {
        if let Some((_, handle)) = self.primitive.take() {
            let _ = self.primitives.borrow_mut().remove(handle, false);
            self.geometry.clear();
            self.updaters.clear();
            self.order.clear();
            self.rectangles.clear();
        }
        if let Some((_, handle)) = self.old_primitive.take() {
            let _ = self.primitives.borrow_mut().remove(handle, false);
        }
        self.subscriptions.clear();
        self.shows_updated.borrow_mut().clear();
    }
}

/// Port of CesiumJS `StaticGroundGeometryColorBatch`.
pub struct StaticGroundGeometryColorBatch {
    batches: Vec<GroundColorBatch>,
    primitives: Rc<RefCell<OrderedGroundPrimitiveCollection>>,
    classification_type: ClassificationType,
}

impl StaticGroundGeometryColorBatch {
    pub fn new(
        primitives: Rc<RefCell<OrderedGroundPrimitiveCollection>>,
        classification_type: ClassificationType,
    ) -> Self {
        Self { batches: Vec::new(), primitives, classification_type }
    }

    pub fn batch_count(&self) -> usize {
        self.batches.len()
    }

    pub fn add(&mut self, time: &JulianDate, updater: GeometryUpdaterRef) -> Result<(), RuntimeError> {
        let (instance, rectangle, z_index) = {
            let u = updater.borrow();
            let inst = u.create_fill_geometry_instance(time)?;
            let rect = u.bounding_rectangle();
            let z = eval_f64(u.z_index().as_ref(), time, 0.0)?;
            (inst, rect, z)
        };

        for batch in &mut self.batches {
            let overlaps = rectangle.as_ref().is_some_and(|r| batch.overlapping(r));
            if batch.z_index == z_index && !overlaps {
                batch.add(updater, instance, rectangle);
                return Ok(());
            }
        }

        let mut batch = GroundColorBatch::new(Rc::clone(&self.primitives), self.classification_type, z_index);
        batch.add(updater, instance, rectangle);
        self.batches.push(batch);
        Ok(())
    }

    pub fn remove(&mut self, updater: &dyn GeometryUpdater) {
        let id = updater.id();
        for batch in &mut self.batches {
            if batch.remove(id) {
                return;
            }
        }
    }

    pub fn update(&mut self, time: &JulianDate) -> Result<bool, RuntimeError> {
        let mut is_updated = true;
        for batch in &mut self.batches {
            is_updated = batch.update(time)? && is_updated;
        }

        for i in (0..self.batches.len()).rev() {
            if self.batches[i].is_dirty {
                is_updated = self.batches[i].update(time)? && is_updated;
                self.batches[i].is_dirty = false;
            }
            if self.batches[i].geometry.is_empty() {
                let mut removed = self.batches.remove(i);
                removed.remove_all_primitives();
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
        for batch in &self.batches {
            if batch.contains(id) {
                return batch.get_bounding_sphere(updater, result);
            }
        }
        BoundingSphereState::Failed
    }

    pub fn mark_all_ready(&self) {
        for batch in &self.batches {
            batch.mark_ready();
        }
    }

    pub fn remove_all_primitives(&mut self) {
        for mut batch in self.batches.drain(..) {
            batch.remove_all_primitives();
        }
    }
}
