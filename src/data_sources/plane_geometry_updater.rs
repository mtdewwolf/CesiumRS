//! CesiumJS `PlaneGeometryUpdater` and `DynamicPlaneGeometryUpdater`.
use super::{
    dynamic_geometry_updater::{dynamic_default_is_hidden, DynamicGeometryHooks, StandardDynamicGeometryUpdater},
    geometry_updater::{
        eval_plane_opt, is_color_material_component, GeometryChangedEvent, GeometryScene, GeometryUpdater,
        GeometryUpdaterBase,
    },
    visualizer_values::*,
    ComponentRef, DynamicGeometryUpdater, Entity, EntityComponent, EntityRef, PropertyInput,
};
use crate::{
    core::{
        iso8601, math, Cartesian2, Cartesian3, GeometryInstance, JulianDate, Matrix3, Matrix4, Plane, PlaneGeometry,
        PlaneOutlineGeometry, RuntimeError, VertexFormat,
    },
    scene::{MaterialSupport, OrderedGroundPrimitiveCollection, PerInstanceColorAppearance, PrimitiveCollection},
};
use std::{any::Any, cell::RefCell, rc::Rc};

const OBSERVED_PROPERTIES: &[&str] = &["availability", "position", "orientation", "plane"];

#[derive(Clone, Debug, Default)]
struct PlaneOptions {
    vertex_format: VertexFormat,
    plane: Option<Plane>,
    dimensions: Option<Cartesian2>,
}

fn create_fill_instance(
    base: &GeometryUpdaterBase,
    options: &mut PlaneOptions,
    time: &JulianDate,
) -> Result<GeometryInstance, RuntimeError> {
    if !base.fill_enabled {
        return Err(RuntimeError::new("This instance does not represent a filled geometry."));
    }
    let plane_comp = base
        .entity
        .get_property("plane")
        .and_then(|p| p.component())
        .ok_or_else(|| RuntimeError::new("entity.plane is required."))?;

    let transform = base.entity.compute_model_matrix(time, None)?.unwrap_or(Matrix4::IDENTITY);
    let plane = eval_plane_opt(plane_comp.get_property("plane").as_ref(), time)?
        .or(options.plane)
        .ok_or_else(|| RuntimeError::new("plane is required."))?;
    let dimensions = eval_cartesian2_opt(plane_comp.get_property("dimensions").as_ref(), time)?
        .or(options.dimensions)
        .ok_or_else(|| RuntimeError::new("plane dimensions are required."))?;

    options.plane = Some(plane);
    options.dimensions = Some(dimensions);

    let model_matrix = PlaneGeometryUpdater::create_primitive_matrix(&plane, dimensions, &transform);
    let geometry = Some(PlaneGeometry::new(options.vertex_format).create_geometry());
    let attributes = base.build_fill_attributes(time, None, None)?;
    Ok(GeometryInstance {
        geometry,
        model_matrix,
        id: Some(base.entity.id().to_owned()),
        attributes,
        west_hemisphere_geometry: None,
        east_hemisphere_geometry: None,
    })
}

fn create_outline_instance(
    base: &GeometryUpdaterBase,
    options: &mut PlaneOptions,
    time: &JulianDate,
) -> Result<GeometryInstance, RuntimeError> {
    if !base.outline_enabled {
        return Err(RuntimeError::new("This instance does not represent an outlined geometry."));
    }
    let plane_comp = base
        .entity
        .get_property("plane")
        .and_then(|p| p.component())
        .ok_or_else(|| RuntimeError::new("entity.plane is required."))?;

    let transform = base.entity.compute_model_matrix(time, None)?.unwrap_or(Matrix4::IDENTITY);
    let plane = eval_plane_opt(plane_comp.get_property("plane").as_ref(), time)?
        .or(options.plane)
        .ok_or_else(|| RuntimeError::new("plane is required."))?;
    let dimensions = eval_cartesian2_opt(plane_comp.get_property("dimensions").as_ref(), time)?
        .or(options.dimensions)
        .ok_or_else(|| RuntimeError::new("plane dimensions are required."))?;

    options.plane = Some(plane);
    options.dimensions = Some(dimensions);

    let model_matrix = PlaneGeometryUpdater::create_primitive_matrix(&plane, dimensions, &transform);
    let geometry = Some(PlaneOutlineGeometry.create_geometry());
    let attributes = base.build_outline_attributes(time, None, None)?;
    Ok(GeometryInstance {
        geometry,
        model_matrix,
        id: Some(base.entity.id().to_owned()),
        attributes,
        west_hemisphere_geometry: None,
        east_hemisphere_geometry: None,
    })
}

/// Port of CesiumJS `PlaneGeometryUpdater`.
pub struct PlaneGeometryUpdater {
    id: String,
    entity: EntityRef,
    geometry_changed: super::ChangeEvent<GeometryChangedEvent>,
    base: Rc<RefCell<GeometryUpdaterBase>>,
    options: Rc<RefCell<PlaneOptions>>,
}

