//! CesiumJS `PolylineVolumeGeometryUpdater` and `DynamicPolylineVolumeGeometryUpdater`.
use super::{
    dynamic_geometry_updater::{dynamic_default_is_hidden, DynamicGeometryHooks, StandardDynamicGeometryUpdater},
    geometry_updater::{
        eval_cartesian2_array_opt, eval_corner_type_opt, eval_positions_opt, is_color_material_component,
        GeometryChangedEvent, GeometryScene, GeometryUpdater, GeometryUpdaterBase,
    },
    visualizer_values::*,
    ComponentRef, DynamicGeometryUpdater, Entity, EntityComponent, EntityRef, PropertyInput,
};
use crate::{
    core::{
        iso8601, math::RADIANS_PER_DEGREE, Cartesian2, Cartesian3, CornerType, GeometryInstance, JulianDate, Matrix4,
        PolylineVolumeGeometry, PolylineVolumeGeometryOptions, PolylineVolumeOutlineGeometry, RuntimeError,
        VertexFormat,
    },
    scene::{MaterialSupport, OrderedGroundPrimitiveCollection, PerInstanceColorAppearance, PrimitiveCollection},
};
use std::{any::Any, cell::RefCell, rc::Rc};

const OBSERVED_PROPERTIES: &[&str] = &["availability", "polylineVolume"];

#[derive(Clone, Debug, Default)]
struct PolylineVolumeOptions {
    vertex_format: VertexFormat,
    polyline_positions: Option<Vec<Cartesian3>>,
    shape_positions: Option<Vec<Cartesian2>>,
    corner_type: Option<CornerType>,
    granularity: Option<f64>,
}

fn create_fill_instance(
    base: &GeometryUpdaterBase,
    options: &PolylineVolumeOptions,
    time: &JulianDate,
) -> Result<GeometryInstance, RuntimeError> {
    if !base.fill_enabled {
        return Err(RuntimeError::new("This instance does not represent a filled geometry."));
    }
    let polyline_positions = options
        .polyline_positions
        .clone()
        .ok_or_else(|| RuntimeError::new("polylineVolume positions are required."))?;
    let shape_positions =
        options.shape_positions.clone().ok_or_else(|| RuntimeError::new("polylineVolume shape is required."))?;

    let mut geom_opts = PolylineVolumeGeometryOptions::new(polyline_positions, shape_positions);
    geom_opts.ellipsoid = base.scene.ellipsoid();
    geom_opts.vertex_format = options.vertex_format;
    geom_opts.corner_type = options.corner_type.unwrap_or(CornerType::Rounded);
    geom_opts.granularity = options.granularity.unwrap_or(RADIANS_PER_DEGREE);

    let geometry = PolylineVolumeGeometry::new(geom_opts).create_geometry();
    let attributes = base.build_fill_attributes(time, None, None)?;
    Ok(GeometryInstance {
        geometry,
        model_matrix: Matrix4::IDENTITY,
        id: Some(base.entity.id().to_owned()),
        attributes,
        west_hemisphere_geometry: None,
        east_hemisphere_geometry: None,
    })
}

fn create_outline_instance(
    base: &GeometryUpdaterBase,
    options: &PolylineVolumeOptions,
    time: &JulianDate,
) -> Result<GeometryInstance, RuntimeError> {
    if !base.outline_enabled {
        return Err(RuntimeError::new("This instance does not represent an outlined geometry."));
    }
    let polyline_positions = options
        .polyline_positions
        .clone()
        .ok_or_else(|| RuntimeError::new("polylineVolume positions are required."))?;
    let shape_positions =
        options.shape_positions.clone().ok_or_else(|| RuntimeError::new("polylineVolume shape is required."))?;

    let geometry = PolylineVolumeOutlineGeometry::new(
        polyline_positions,
        shape_positions,
        Some(base.scene.ellipsoid()),
        options.corner_type,
        options.granularity,
    )
    .create_geometry();
    let attributes = base.build_outline_attributes(time, None, None)?;
    Ok(GeometryInstance {
        geometry,
        model_matrix: Matrix4::IDENTITY,
        id: Some(base.entity.id().to_owned()),
        attributes,
        west_hemisphere_geometry: None,
        east_hemisphere_geometry: None,
    })
}

/// Port of CesiumJS `PolylineVolumeGeometryUpdater`.
pub struct PolylineVolumeGeometryUpdater {
    id: String,
    entity: EntityRef,
    geometry_changed: super::ChangeEvent<GeometryChangedEvent>,
    base: Rc<RefCell<GeometryUpdaterBase>>,
    options: Rc<RefCell<PolylineVolumeOptions>>,
}

impl PolylineVolumeGeometryUpdater {
    pub fn new(entity: EntityRef, scene: Rc<dyn GeometryScene>) -> Self {
        let base = GeometryUpdaterBase::new(Rc::clone(&entity), scene, "polylineVolume", OBSERVED_PROPERTIES);
        let id = base.id.clone();
        let geometry_changed = base.geometry_changed.clone();
        let mut updater = Self {
            id,
            entity: Rc::clone(&entity),
            geometry_changed,
            base: Rc::new(RefCell::new(base)),
            options: Rc::new(RefCell::new(PolylineVolumeOptions::default())),
        };
        updater.on_entity_property_changed(&entity, "polylineVolume");
        updater
    }
}

