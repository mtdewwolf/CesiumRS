//! CesiumJS `WallGeometryUpdater` and `DynamicWallGeometryUpdater`.
use super::{
    dynamic_geometry_updater::{dynamic_default_is_hidden, DynamicGeometryHooks, StandardDynamicGeometryUpdater},
    geometry_updater::{
        eval_f64_array_opt, eval_positions_opt, is_color_material_component, GeometryChangedEvent, GeometryScene,
        GeometryUpdater, GeometryUpdaterBase,
    },
    visualizer_values::*,
    ComponentRef, DynamicGeometryUpdater, Entity, EntityComponent, EntityRef, PropertyInput,
};
use crate::{
    core::{
        iso8601, math::RADIANS_PER_DEGREE, Cartesian3, GeometryInstance, JulianDate, Matrix4, RuntimeError,
        VertexFormat, WallGeometry, WallGeometryOptions, WallOutlineGeometry,
    },
    scene::{MaterialSupport, OrderedGroundPrimitiveCollection, PerInstanceColorAppearance, PrimitiveCollection},
};
use std::{any::Any, cell::RefCell, rc::Rc};

const OBSERVED_PROPERTIES: &[&str] = &["availability", "wall"];

#[derive(Clone, Debug, Default)]
struct WallOptions {
    vertex_format: VertexFormat,
    positions: Option<Vec<Cartesian3>>,
    minimum_heights: Option<Vec<f64>>,
    maximum_heights: Option<Vec<f64>>,
    granularity: Option<f64>,
}

fn create_fill_instance(
    base: &GeometryUpdaterBase,
    options: &WallOptions,
    time: &JulianDate,
) -> Result<GeometryInstance, RuntimeError> {
    if !base.fill_enabled {
        return Err(RuntimeError::new("This instance does not represent a filled geometry."));
    }
    let positions = options.positions.clone().ok_or_else(|| RuntimeError::new("wall positions are required."))?;
    let mut geom_opts = WallGeometryOptions::new(positions);
    geom_opts.ellipsoid = base.scene.ellipsoid();
    geom_opts.vertex_format = options.vertex_format;
    geom_opts.minimum_heights = options.minimum_heights.clone();
    geom_opts.maximum_heights = options.maximum_heights.clone();
    geom_opts.granularity = options.granularity.unwrap_or(RADIANS_PER_DEGREE);

    let geometry = WallGeometry::new(geom_opts).create_geometry();
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
    options: &WallOptions,
    time: &JulianDate,
) -> Result<GeometryInstance, RuntimeError> {
    if !base.outline_enabled {
        return Err(RuntimeError::new("This instance does not represent an outlined geometry."));
    }
    let positions = options.positions.clone().ok_or_else(|| RuntimeError::new("wall positions are required."))?;
    let geometry = WallOutlineGeometry::new(
        positions,
        options.maximum_heights.clone(),
        options.minimum_heights.clone(),
        options.granularity,
        Some(base.scene.ellipsoid()),
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

/// Port of CesiumJS `WallGeometryUpdater`.
pub struct WallGeometryUpdater {
    id: String,
    entity: EntityRef,
    geometry_changed: super::ChangeEvent<GeometryChangedEvent>,
    base: Rc<RefCell<GeometryUpdaterBase>>,
    options: Rc<RefCell<WallOptions>>,
}

impl WallGeometryUpdater {
    pub fn new(entity: EntityRef, scene: Rc<dyn GeometryScene>) -> Self {
        let base = GeometryUpdaterBase::new(Rc::clone(&entity), scene, "wall", OBSERVED_PROPERTIES);
        let id = base.id.clone();
        let geometry_changed = base.geometry_changed.clone();
        let mut updater = Self {
            id,
            entity: Rc::clone(&entity),
            geometry_changed,
            base: Rc::new(RefCell::new(base)),
            options: Rc::new(RefCell::new(WallOptions::default())),
        };
        updater.on_entity_property_changed(&entity, "wall");
        updater
    }
}

impl GeometryUpdater for WallGeometryUpdater {
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
            DynamicWallHooks { base: Rc::clone(&self.base), options: Rc::clone(&self.options) },
            primitives,
            ground_primitives,
        )))
    }

    fn on_entity_property_changed(&mut self, entity: &Entity, property_name: &str) {
        let options_ref = Rc::clone(&self.options);
        self.base.borrow_mut().on_entity_property_changed_with(
            entity,
            property_name,
            |_e, wall| wall.get_property("positions").is_none() || GeometryUpdaterBase::default_is_hidden(wall),
            |_base, _e, _wall| false,
            |_base, _e, wall| {
                !is_slot_constant(wall.get_property("positions").as_ref())
                    || !is_slot_constant(wall.get_property("minimumHeights").as_ref())
                    || !is_slot_constant(wall.get_property("maximumHeights").as_ref())
                    || !is_slot_constant(wall.get_property("outlineWidth").as_ref())
                    || !is_slot_constant(wall.get_property("granularity").as_ref())
            },
            |base, _e, wall| {
                let is_color = base.material_property.as_ref().is_some_and(is_color_material_component);
                let min_time = iso8601::minimum_value();
                let mut opts = options_ref.borrow_mut();
                opts.vertex_format = if is_color {
                    PerInstanceColorAppearance::VERTEX_FORMAT
                } else {
                    MaterialSupport::Textured.vertex_format()
                };
                opts.positions = eval_positions_opt(wall.get_property("positions").as_ref(), &min_time).ok().flatten();
                opts.minimum_heights =
                    eval_f64_array_opt(wall.get_property("minimumHeights").as_ref(), &min_time).ok().flatten();
                opts.maximum_heights =
                    eval_f64_array_opt(wall.get_property("maximumHeights").as_ref(), &min_time).ok().flatten();
                opts.granularity = eval_f64_opt(wall.get_property("granularity").as_ref(), &min_time).ok().flatten();
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

struct DynamicWallHooks {
    base: Rc<RefCell<GeometryUpdaterBase>>,
    options: Rc<RefCell<WallOptions>>,
}

impl DynamicGeometryHooks for DynamicWallHooks {
    fn entity(&self) -> EntityRef {
        Rc::clone(&self.base.borrow().entity)
    }

    fn geometry_property_name(&self) -> &'static str {
        "wall"
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
        opts.positions = eval_positions_opt(geometry.get_property("positions").as_ref(), time)?;
        opts.minimum_heights = eval_f64_array_opt(geometry.get_property("minimumHeights").as_ref(), time)?;
        opts.maximum_heights = eval_f64_array_opt(geometry.get_property("maximumHeights").as_ref(), time)?;
        opts.granularity = eval_f64_opt(geometry.get_property("granularity").as_ref(), time)?;
        Ok(())
    }

    fn is_hidden(
        &self,
        entity: &Entity,
        geometry: &dyn EntityComponent,
        time: &JulianDate,
    ) -> Result<bool, RuntimeError> {
        let opts = self.options.borrow();
        Ok(opts.positions.is_none() || dynamic_default_is_hidden(entity, geometry, time)?)
    }

    fn get_is_closed(&self) -> bool {
        false
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
