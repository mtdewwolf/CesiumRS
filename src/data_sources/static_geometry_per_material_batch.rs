//! CesiumJS `StaticGeometryPerMaterialBatch`: batches static material-shaded
//! geometries by matching fill material and depth-fail material properties.
use super::{
    geometry_updater::{
        bounding_sphere_from_primitive_attr, ddc_equals, eval_ddc_or_default, eval_terrain_offset_or_zero,
        instance_set_show_value, is_color_material_component, GeometryAppearanceKind, GeometryBatchPrimitive,
        GeometryUpdater, GeometryUpdaterRef,
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

struct MaterialBatch {
    primitives: Rc<RefCell<PrimitiveCollection>>,
    appearance_kind: GeometryAppearanceKind,
    material_property: ComponentRef,
    depth_fail_appearance_kind: Option<GeometryAppearanceKind>,
    depth_fail_material_property: Option<ComponentRef>,
    closed: bool,
    shadows: ShadowMode,
    order: Vec<String>,
    updaters: HashMap<String, GeometryUpdaterRef>,
    geometry: HashMap<String, GeometryInstance>,
    create_primitive: bool,
    primitive: Option<(Rc<RefCell<GeometryBatchPrimitive>>, PrimitiveHandle)>,
    old_primitive: Option<(Rc<RefCell<GeometryBatchPrimitive>>, PrimitiveHandle)>,
    material: Option<Material>,
    depth_fail_material: Option<Material>,
    updaters_with_attributes: Vec<String>,
    invalidated: Rc<Cell<bool>>,
    _remove_material_subscription: Subscription,
    subscriptions: HashMap<String, Subscription>,
    shows_updated: Rc<RefCell<Vec<String>>>,
}

impl MaterialBatch {
    fn new(
        primitives: Rc<RefCell<PrimitiveCollection>>,
        appearance_kind: GeometryAppearanceKind,
        material_property: ComponentRef,
        depth_fail_appearance_kind: Option<GeometryAppearanceKind>,
        depth_fail_material_property: Option<ComponentRef>,
        closed: bool,
        shadows: ShadowMode,
    ) -> Self {
        let invalidated = Rc::new(Cell::new(false));
        let inv = Rc::clone(&invalidated);
        let remove_material_subscription = material_property.definition_changed().add_listener(move |_| {
            inv.set(true);
        });
        Self {
            primitives,
            appearance_kind,
            material_property,
            depth_fail_appearance_kind,
            depth_fail_material_property,
            closed,
            shadows,
            order: Vec::new(),
            updaters: HashMap::new(),
            geometry: HashMap::new(),
            create_primitive: true,
            primitive: None,
            old_primitive: None,
            material: None,
            depth_fail_material: None,
            updaters_with_attributes: Vec::new(),
            invalidated,
            _remove_material_subscription: remove_material_subscription,
            subscriptions: HashMap::new(),
            shows_updated: Rc::new(RefCell::new(Vec::new())),
        }
    }

    fn is_material(&self, updater: &dyn GeometryUpdater) -> bool {
        let Some(updater_material) = updater.fill_material_property() else {
            return false;
        };
        let updater_df = updater.depth_fail_material_property();

        let mat_same = self.material_property.identity() == updater_material.identity();
        let df_same = match (&self.depth_fail_material_property, &updater_df) {
            (None, None) => true,
            (Some(a), Some(b)) => a.identity() == b.identity(),
            _ => false,
        };
        if mat_same && df_same {
            return true;
        }

        let mat_eq = mat_same || self.material_property.equals(&*updater_material).unwrap_or(false);
        let df_eq = match (&self.depth_fail_material_property, &updater_df) {
            (None, None) => true,
            (Some(a), Some(b)) => a.identity() == b.identity() || a.equals(&**b).unwrap_or(false),
            _ => false,
        };
        mat_eq && df_eq
    }

    fn add(&mut self, time: &JulianDate, updater: GeometryUpdaterRef) -> Result<(), RuntimeError> {
        let instance = updater.borrow().create_fill_geometry_instance(time)?;
        let u = updater.borrow();
        let id = u.id().to_owned();
        let has_dynamic_attrs = !u.has_constant_fill()
            || !u.fill_material_property().is_none_or(|m| m.is_constant())
            || u.depth_fail_material_property().is_some_and(|m| is_color_material_component(&m) && !m.is_constant())
            || !is_slot_constant(u.distance_display_condition_property().as_ref())
            || u.terrain_offset_property().is_some_and(|p| !p.is_constant());
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
        Ok(())
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

                let mat =
                    MaterialProperty::get_value(Some(time), Some(&*self.material_property), self.material.take())?;
                let translucent = mat.is_translucent();
                self.material = Some(mat.clone());

                let depth_fail_appearance = match (&self.depth_fail_appearance_kind, &self.depth_fail_material_property)
                {
                    (Some(df_kind), Some(df_prop)) => {
                        let df_mat =
                            MaterialProperty::get_value(Some(time), Some(&**df_prop), self.depth_fail_material.take())?;
                        let df_translucent = df_mat.is_translucent();
                        self.depth_fail_material = Some(df_mat.clone());
                        Some(df_kind.build(df_translucent, self.closed, false, None, Some(df_mat)))
                    }
                    _ => None,
                };

                let appearance = self.appearance_kind.build(translucent, self.closed, false, None, Some(mat));
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
        } else if let Some((prim_rc, _)) = &self.primitive {
            if prim_rc.borrow().is_ready() {
                prim_rc.borrow_mut().show = true;
                if let Some((_, old_handle)) = self.old_primitive.take() {
                    let _ = self.primitives.borrow_mut().remove(&old_handle);
                }

                let mat =
                    MaterialProperty::get_value(Some(time), Some(&*self.material_property), self.material.take())?;
                prim_rc.borrow_mut().set_appearance_material(mat.clone());
                self.material = Some(mat);

                if self.depth_fail_appearance_kind.is_some()
                    && !self.depth_fail_material_property.as_ref().is_some_and(is_color_material_component)
                {
                    if let Some(df_prop) = &self.depth_fail_material_property {
                        let df_mat =
                            MaterialProperty::get_value(Some(time), Some(&**df_prop), self.depth_fail_material.take())?;
                        prim_rc.borrow_mut().set_depth_fail_material(df_mat.clone());
                        self.depth_fail_material = Some(df_mat);
                    }
                }

                let mut prim = prim_rc.borrow_mut();
                for id in &self.updaters_with_attributes {
                    let Some(updater_rc) = self.updaters.get(id) else { continue };
                    let updater = updater_rc.borrow();
                    let entity = updater.entity();
                    let Some(attr) = prim.get_attributes_mut(entity.id()) else { continue };

                    if self.depth_fail_appearance_kind.is_some()
                        && self.depth_fail_material_property.as_ref().is_some_and(is_color_material_component)
                    {
                        if let Some(df_mat) = updater.depth_fail_material_property() {
                            if !df_mat.is_constant() {
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
        self.subscriptions.clear();
        self.shows_updated.borrow_mut().clear();
    }
}

/// Port of CesiumJS `StaticGeometryPerMaterialBatch`.
pub struct StaticGeometryPerMaterialBatch {
    items: Vec<MaterialBatch>,
    primitives: Rc<RefCell<PrimitiveCollection>>,
    appearance_kind: GeometryAppearanceKind,
    depth_fail_appearance_kind: Option<GeometryAppearanceKind>,
    closed: bool,
    shadows: ShadowMode,
}

impl StaticGeometryPerMaterialBatch {
    pub fn new(
        primitives: Rc<RefCell<PrimitiveCollection>>,
        appearance_kind: GeometryAppearanceKind,
        depth_fail_appearance_kind: Option<GeometryAppearanceKind>,
        closed: bool,
        shadows: ShadowMode,
    ) -> Self {
        Self { items: Vec::new(), primitives, appearance_kind, depth_fail_appearance_kind, closed, shadows }
    }

    pub fn batch_count(&self) -> usize {
        self.items.len()
    }

    pub fn add(&mut self, time: &JulianDate, updater: GeometryUpdaterRef) -> Result<(), RuntimeError> {
        for item in &mut self.items {
            if item.is_material(&*updater.borrow()) {
                return item.add(time, updater);
            }
        }

        let fill_mat = updater
            .borrow()
            .fill_material_property()
            .ok_or_else(|| RuntimeError::new("updater has no fill material property"))?;
        let df_mat = updater.borrow().depth_fail_material_property();
        let mut batch = MaterialBatch::new(
            Rc::clone(&self.primitives),
            self.appearance_kind,
            fill_mat,
            self.depth_fail_appearance_kind,
            df_mat,
            self.closed,
            self.shadows,
        );
        batch.add(time, updater)?;
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
