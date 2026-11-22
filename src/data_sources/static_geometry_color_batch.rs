//! CesiumJS `StaticGeometryColorBatch`: batches static per-instance-colored
//! geometries into solid and translucent [`GeometryBatchPrimitive`]s grouped by
//! depth-fail material.
use super::{
    geometry_updater::{
        bounding_sphere_from_primitive_attr, ddc_equals, eval_ddc_or_default, eval_terrain_offset_or_zero,
        instance_alpha, instance_set_show_value, is_color_material_component, GeometryAppearanceKind,
        GeometryBatchPrimitive, GeometryUpdater, GeometryUpdaterRef,
    },
    visualizer_values::*,
    BoundingSphereState, ColorMaterialProperty, ComponentRef, EntityChange, MaterialProperty, Property, Subscription,
};
use crate::{
    core::{
        BoundingSphere, Color, ColorGeometryInstanceAttribute, DistanceDisplayConditionGeometryInstanceAttribute,
        GeometryInstance, JulianDate, OffsetGeometryInstanceAttribute, RuntimeError, ShowGeometryInstanceAttribute,
    },
    scene::{Material, PrimitiveCollection, PrimitiveHandle, ScenePrimitive, ShadowMode},
};
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    rc::Rc,
};

struct ColorBatch {
    translucent: bool,
    appearance_kind: GeometryAppearanceKind,
    depth_fail_appearance_kind: Option<GeometryAppearanceKind>,
    depth_fail_material_property: Option<ComponentRef>,
    depth_fail_material: Option<Material>,
    closed: bool,
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
    subscriptions: HashMap<String, Subscription>,
    shows_updated: Rc<RefCell<Vec<String>>>,
    items_to_remove: Vec<GeometryUpdaterRef>,
    invalidated: Rc<Cell<bool>>,
    _remove_material_subscription: Option<Subscription>,
}

impl ColorBatch {
    fn new(
        primitives: Rc<RefCell<PrimitiveCollection>>,
        translucent: bool,
        appearance_kind: GeometryAppearanceKind,
        depth_fail_appearance_kind: Option<GeometryAppearanceKind>,
        depth_fail_material_property: Option<ComponentRef>,
        closed: bool,
        shadows: ShadowMode,
    ) -> Self {
        let invalidated = Rc::new(Cell::new(false));
        let remove_material_subscription = depth_fail_material_property.as_ref().map(|prop| {
            let inv = Rc::clone(&invalidated);
            prop.definition_changed().add_listener(move |_| {
                inv.set(true);
            })
        });
        Self {
            translucent,
            appearance_kind,
            depth_fail_appearance_kind,
            depth_fail_material_property,
            depth_fail_material: None,
            closed,
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
            subscriptions: HashMap::new(),
            shows_updated: Rc::new(RefCell::new(Vec::new())),
            items_to_remove: Vec::new(),
            invalidated,
            _remove_material_subscription: remove_material_subscription,
        }
    }

    fn is_material(&self, updater: &dyn GeometryUpdater) -> bool {
        let updater_material = updater.depth_fail_material_property();
        match (&self.depth_fail_material_property, &updater_material) {
            (None, None) => true,
            (Some(a), Some(b)) => a.identity() == b.identity() || a.equals(&**b).unwrap_or(false),
            _ => false,
        }
    }

