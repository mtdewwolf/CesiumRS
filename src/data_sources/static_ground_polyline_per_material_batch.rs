//! CesiumJS `StaticGroundPolylinePerMaterialBatch`: batches ground-clamped
//! polylines by compatible material and `zIndex` into ground polyline
//! primitives inside an [`OrderedGroundPrimitiveCollection`].
use super::{
    geometry_updater::{
        bounding_sphere_from_ground_attr, ddc_equals, eval_ddc_or_default, instance_set_show_value,
        is_color_material_component, GeometryAppearanceKind, GeometryUpdater, GeometryUpdaterRef,
        GroundGeometryPrimitive, SharedGroundPrimitive,
    },
    visualizer_values::*,
    BoundingSphereState, ColorMaterialProperty, ComponentRef, EntityChange, MaterialProperty, Subscription,
};
use crate::{
    core::{
        BoundingSphere, Color, ColorGeometryInstanceAttribute, DistanceDisplayConditionGeometryInstanceAttribute,
        GeometryInstance, JulianDate, RuntimeError, ShowGeometryInstanceAttribute,
    },
    scene::{ClassificationType, Material, OrderedGroundPrimitiveCollection, ShadowMode},
};
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    rc::Rc,
};

struct GroundPolylineBatch {
    ordered_ground_primitives: Rc<RefCell<OrderedGroundPrimitiveCollection>>,
    classification_type: ClassificationType,
    appearance_kind: GeometryAppearanceKind,
    material_property: ComponentRef,
    order: Vec<String>,
    updaters: HashMap<String, GeometryUpdaterRef>,
    geometry: HashMap<String, GeometryInstance>,
    create_primitive: bool,
    primitive: Option<(Rc<RefCell<GroundGeometryPrimitive>>, u64)>,
    old_primitive: Option<(Rc<RefCell<GroundGeometryPrimitive>>, u64)>,
    material: Option<Material>,
    updaters_with_attributes: Vec<String>,
    invalidated: Rc<Cell<bool>>,
    _remove_material_subscription: Subscription,
    subscriptions: HashMap<String, Subscription>,
    shows_updated: Rc<RefCell<Vec<String>>>,
    z_index: f64,
    asynchronous: bool,
}

impl GroundPolylineBatch {
    fn new(
        ordered_ground_primitives: Rc<RefCell<OrderedGroundPrimitiveCollection>>,
        classification_type: ClassificationType,
        material_property: ComponentRef,
        z_index: f64,
        asynchronous: bool,
    ) -> Self {
        let appearance_kind = if is_color_material_component(&material_property) {
            GeometryAppearanceKind::PolylineColor
        } else {
            GeometryAppearanceKind::PolylineMaterial
        };
        let invalidated = Rc::new(Cell::new(false));
        let inv = Rc::clone(&invalidated);
        let remove_material_subscription = material_property.definition_changed().add_listener(move |_| {
            inv.set(true);
        });
        Self {
            ordered_ground_primitives,
            classification_type,
            appearance_kind,
            material_property,
            order: Vec::new(),
            updaters: HashMap::new(),
            geometry: HashMap::new(),
            create_primitive: true,
            primitive: None,
            old_primitive: None,
            material: None,
            updaters_with_attributes: Vec::new(),
            invalidated,
            _remove_material_subscription: remove_material_subscription,
            subscriptions: HashMap::new(),
            shows_updated: Rc::new(RefCell::new(Vec::new())),
            z_index,
            asynchronous,
        }
    }

    fn is_material(&self, updater: &dyn GeometryUpdater) -> bool {
        let Some(updater_material) = updater.fill_material_property() else {
            return false;
        };
        if updater_material.identity() == self.material_property.identity()
            || (is_color_material_component(&updater_material) && is_color_material_component(&self.material_property))
        {
            return true;
        }
        self.material_property.equals(&*updater_material).unwrap_or(false)
    }

