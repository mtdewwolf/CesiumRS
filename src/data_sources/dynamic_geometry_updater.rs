//! CesiumJS `DynamicGeometryUpdater`: per-frame rebuild and lifecycle for
//! time-varying geometry entities.
use super::{
    geometry_updater::{
        eval_classification_type, instance_alpha, is_color_material_component, GeometryAppearanceKind,
        GeometryBatchPrimitive, GroundGeometryPrimitive, SharedGroundPrimitive,
    },
    visualizer_values::*,
    BoundingSphereState, ComponentRef, Entity, EntityComponent, EntityRef, MaterialProperty, PropertyInput,
};
use crate::{
    core::{BoundingSphere, GeometryInstance, JulianDate, RuntimeError, VertexFormat},
    scene::{
        ClassificationType, Material, OrderedGroundPrimitiveCollection, PerInstanceColorAppearance,
        PrimitiveCollection, PrimitiveHandle, ShadowMode,
    },
};
use std::{cell::RefCell, rc::Rc};

/// Public contract of a dynamic geometry updater managed by
/// [`DynamicGeometryBatch`](super::DynamicGeometryBatch).
pub trait DynamicGeometryUpdater {
    fn update(&mut self, time: &JulianDate) -> Result<(), RuntimeError>;
    fn get_bounding_sphere(&self, result: &mut BoundingSphere) -> BoundingSphereState;
    fn is_destroyed(&self) -> bool;
    fn destroy(&mut self);
}

/// Callbacks supplied by concrete geometry updaters to drive the shared
/// [`StandardDynamicGeometryUpdater`] rebuild loop.
pub trait DynamicGeometryHooks {
    fn entity(&self) -> EntityRef;
    fn geometry_property_name(&self) -> &'static str;
    fn on_terrain(&self) -> bool;
    fn supports_materials_for_entities_on_terrain(&self) -> bool;
    fn shadows_property(&self) -> Option<PropertyInput>;
    fn classification_type_property(&self) -> Option<PropertyInput>;
    fn fill_material_property(&self) -> Option<ComponentRef>;
    fn z_index(&self) -> Option<PropertyInput>;
    fn clamp_line_width(&self, width: f64) -> f64;

    fn set_options(
        &mut self,
        entity: &Entity,
        geometry: &dyn EntityComponent,
        time: &JulianDate,
    ) -> Result<(), RuntimeError>;
    fn is_hidden(
        &self,
        entity: &Entity,
        geometry: &dyn EntityComponent,
        time: &JulianDate,
    ) -> Result<bool, RuntimeError>;
    fn get_is_closed(&self) -> bool;
    fn set_vertex_format(&mut self, vertex_format: VertexFormat);
    fn create_fill_geometry_instance(&self, time: &JulianDate) -> Result<GeometryInstance, RuntimeError>;
    fn create_outline_geometry_instance(&self, time: &JulianDate) -> Result<GeometryInstance, RuntimeError>;
}

/// Default `DynamicGeometryUpdater.prototype._isHidden` check.
pub(crate) fn dynamic_default_is_hidden(
    entity: &Entity,
    geometry: &dyn EntityComponent,
    time: &JulianDate,
) -> Result<bool, RuntimeError> {
    Ok(!entity.is_showing()
        || !entity.is_available(time)
        || !eval_bool(geometry.get_property("show").as_ref(), time, true)?)
}

/// Shared implementation of CesiumJS `DynamicGeometryUpdater.prototype.update`,
/// `getBoundingSphere`, and `destroy`.
pub struct StandardDynamicGeometryUpdater<H: DynamicGeometryHooks> {
    hooks: H,
    primitives: Rc<RefCell<PrimitiveCollection>>,
    ordered_ground_primitives: Rc<RefCell<OrderedGroundPrimitiveCollection>>,
    primitive: Option<(Rc<RefCell<GeometryBatchPrimitive>>, PrimitiveHandle)>,
    ground_primitive: Option<(Rc<RefCell<GroundGeometryPrimitive>>, u64)>,
    outline_primitive: Option<(Rc<RefCell<GeometryBatchPrimitive>>, PrimitiveHandle)>,
    material: Option<Material>,
    destroyed: bool,
}