impl GeometryUpdater for PolylineVolumeGeometryUpdater {
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
        create_fill_instance(&self.base.borrow(), &self.options.borrow(), time)
    }

    fn create_outline_geometry_instance(&self, time: &JulianDate) -> Result<GeometryInstance, RuntimeError> {
        create_outline_instance(&self.base.borrow(), &self.options.borrow(), time)
    }

    fn create_dynamic_updater(
        &self,
        primitives: Rc<RefCell<PrimitiveCollection>>,
        ground_primitives: Rc<RefCell<OrderedGroundPrimitiveCollection>>,
    ) -> Result<Box<dyn DynamicGeometryUpdater>, RuntimeError> {
        if !self.is_dynamic() {
            return Err(RuntimeError::new("This instance does not represent dynamic geometry."));
        }
        Ok(Box::new(StandardDynamicGeometryUpdater::new(
            DynamicPolylineVolumeHooks { base: Rc::clone(&self.base), options: Rc::clone(&self.options) },
            primitives,
            ground_primitives,
        )))
    }

    fn on_entity_property_changed(&mut self, entity: &Entity, property_name: &str) {
        let options_ref = Rc::clone(&self.options);
        self.base.borrow_mut().on_entity_property_changed_with(
            entity,
            property_name,
            |_e, pv| {
                pv.get_property("positions").is_none()
                    || pv.get_property("shape").is_none()
                    || GeometryUpdaterBase::default_is_hidden(pv)
            },
            |_base, _e, _pv| false,
            |_base, _e, pv| {
                !is_slot_constant(pv.get_property("positions").as_ref())
                    || !is_slot_constant(pv.get_property("shape").as_ref())
                    || !is_slot_constant(pv.get_property("granularity").as_ref())
                    || !is_slot_constant(pv.get_property("outlineWidth").as_ref())
                    || !is_slot_constant(pv.get_property("cornerType").as_ref())
            },
            |base, _e, pv| {
                let is_color = base.material_property.as_ref().is_some_and(is_color_material_component);
                let min_time = iso8601::minimum_value();
                let mut opts = options_ref.borrow_mut();
                opts.vertex_format = if is_color {
                    PerInstanceColorAppearance::VERTEX_FORMAT
                } else {
                    MaterialSupport::Textured.vertex_format()
                };
                opts.polyline_positions =
                    eval_positions_opt(pv.get_property("positions").as_ref(), &min_time).ok().flatten();
                opts.shape_positions =
                    eval_cartesian2_array_opt(pv.get_property("shape").as_ref(), &min_time).ok().flatten();
                opts.granularity = eval_f64_opt(pv.get_property("granularity").as_ref(), &min_time).ok().flatten();
                opts.corner_type =
                    eval_corner_type_opt(pv.get_property("cornerType").as_ref(), &min_time).ok().flatten();
            },
            || true,
        );
    }

    fn is_destroyed(&self) -> bool {
        self.base.borrow().destroyed
    }

    fn destroy(&mut self) {
        self.base.borrow_mut().destroy();
    }
}

struct DynamicPolylineVolumeHooks {
    base: Rc<RefCell<GeometryUpdaterBase>>,
    options: Rc<RefCell<PolylineVolumeOptions>>,
}

impl DynamicGeometryHooks for DynamicPolylineVolumeHooks {
    fn entity(&self) -> EntityRef {
        Rc::clone(&self.base.borrow().entity)
    }

    fn geometry_property_name(&self) -> &'static str {
        "polylineVolume"
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
        opts.polyline_positions = eval_positions_opt(geometry.get_property("positions").as_ref(), time)?;
        opts.shape_positions = eval_cartesian2_array_opt(geometry.get_property("shape").as_ref(), time)?;
        opts.granularity = eval_f64_opt(geometry.get_property("granularity").as_ref(), time)?;
        opts.corner_type = eval_corner_type_opt(geometry.get_property("cornerType").as_ref(), time)?;
        Ok(())
    }

    fn is_hidden(
        &self,
        entity: &Entity,
        geometry: &dyn EntityComponent,
        time: &JulianDate,
    ) -> Result<bool, RuntimeError> {
        let opts = self.options.borrow();
        Ok(opts.polyline_positions.is_none()
            || opts.shape_positions.is_none()
            || dynamic_default_is_hidden(entity, geometry, time)?)
    }

    fn get_is_closed(&self) -> bool {
        true
    }

    fn set_vertex_format(&mut self, vertex_format: VertexFormat) {
        self.options.borrow_mut().vertex_format = vertex_format;
    }

    fn create_fill_geometry_instance(&self, time: &JulianDate) -> Result<GeometryInstance, RuntimeError> {
        create_fill_instance(&self.base.borrow(), &self.options.borrow(), time)
    }

    fn create_outline_geometry_instance(&self, time: &JulianDate) -> Result<GeometryInstance, RuntimeError> {
        create_outline_instance(&self.base.borrow(), &self.options.borrow(), time)
    }
}