    fn add(&mut self, updater: GeometryUpdaterRef, instance: GeometryInstance) {
        let u = updater.borrow();
        let id = u.id().to_owned();
        let has_dynamic_attrs = !u.has_constant_fill()
            || !u.fill_material_property().is_none_or(|m| m.is_constant())
            || !is_slot_constant(u.distance_display_condition_property().as_ref());
        let entity = u.entity();
        drop(u);

        if !self.geometry.contains_key(&id) {
            self.order.push(id.clone());
        }
        self.updaters.insert(id.clone(), updater);
        self.geometry.insert(id.clone(), instance);
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
        self.create_primitive = true;
    }

    fn remove(&mut self, updater_id: &str) -> bool {
        let removed_geom = self.geometry.remove(updater_id).is_some();
        self.create_primitive = removed_geom || self.create_primitive;
        if self.updaters.remove(updater_id).is_some() {
            self.order.retain(|k| k != updater_id);
            self.updaters_with_attributes.retain(|k| k != updater_id);
            self.subscriptions.remove(updater_id);
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
                        let _ = self.ordered_ground_primitives.borrow_mut().remove(prev.1, false);
                    }
                }

                let appearance = if self.appearance_kind == GeometryAppearanceKind::PolylineMaterial {
                    let mat =
                        MaterialProperty::get_value(Some(time), Some(&*self.material_property), self.material.take())?;
                    self.material = Some(mat.clone());
                    self.appearance_kind.build(true, false, false, None, Some(mat))
                } else {
                    self.appearance_kind.build(true, false, false, None, None)
                };