impl<H: DynamicGeometryHooks> StandardDynamicGeometryUpdater<H> {
    pub fn new(
        hooks: H,
        primitives: Rc<RefCell<PrimitiveCollection>>,
        ordered_ground_primitives: Rc<RefCell<OrderedGroundPrimitiveCollection>>,
    ) -> Self {
        Self {
            hooks,
            primitives,
            ordered_ground_primitives,
            primitive: None,
            ground_primitive: None,
            outline_primitive: None,
            material: None,
            destroyed: false,
        }
    }

    fn remove_primitives(&mut self) {
        if let Some((_, handle)) = self.ground_primitive.take() {
            let _ = self.ordered_ground_primitives.borrow_mut().remove(handle, false);
        }
        if let Some((_, handle)) = self.primitive.take() {
            let _ = self.primitives.borrow_mut().remove_and_destroy(&handle);
        }
        if let Some((_, handle)) = self.outline_primitive.take() {
            let _ = self.primitives.borrow_mut().remove_and_destroy(&handle);
        }
    }
}

impl<H: DynamicGeometryHooks> DynamicGeometryUpdater for StandardDynamicGeometryUpdater<H> {
    fn update(&mut self, time: &JulianDate) -> Result<(), RuntimeError> {
        self.remove_primitives();

        let entity = self.hooks.entity();
        let geometry = match entity.get_property(self.hooks.geometry_property_name()) {
            Some(PropertyInput::Component(c)) => c,
            _ => return Ok(()),
        };

        self.hooks.set_options(&entity, &*geometry, time)?;
        if self.hooks.is_hidden(&entity, &*geometry, time)? {
            return Ok(());
        }

        let on_terrain = self.hooks.on_terrain();
        let shadows = eval_shadow_mode(self.hooks.shadows_property().as_ref(), time, ShadowMode::Disabled)?;

        let fill_slot = geometry.get_property("fill");
        let show_fill = match fill_slot.as_ref() {
            None => true,
            Some(slot) => eval_bool(Some(slot), time, true)?,
        };

        if show_fill {
            let fill_material = self.hooks.fill_material_property();
            let is_color_appearance = fill_material.as_ref().is_some_and(is_color_material_component);
            let closed = self.hooks.get_is_closed();

            if on_terrain {
                self.hooks.set_vertex_format(PerInstanceColorAppearance::VERTEX_FORMAT);
                let fill_instance = self.hooks.create_fill_geometry_instance(time)?;
                let appearance = if is_color_appearance {
                    let flat = !self.hooks.supports_materials_for_entities_on_terrain();
                    GeometryAppearanceKind::PerInstanceColor.build(true, closed, flat, None, None)
                } else {
                    let prev = self.material.take();
                    let mat = MaterialProperty::get_value(Some(time), fill_material.as_deref(), prev)?;
                    let translucent = mat.is_translucent();
                    self.material = Some(mat.clone());
                    GeometryAppearanceKind::Material.build(translucent, closed, false, None, Some(mat))
                };
                let classification = eval_classification_type(
                    self.hooks.classification_type_property().as_ref(),
                    time,
                    ClassificationType::Both,
                )?;
                let z_index = eval_f64_opt(self.hooks.z_index().as_ref(), time)?.unwrap_or(0.0);
                let gp = Rc::new(RefCell::new(GroundGeometryPrimitive::new(
                    vec![fill_instance],
                    Some(appearance),
                    classification,
                    shadows,
                    true,
                    false,
                    false,
                )));
                let handle = self
                    .ordered_ground_primitives
                    .borrow_mut()
                    .add(Box::new(SharedGroundPrimitive(Rc::clone(&gp))), z_index)?;
                self.ground_primitive = Some((gp, handle));
            } else {
                let (appearance, vertex_format) = if is_color_appearance {
                    let vf = PerInstanceColorAppearance::VERTEX_FORMAT;
                    (None, vf)
                } else {
                    let prev = self.material.take();
                    let mat = MaterialProperty::get_value(Some(time), fill_material.as_deref(), prev)?;
                    let translucent = mat.is_translucent();
                    self.material = Some(mat.clone());
                    let app = GeometryAppearanceKind::Material.build(translucent, closed, false, None, Some(mat));
                    let vf = app.vertex_format();
                    (Some(app), vf)
                };
                self.hooks.set_vertex_format(vertex_format);
                let fill_instance = self.hooks.create_fill_geometry_instance(time)?;
                let appearance = match appearance {
                    Some(app) => app,
                    None => {
                        let translucent = instance_alpha(&fill_instance) != 255;
                        GeometryAppearanceKind::PerInstanceColor.build(translucent, closed, false, None, None)
                    }
                };
                let prim = Rc::new(RefCell::new(GeometryBatchPrimitive::new(
                    vec![fill_instance],
                    Some(appearance),
                    None,
                    true,
                    false,
                    shadows,
                )));
                let handle =
                    PrimitiveHandle::from_rc(Rc::clone(&prim) as Rc<RefCell<dyn crate::scene::ScenePrimitive>>);
                self.primitives.borrow_mut().add(handle.clone(), None)?;
                self.primitive = Some((prim, handle));
            }
        }

        let outline_slot = geometry.get_property("outline");
        let show_outline = !on_terrain && outline_slot.is_some() && eval_bool(outline_slot.as_ref(), time, false)?;
        if show_outline {
            let outline_instance = self.hooks.create_outline_geometry_instance(time)?;
            let outline_width = eval_f64(geometry.get_property("outlineWidth").as_ref(), time, 1.0)?;
            let clamped = self.hooks.clamp_line_width(outline_width);
            let translucent = instance_alpha(&outline_instance) != 255;
            let appearance =
                GeometryAppearanceKind::PerInstanceColor.build(translucent, false, true, Some(clamped), None);
            let prim = Rc::new(RefCell::new(GeometryBatchPrimitive::new(
                vec![outline_instance],
                Some(appearance),
                None,
                true,
                false,
                shadows,
            )));
            let handle = PrimitiveHandle::from_rc(Rc::clone(&prim) as Rc<RefCell<dyn crate::scene::ScenePrimitive>>);
            self.primitives.borrow_mut().add(handle.clone(), None)?;
            self.outline_primitive = Some((prim, handle));
        }

        Ok(())
    }