impl PlaneGeometryUpdater {
    pub fn new(entity: EntityRef, scene: Rc<dyn GeometryScene>) -> Self {
        let base = GeometryUpdaterBase::new(Rc::clone(&entity), scene, "plane", OBSERVED_PROPERTIES);
        let id = base.id.clone();
        let geometry_changed = base.geometry_changed.clone();
        let mut updater = Self {
            id,
            entity: Rc::clone(&entity),
            geometry_changed,
            base: Rc::new(RefCell::new(base)),
            options: Rc::new(RefCell::new(PlaneOptions::default())),
        };
        updater.on_entity_property_changed(&entity, "plane");
        updater
    }

    /// Port of CesiumJS `PlaneGeometryUpdater.createPrimitiveMatrix`.
    pub fn create_primitive_matrix(plane: &Plane, dimensions: Cartesian2, transform: &Matrix4) -> Matrix4 {
        let normal = plane.normal;
        let translation = normal.multiply_by_scalar(-plane.distance);

        let mut up = Cartesian3::UNIT_Z;
        if math::equals_epsilon(up.dot(normal).abs(), 1.0, math::EPSILON8, math::EPSILON8) {
            up = Cartesian3::UNIT_Y;
        }

        let left = up.cross(normal).normalize();
        up = normal.cross(left).normalize();

        let rotation = Matrix3::IDENTITY.set_column(0, left).set_column(1, up).set_column(2, normal);
        let scale = Cartesian3::new(dimensions.x, dimensions.y, 1.0);
        let rotation_scale = rotation.multiply_by_scale(scale);
        let local_transform = Matrix4::from_rotation_translation(&rotation_scale, translation);
        transform.multiply_transformation(&local_transform)
    }
}

impl GeometryUpdater for PlaneGeometryUpdater {
    fn as_any(&self) -> &dyn Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn Any {
        self
    }

    fn id(&self) -> &str {
        &self.id
    }

    fn entity(&self) -> EntityRef {
        Rc::clone(&self.entity)
    }

    fn scene(&self) -> Rc<dyn GeometryScene> {
        Rc::clone(&self.base.borrow().scene)
    }

    fn fill_enabled(&self) -> bool {
        self.base.borrow().fill_enabled
    }

    fn has_constant_fill(&self) -> bool {
        self.base.borrow().has_constant_fill()
    }

    fn fill_material_property(&self) -> Option<ComponentRef> {
        self.base.borrow().material_property.clone()
    }

    fn outline_enabled(&self) -> bool {
        self.base.borrow().outline_enabled
    }

    fn has_constant_outline(&self) -> bool {
        self.base.borrow().has_constant_outline()
    }

    fn outline_color_property(&self) -> Option<PropertyInput> {
        self.base.borrow().outline_color_property.clone()
    }

    fn outline_width(&self) -> f64 {
        self.base.borrow().outline_width
    }

    fn shadows_property(&self) -> Option<PropertyInput> {
        self.base.borrow().shadows_property.clone()
    }

    fn distance_display_condition_property(&self) -> Option<PropertyInput> {
        self.base.borrow().distance_display_condition_property.clone()
    }

    fn classification_type_property(&self) -> Option<PropertyInput> {
        self.base.borrow().classification_type_property.clone()
    }

    fn is_dynamic(&self) -> bool {
        self.base.borrow().dynamic
    }

    fn is_closed(&self) -> bool {
        self.base.borrow().is_closed
    }

    fn on_terrain(&self) -> bool {
        self.base.borrow().on_terrain
    }

    fn geometry_changed(&self) -> &super::ChangeEvent<GeometryChangedEvent> {
        &self.geometry_changed
    }

    fn is_filled(&self, time: &JulianDate) -> Result<bool, RuntimeError> {
        self.base.borrow().is_filled(time)
    }

    fn is_outline_visible(&self, time: &JulianDate) -> Result<bool, RuntimeError> {
        self.base.borrow().is_outline_visible(time)
    }

    fn create_fill_geometry_instance(&self, time: &JulianDate) -> Result<GeometryInstance, RuntimeError> {
        create_fill_instance(&self.base.borrow(), &mut self.options.borrow_mut(), time)
    }

    fn create_outline_geometry_instance(&self, time: &JulianDate) -> Result<GeometryInstance, RuntimeError> {
        create_outline_instance(&self.base.borrow(), &mut self.options.borrow_mut(), time)
    }

    fn create_dynamic_updater(
        &self,
        primitives: Rc<RefCell<PrimitiveCollection>>,
        ground_primitives: Rc<RefCell<OrderedGroundPrimitiveCollection>>,
    ) -> Result<Box<dyn DynamicGeometryUpdater>, RuntimeError> {
        if !self.base.borrow().dynamic {
            return Err(RuntimeError::new("This instance does not represent dynamic geometry."));
        }
        let hooks = DynamicPlaneHooks { base: Rc::clone(&self.base), options: Rc::clone(&self.options) };
        Ok(Box::new(StandardDynamicGeometryUpdater::new(hooks, primitives, ground_primitives)))
    }