    fn add(&mut self, updater: GeometryUpdaterRef, instance: GeometryInstance) {
        let u = updater.borrow();
        let id = u.id().to_owned();
        let has_dynamic_attrs = !u.has_constant_fill()
            || !u.fill_material_property().is_none_or(|m| m.is_constant())
            || u.depth_fail_material_property().is_some_and(|m| is_color_material_component(&m) && !m.is_constant())
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

                let depth_fail_appearance = if let Some(df_kind) = self.depth_fail_appearance_kind {
                    if let Some(df_prop) = &self.depth_fail_material_property {
                        let mat =
                            MaterialProperty::get_value(Some(time), Some(&**df_prop), self.depth_fail_material.take())?;
                        self.depth_fail_material = Some(mat);
                    }
                    Some(df_kind.build(self.translucent, self.closed, false, None, self.depth_fail_material.clone()))
                } else {
                    None
                };

                let appearance = self.appearance_kind.build(self.translucent, self.closed, false, None, None);
                let prim = Rc::new(RefCell::new(GeometryBatchPrimitive::new(
                    instances,
                    Some(appearance),
                    depth_fail_appearance,
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
            let ready = prim_rc.borrow().is_ready();
            if ready {
                prim_rc.borrow_mut().show = true;
                if let Some((_, old_handle)) = self.old_primitive.take() {
                    let _ = self.primitives.borrow_mut().remove(&old_handle);
                }

                if self.depth_fail_appearance_kind.is_some()
                    && !self.depth_fail_material_property.as_ref().is_some_and(is_color_material_component)
                {
                    if let Some(df_prop) = &self.depth_fail_material_property {
                        let mat =
                            MaterialProperty::get_value(Some(time), Some(&**df_prop), self.depth_fail_material.take())?;
                        prim_rc.borrow_mut().set_depth_fail_material(mat.clone());
                        self.depth_fail_material = Some(mat);
                    }
                }

                let waiting_on_create = self.waiting_on_create;
                let mut prim = prim_rc.borrow_mut();
                for id in &self.updaters_with_attributes {
                    let Some(updater_rc) = self.updaters.get(id) else { continue };
                    let updater = updater_rc.borrow();
                    let entity_id = updater.entity().id().to_owned();
                    let Some(attr) = prim.get_attributes_mut(&entity_id) else { continue };

                    if let Some(fill_mat) = updater.fill_material_property() {
                        if !fill_mat.is_constant() || waiting_on_create {
                            if let Some(color_mat) = fill_mat.as_any().downcast_ref::<ColorMaterialProperty>() {
                                let result_color = eval_color(color_mat.color().as_ref(), time, Color::WHITE)?;
                                if attr.last_color != Some(result_color) {
                                    attr.last_color = Some(result_color);
                                    let bytes = ColorGeometryInstanceAttribute::to_value(&result_color);
                                    attr.color = Some(bytes);
                                    if (self.translucent && bytes[3] == 255) || (!self.translucent && bytes[3] != 255) {
                                        self.items_to_remove.push(Rc::clone(updater_rc));
                                    }
                                }
                            }
                        }
                    }

                    if self.depth_fail_appearance_kind.is_some() {
                        if let Some(df_mat) = updater.depth_fail_material_property() {
                            if (!df_mat.is_constant() || waiting_on_create) && is_color_material_component(&df_mat) {
                                if let Some(color_mat) = df_mat.as_any().downcast_ref::<ColorMaterialProperty>() {
                                    let depth_color = eval_color(color_mat.color().as_ref(), time, Color::WHITE)?;
                                    if attr.last_depth_fail_color != Some(depth_color) {
                                        attr.last_depth_fail_color = Some(depth_color);
                                        attr.depth_fail_color =
                                            Some(ColorGeometryInstanceAttribute::to_value(&depth_color));
                                    }
                                }
                            }
                        }
                    }

                    let show =
                        updater.entity().is_showing() && (updater.has_constant_fill() || updater.is_filled(time)?);
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

    fn destroy(&mut self) {
        if let Some((_, handle)) = self.primitive.take() {
            let _ = self.primitives.borrow_mut().remove(&handle);
        }
        if let Some((_, handle)) = self.old_primitive.take() {
            let _ = self.primitives.borrow_mut().remove(&handle);
        }
        self._remove_material_subscription = None;
        self.subscriptions.clear();
        self.shows_updated.borrow_mut().clear();
    }
}

/// Port of CesiumJS `StaticGeometryColorBatch`.
pub struct StaticGeometryColorBatch {
    solid_items: Vec<ColorBatch>,
    translucent_items: Vec<ColorBatch>,
    primitives: Rc<RefCell<PrimitiveCollection>>,
    appearance_kind: GeometryAppearanceKind,
    depth_fail_appearance_kind: Option<GeometryAppearanceKind>,
    closed: bool,
    shadows: ShadowMode,
}

impl StaticGeometryColorBatch {
    pub fn new(
        primitives: Rc<RefCell<PrimitiveCollection>>,
        appearance_kind: GeometryAppearanceKind,
        depth_fail_appearance_kind: Option<GeometryAppearanceKind>,
        closed: bool,
        shadows: ShadowMode,
    ) -> Self {
        Self {
            solid_items: Vec::new(),
            translucent_items: Vec::new(),
            primitives,
            appearance_kind,
            depth_fail_appearance_kind,
            closed,
            shadows,
        }
    }

    pub fn solid_batch_count(&self) -> usize {
        self.solid_items.len()
    }

    pub fn translucent_batch_count(&self) -> usize {
        self.translucent_items.len()
    }

    pub fn add(&mut self, time: &JulianDate, updater: GeometryUpdaterRef) -> Result<(), RuntimeError> {
        let instance = updater.borrow().create_fill_geometry_instance(time)?;
        let translucent = instance_alpha(&instance) != 255;
        let items = if translucent { &mut self.translucent_items } else { &mut self.solid_items };

        for item in items.iter_mut() {
            if item.is_material(&*updater.borrow()) {
                item.add(updater, instance);
                return Ok(());
            }
        }

        let df_prop = updater.borrow().depth_fail_material_property();
        let mut batch = ColorBatch::new(
            Rc::clone(&self.primitives),
            translucent,
            self.appearance_kind,
            self.depth_fail_appearance_kind,
            df_prop,
            self.closed,
            self.shadows,
        );
        batch.add(updater, instance);
        items.push(batch);
        Ok(())
    }

    fn remove_from_list(items: &mut Vec<ColorBatch>, updater_id: &str) -> bool {
        for i in (0..items.len()).rev() {
            if items[i].remove(updater_id) {
                if items[i].updaters.is_empty() {
                    let mut removed = items.remove(i);
                    removed.destroy();
                }
                return true;
            }
        }
        false
    }

    pub fn remove(&mut self, updater: &dyn GeometryUpdater) {
        let id = updater.id();
        if !Self::remove_from_list(&mut self.solid_items, id) {
            Self::remove_from_list(&mut self.translucent_items, id);
        }
    }

    fn drain_invalidated(items: &mut Vec<ColorBatch>) -> Vec<GeometryUpdaterRef> {
        let mut readd = Vec::new();
        for i in (0..items.len()).rev() {
            if items[i].invalidated.get() {
                let mut item = items.remove(i);
                for id in &item.order {
                    if let Some(u) = item.updaters.get(id) {
                        readd.push(Rc::clone(u));
                    }
                }
                item.destroy();
            }
        }
        readd
    }

    fn update_item_list(
        items: &mut [ColorBatch],
        time: &JulianDate,
        mut is_updated: bool,
    ) -> Result<bool, RuntimeError> {
        for item in items.iter_mut() {
            is_updated = item.update(time)? && is_updated;
        }
        Ok(is_updated)
    }

    fn collect_moves(items: &mut [ColorBatch]) -> Vec<GeometryUpdaterRef> {
        let mut moved = Vec::new();
        for item in items.iter_mut() {
            if !item.items_to_remove.is_empty() {
                let to_move: Vec<GeometryUpdaterRef> = std::mem::take(&mut item.items_to_remove);
                for updater in to_move {
                    let id = updater.borrow().id().to_owned();
                    item.remove(&id);
                    moved.push(updater);
                }
            }
        }
        moved
    }

    pub fn update(&mut self, time: &JulianDate) -> Result<bool, RuntimeError> {
        let solid_readd = Self::drain_invalidated(&mut self.solid_items);
        for u in solid_readd {
            self.add(time, u)?;
        }
        let mut is_updated = Self::update_item_list(&mut self.solid_items, time, true)?;

        let translucent_readd = Self::drain_invalidated(&mut self.translucent_items);
        for u in translucent_readd {
            self.add(time, u)?;
        }
        is_updated = Self::update_item_list(&mut self.translucent_items, time, is_updated)? && is_updated;

        let mut moved = Self::collect_moves(&mut self.solid_items);
        moved.extend(Self::collect_moves(&mut self.translucent_items));
        let any_moved = !moved.is_empty();
        for u in moved {
            self.add(time, u)?;
        }

        if any_moved {
            is_updated = Self::update_item_list(&mut self.solid_items, time, is_updated)? && is_updated;
            is_updated = Self::update_item_list(&mut self.translucent_items, time, is_updated)? && is_updated;
        }

        Ok(is_updated)
    }

    pub fn get_bounding_sphere(
        &self,
        updater: &dyn GeometryUpdater,
        result: &mut BoundingSphere,
    ) -> BoundingSphereState {
        let id = updater.id();
        for item in &self.solid_items {
            if item.contains(id) {
                return item.get_bounding_sphere(updater, result);
            }
        }
        for item in &self.translucent_items {
            if item.contains(id) {
                return item.get_bounding_sphere(updater, result);
            }
        }
        BoundingSphereState::Failed
    }

    pub fn mark_all_ready(&self) {
        for item in &self.solid_items {
            item.mark_ready();
        }
        for item in &self.translucent_items {
            item.mark_ready();
        }
    }

    pub fn remove_all_primitives(&mut self) {
        for mut item in self.solid_items.drain(..) {
            item.destroy();
        }
        for mut item in self.translucent_items.drain(..) {
            item.destroy();
        }
    }
}