    fn get_bounding_sphere(&self, result: &mut BoundingSphere) -> BoundingSphereState {
        let entity_id = self.hooks.entity().id().to_owned();

        if let Some((prim, _)) = &self.primitive {
            let prim = prim.borrow();
            if prim.show && prim.is_ready() {
                if let Some(bs) = prim.bounding_sphere_for(&entity_id, false) {
                    *result = bs;
                    return BoundingSphereState::Done;
                }
            }
        }

        if let Some((gp, _)) = &self.ground_primitive {
            let gp = gp.borrow();
            if gp.show && gp.is_ready() {
                if let Some(bs) = gp.bounding_sphere_for(&entity_id, false) {
                    *result = bs;
                    return BoundingSphereState::Done;
                }
            }
        }

        if let Some((outline, _)) = &self.outline_primitive {
            let outline = outline.borrow();
            if outline.show && outline.is_ready() {
                if let Some(bs) = outline.bounding_sphere_for(&entity_id, false) {
                    *result = bs;
                    return BoundingSphereState::Done;
                }
            }
        }

        let prim_pending = self.primitive.as_ref().is_some_and(|(p, _)| !p.borrow().is_ready())
            || self.ground_primitive.as_ref().is_some_and(|(p, _)| !p.borrow().is_ready())
            || self.outline_primitive.as_ref().is_some_and(|(p, _)| !p.borrow().is_ready());
        if prim_pending {
            return BoundingSphereState::Pending;
        }

        BoundingSphereState::Failed
    }

    fn is_destroyed(&self) -> bool {
        self.destroyed
    }

    fn destroy(&mut self) {
        if !self.destroyed {
            self.remove_primitives();
            self.destroyed = true;
        }
    }
}