    fn on_entity_property_changed(&mut self, entity: &Entity, property_name: &str) {
        let options_rc = Rc::clone(&self.options);
        self.base.borrow_mut().on_entity_property_changed_with(
            entity,
            property_name,
            |entity, plane| {
                plane.get_property("plane").is_none()
                    || plane.get_property("dimensions").is_none()
                    || entity.get_property("position").is_none()
                    || GeometryUpdaterBase::default_is_hidden(plane)
            },
            |_base, _entity, _plane| false,
            |_base, entity, plane| {
                !is_slot_constant(entity.get_property("position").as_ref())
                    || !is_slot_constant(entity.get_property("orientation").as_ref())
                    || !is_slot_constant(plane.get_property("plane").as_ref())
                    || !is_slot_constant(plane.get_property("dimensions").as_ref())
                    || !is_slot_constant(plane.get_property("outlineWidth").as_ref())
            },
            |base, _entity, plane| {
                let min_time = iso8601::minimum_value();
                let mut opts = options_rc.borrow_mut();
                opts.vertex_format = if base.material_property.as_ref().is_some_and(is_color_material_component) {
                    PerInstanceColorAppearance::VERTEX_FORMAT
                } else {
                    MaterialSupport::Textured.vertex_format()
                };
                opts.plane = eval_plane_opt(plane.get_property("plane").as_ref(), &min_time).ok().flatten();
                opts.dimensions =
                    eval_cartesian2_opt(plane.get_property("dimensions").as_ref(), &min_time).ok().flatten();
            },
            || false,
        );
    }

    fn is_destroyed(&self) -> bool {
        self.base.borrow().destroyed
    }

    fn destroy(&mut self) {
        self.base.borrow_mut().destroy();
    }
}

struct DynamicPlaneHooks {
    base: Rc<RefCell<GeometryUpdaterBase>>,
    options: Rc<RefCell<PlaneOptions>>,
}

impl DynamicGeometryHooks for DynamicPlaneHooks {
    fn entity(&self) -> EntityRef {
        Rc::clone(&self.base.borrow().entity)
    }

    fn geometry_property_name(&self) -> &'static str {
        "plane"
    }

    fn on_terrain(&self) -> bool {
        self.base.borrow().on_terrain
    }

    fn supports_materials_for_entities_on_terrain(&self) -> bool {
        self.base.borrow().supports_materials_for_entities_on_terrain
    }

    fn shadows_property(&self) -> Option<PropertyInput> {
        self.base.borrow().shadows_property.clone()
    }

    fn classification_type_property(&self) -> Option<PropertyInput> {
        self.base.borrow().classification_type_property.clone()
    }

    fn fill_material_property(&self) -> Option<ComponentRef> {
        self.base.borrow().material_property.clone()
    }

    fn z_index(&self) -> Option<PropertyInput> {
        None
    }

    fn clamp_line_width(&self, width: f64) -> f64 {
        self.base.borrow().scene.clamp_line_width(width)
    }

    fn set_options(
        &mut self,
        _entity: &Entity,
        geometry: &dyn EntityComponent,
        time: &JulianDate,
    ) -> Result<(), RuntimeError> {
        let mut opts = self.options.borrow_mut();
        if let Some(p) = eval_plane_opt(geometry.get_property("plane").as_ref(), time)? {
            opts.plane = Some(p);
        }
        opts.dimensions = eval_cartesian2_opt(geometry.get_property("dimensions").as_ref(), time)?;
        Ok(())
    }

    fn is_hidden(
        &self,
        entity: &Entity,
        geometry: &dyn EntityComponent,
        time: &JulianDate,
    ) -> Result<bool, RuntimeError> {
        let position = entity.position_at(time)?;
        let opts = self.options.borrow();
        Ok(position.is_none()
            || opts.plane.is_none()
            || opts.dimensions.is_none()
            || dynamic_default_is_hidden(entity, geometry, time)?)
    }

    fn get_is_closed(&self) -> bool {
        false
    }

    fn set_vertex_format(&mut self, vertex_format: VertexFormat) {
        self.options.borrow_mut().vertex_format = vertex_format;
    }

    fn create_fill_geometry_instance(&self, time: &JulianDate) -> Result<GeometryInstance, RuntimeError> {
        create_fill_instance(&self.base.borrow(), &mut self.options.borrow_mut(), time)
    }

    fn create_outline_geometry_instance(&self, time: &JulianDate) -> Result<GeometryInstance, RuntimeError> {
        create_outline_instance(&self.base.borrow(), &mut self.options.borrow_mut(), time)
    }
}