                let gp = Rc::new(RefCell::new(GroundGeometryPrimitive::new(
                    instances,
                    Some(appearance),
                    self.classification_type,
                    ShadowMode::Disabled,
                    false,
                    self.asynchronous,
                    true,
                )));
                let handle = self
                    .ordered_ground_primitives
                    .borrow_mut()
                    .add(Box::new(SharedGroundPrimitive(Rc::clone(&gp))), self.z_index)?;
                self.primitive = Some((gp, handle));
                is_updated = false;
            } else {
                if let Some((_, handle)) = self.primitive.take() {
                    let _ = self.ordered_ground_primitives.borrow_mut().remove(handle, false);
                }
                if let Some((_, handle)) = self.old_primitive.take() {
                    let _ = self.ordered_ground_primitives.borrow_mut().remove(handle, false);
                }
            }
            self.create_primitive = false;
        } else if let Some((prim_rc, _)) = &self.primitive {
            if prim_rc.borrow().is_ready() {
                prim_rc.borrow_mut().show = true;
                if let Some((_, old_handle)) = self.old_primitive.take() {
                    let _ = self.ordered_ground_primitives.borrow_mut().remove(old_handle, false);
                }

                if self.appearance_kind == GeometryAppearanceKind::PolylineMaterial {
                    let mat =
                        MaterialProperty::get_value(Some(time), Some(&*self.material_property), self.material.take())?;
                    prim_rc.borrow_mut().set_appearance_material(mat.clone());
                    self.material = Some(mat);
                }

                let mut prim = prim_rc.borrow_mut();
                for id in &self.updaters_with_attributes {
                    let Some(updater_rc) = self.updaters.get(id) else { continue };
                    let updater = updater_rc.borrow();
                    let entity = updater.entity();
                    let Some(attr) = prim.get_attributes_mut(entity.id()) else { continue };

                    if let Some(fill_mat) = updater.fill_material_property() {
                        if !fill_mat.is_constant() {
                            if let Some(color_mat) = fill_mat.as_any().downcast_ref::<ColorMaterialProperty>() {
                                let result_color = eval_color(color_mat.color().as_ref(), time, Color::WHITE)?;
                                if attr.last_color != Some(result_color) {
                                    attr.last_color = Some(result_color);
                                    attr.color = Some(ColorGeometryInstanceAttribute::to_value(&result_color));
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
        bounding_sphere_from_ground_attr(&prim_rc.borrow(), updater.entity().id(), true, result)
    }

    fn mark_ready(&self) {
        if let Some((prim, _)) = &self.primitive {
            prim.borrow_mut().mark_ready();
        }
    }

    fn destroy(&mut self) {
        if let Some((_, handle)) = self.primitive.take() {
            let _ = self.ordered_ground_primitives.borrow_mut().remove(handle, false);
        }
        if let Some((_, handle)) = self.old_primitive.take() {
            let _ = self.ordered_ground_primitives.borrow_mut().remove(handle, false);
        }
        self.subscriptions.clear();
        self.shows_updated.borrow_mut().clear();
    }
}

/// Port of CesiumJS `StaticGroundPolylinePerMaterialBatch`.
pub struct StaticGroundPolylinePerMaterialBatch {
    items: Vec<GroundPolylineBatch>,
    ordered_ground_primitives: Rc<RefCell<OrderedGroundPrimitiveCollection>>,
    classification_type: ClassificationType,
    asynchronous: bool,
}

impl StaticGroundPolylinePerMaterialBatch {
    pub fn new(
        ordered_ground_primitives: Rc<RefCell<OrderedGroundPrimitiveCollection>>,
        classification_type: ClassificationType,
        asynchronous: Option<bool>,
    ) -> Self {
        Self {
            items: Vec::new(),
            ordered_ground_primitives,
            classification_type,
            asynchronous: asynchronous.unwrap_or(true),
        }
    }

    pub fn batch_count(&self) -> usize {
        self.items.len()
    }

    pub fn add(&mut self, time: &JulianDate, updater: GeometryUpdaterRef) -> Result<(), RuntimeError> {
        let (instance, z_index, fill_mat) = {
            let u = updater.borrow();
            let inst = u.create_fill_geometry_instance(time)?;
            let z = eval_f64(u.z_index().as_ref(), time, 0.0)?;
            let mat =
                u.fill_material_property().ok_or_else(|| RuntimeError::new("updater has no fill material property"))?;
            (inst, z, mat)
        };

        for item in &mut self.items {
            if item.is_material(&*updater.borrow()) && item.z_index == z_index {
                item.add(updater, instance);
                return Ok(());
            }
        }

        let mut batch = GroundPolylineBatch::new(
            Rc::clone(&self.ordered_ground_primitives),
            self.classification_type,
            fill_mat,
            z_index,
            self.asynchronous,
        );
        batch.add(updater, instance);
        self.items.push(batch);
        Ok(())
    }

    pub fn remove(&mut self, updater: &dyn GeometryUpdater) {
        let id = updater.id();
        for i in (0..self.items.len()).rev() {
            if self.items[i].remove(id) {
                if self.items[i].updaters.is_empty() {
                    let mut removed = self.items.remove(i);
                    removed.destroy();
                }
                break;
            }
        }
    }

    pub fn update(&mut self, time: &JulianDate) -> Result<bool, RuntimeError> {
        let mut readd = Vec::new();
        for i in (0..self.items.len()).rev() {
            if self.items[i].invalidated.get() {
                let mut item = self.items.remove(i);
                for id in &item.order {
                    if let Some(u) = item.updaters.get(id) {
                        readd.push(Rc::clone(u));
                    }
                }
                item.destroy();
            }
        }
        for u in readd {
            self.add(time, u)?;
        }

        let mut is_updated = true;
        for item in &mut self.items {
            is_updated = item.update(time)? && is_updated;
        }
        Ok(is_updated)
    }

    pub fn get_bounding_sphere(
        &self,
        updater: &dyn GeometryUpdater,
        result: &mut BoundingSphere,
    ) -> BoundingSphereState {
        let id = updater.id();
        for item in &self.items {
            if item.contains(id) {
                return item.get_bounding_sphere(updater, result);
            }
        }
        BoundingSphereState::Failed
    }

    pub fn mark_all_ready(&self) {
        for item in &self.items {
            item.mark_ready();
        }
    }

    pub fn remove_all_primitives(&mut self) {
        for mut item in self.items.drain(..) {
            item.destroy();
        }
    }
}
