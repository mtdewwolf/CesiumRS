//! CesiumJS `EllipseGeometryUpdater` and `DynamicEllipseGeometryUpdater`.
use super::{
    dynamic_geometry_updater::{dynamic_default_is_hidden, DynamicGeometryHooks, StandardDynamicGeometryUpdater},
    geometry_updater::{
        is_color_material_component, GeometryChangedEvent, GeometryScene, GeometryUpdater, GeometryUpdaterBase,
    },
    ground_geometry_updater::{GroundExtrudedHeight, GroundGeometryUpdater},
    visualizer_values::*,
    ComponentRef, DynamicGeometryUpdater, Entity, EntityComponent, EntityRef, PropertyInput, TerrainOffsetProperty,
};
use crate::{
    core::{
        approximate_terrain_heights::DEFAULT_MIN_TERRAIN_HEIGHT, iso8601, math::RADIANS_PER_DEGREE,
        ApproximateTerrainHeights, Cartesian3, EllipseGeometry, EllipseGeometryOptions, EllipseOutlineGeometry,
        EllipseOutlineOptions, Ellipsoid, GeometryInstance, GeometryOffsetAttribute, JulianDate, Matrix4, Rectangle,
        RuntimeError, VertexFormat,
    },
    scene::{
        HeightReference, MaterialSupport, OrderedGroundPrimitiveCollection, PerInstanceColorAppearance,
        PrimitiveCollection,
    },
};
use std::{any::Any, cell::RefCell, rc::Rc};

const OBSERVED_PROPERTIES: &[&str] = &["availability", "position", "ellipse"];

#[derive(Clone, Debug, Default)]
struct EllipseOptions {
    vertex_format: VertexFormat,
    center: Option<Cartesian3>,
    semi_major_axis: Option<f64>,
    semi_minor_axis: Option<f64>,
    rotation: Option<f64>,
    height: Option<f64>,
    extruded_height: Option<f64>,
    granularity: Option<f64>,
    st_rotation: Option<f64>,
    number_of_vertical_lines: Option<f64>,
    offset_attribute: Option<GeometryOffsetAttribute>,
}

fn compute_ellipse_rectangle(opts: &EllipseOptions, ellipsoid: &Ellipsoid) -> Option<Rectangle> {
    let center = opts.center?;
    let major = opts.semi_major_axis?;
    let minor = opts.semi_minor_axis?;
    if major < minor || major <= 0.0 || minor <= 0.0 {
        return None;
    }
    Some(EllipseGeometry::compute_rectangle(
        center,
        major,
        minor,
        opts.rotation.unwrap_or(0.0),
        opts.granularity.unwrap_or(RADIANS_PER_DEGREE),
        ellipsoid,
    ))
}

fn populate_options(
    opts: &mut EllipseOptions,
    entity: &Entity,
    ellipse: &dyn EntityComponent,
    time: &JulianDate,
    ellipsoid: &Ellipsoid,
) -> Result<(), RuntimeError> {
    let mut height_value = eval_f64_opt(ellipse.get_property("height").as_ref(), time)?;
    let hr_value =
        eval_height_reference(ellipse.get_property("heightReference").as_ref(), time, HeightReference::None)?;
    let extruded_height_value = eval_f64_opt(ellipse.get_property("extrudedHeight").as_ref(), time)?;
    let ehr_value =
        eval_height_reference(ellipse.get_property("extrudedHeightReference").as_ref(), time, HeightReference::None)?;
    if extruded_height_value.is_some() && height_value.is_none() {
        height_value = Some(0.0);
    }

    opts.center = entity.position_at(time)?;
    opts.semi_major_axis = eval_f64_opt(ellipse.get_property("semiMajorAxis").as_ref(), time)?;
    opts.semi_minor_axis = eval_f64_opt(ellipse.get_property("semiMinorAxis").as_ref(), time)?;
    opts.rotation = eval_f64_opt(ellipse.get_property("rotation").as_ref(), time)?;
    opts.granularity = eval_f64_opt(ellipse.get_property("granularity").as_ref(), time)?;
    opts.st_rotation = eval_f64_opt(ellipse.get_property("stRotation").as_ref(), time)?;
    opts.number_of_vertical_lines = eval_f64_opt(ellipse.get_property("numberOfVerticalLines").as_ref(), time)?;
    opts.offset_attribute = GroundGeometryUpdater::compute_geometry_offset_attribute(
        height_value,
        Some(hr_value),
        extruded_height_value,
        Some(ehr_value),
    );
    opts.height = GroundGeometryUpdater::get_geometry_height(height_value, hr_value);

    opts.extruded_height = match GroundGeometryUpdater::get_geometry_extruded_height(extruded_height_value, ehr_value) {
        Some(GroundExtrudedHeight::Height(h)) => Some(h),
        Some(GroundExtrudedHeight::ClampToGround) => {
            let min_h = compute_ellipse_rectangle(opts, ellipsoid)
                .and_then(|rect| ApproximateTerrainHeights::bundled().get_minimum_maximum_heights(&rect, ellipsoid))
                .map_or(DEFAULT_MIN_TERRAIN_HEIGHT, |(min, _)| min);
            Some(min_h)
        }
        None => None,
    };
    Ok(())
}

fn create_fill_instance(
    ground: &GroundGeometryUpdater,
    options: &EllipseOptions,
    time: &JulianDate,
) -> Result<GeometryInstance, RuntimeError> {
    let base = &ground.base;
    if !base.fill_enabled {
        return Err(RuntimeError::new("This instance does not represent a filled geometry."));
    }
    let center = options.center.ok_or_else(|| RuntimeError::new("ellipse center is required."))?;
    let major = options.semi_major_axis.ok_or_else(|| RuntimeError::new("ellipse semiMajorAxis is required."))?;
    let minor = options.semi_minor_axis.ok_or_else(|| RuntimeError::new("ellipse semiMinorAxis is required."))?;

    let mut geom_opts = EllipseGeometryOptions::new(center, major, minor);
    geom_opts.ellipsoid = base.scene.ellipsoid();
    geom_opts.vertex_format = options.vertex_format;
    geom_opts.height = options.height.unwrap_or(0.0);
    geom_opts.extruded_height = options.extruded_height;
    geom_opts.rotation = options.rotation.unwrap_or(0.0);
    geom_opts.st_rotation = options.st_rotation.unwrap_or(0.0);
    geom_opts.granularity = options.granularity.unwrap_or(RADIANS_PER_DEGREE);
    geom_opts.offset_attribute = options.offset_attribute;

    let geometry = EllipseGeometry::new(geom_opts).and_then(|g| g.create_geometry());
    let attributes =
        base.build_fill_attributes(time, options.offset_attribute, ground.terrain_offset_property.as_ref())?;
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
    ground: &GroundGeometryUpdater,
    options: &EllipseOptions,
    time: &JulianDate,
) -> Result<GeometryInstance, RuntimeError> {
    let base = &ground.base;
    if !base.outline_enabled {
        return Err(RuntimeError::new("This instance does not represent an outlined geometry."));
    }
    let center = options.center.ok_or_else(|| RuntimeError::new("ellipse center is required."))?;
    let major = options.semi_major_axis.ok_or_else(|| RuntimeError::new("ellipse semiMajorAxis is required."))?;
    let minor = options.semi_minor_axis.ok_or_else(|| RuntimeError::new("ellipse semiMinorAxis is required."))?;

    let mut geom_opts = EllipseOutlineOptions::new(center, major, minor);
    geom_opts.ellipsoid = base.scene.ellipsoid();
    geom_opts.height = options.height.unwrap_or(0.0);
    geom_opts.extruded_height = options.extruded_height;
    geom_opts.rotation = options.rotation.unwrap_or(0.0);
    geom_opts.granularity = options.granularity.unwrap_or(RADIANS_PER_DEGREE);
    geom_opts.number_of_vertical_lines = options.number_of_vertical_lines.unwrap_or(16.0);
    geom_opts.offset_attribute = options.offset_attribute;

    let geometry = EllipseOutlineGeometry::new(geom_opts).and_then(|g| g.create_geometry());
    let attributes =
        base.build_outline_attributes(time, options.offset_attribute, ground.terrain_offset_property.as_ref())?;
    Ok(GeometryInstance {
        geometry,
        model_matrix: Matrix4::IDENTITY,
        id: Some(base.entity.id().to_owned()),
        attributes,
        west_hemisphere_geometry: None,
        east_hemisphere_geometry: None,
    })
}

/// Port of CesiumJS `EllipseGeometryUpdater`.
pub struct EllipseGeometryUpdater {
    id: String,
    entity: EntityRef,
    geometry_changed: super::ChangeEvent<GeometryChangedEvent>,
    ground: Rc<RefCell<GroundGeometryUpdater>>,
    options: Rc<RefCell<EllipseOptions>>,
}

impl EllipseGeometryUpdater {
    pub fn new(entity: EntityRef, scene: Rc<dyn GeometryScene>) -> Self {
        let ground = GroundGeometryUpdater::new(Rc::clone(&entity), scene, "ellipse", OBSERVED_PROPERTIES);
        let id = ground.base.id.clone();
        let geometry_changed = ground.base.geometry_changed.clone();
        let mut updater = Self {
            id,
            entity: Rc::clone(&entity),
            geometry_changed,
            ground: Rc::new(RefCell::new(ground)),
            options: Rc::new(RefCell::new(EllipseOptions::default())),
        };
        updater.on_entity_property_changed(&entity, "ellipse");
        updater
    }
}

impl GeometryUpdater for EllipseGeometryUpdater {
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
        Rc::clone(&self.ground.borrow().base.scene)
    }

    fn fill_enabled(&self) -> bool {
        self.ground.borrow().base.fill_enabled
    }

    fn has_constant_fill(&self) -> bool {
        self.ground.borrow().base.has_constant_fill()
    }

    fn fill_material_property(&self) -> Option<ComponentRef> {
        self.ground.borrow().base.material_property.clone()
    }

    fn outline_enabled(&self) -> bool {
        self.ground.borrow().base.outline_enabled
    }

    fn has_constant_outline(&self) -> bool {
        self.ground.borrow().base.has_constant_outline()
    }

    fn outline_color_property(&self) -> Option<PropertyInput> {
        self.ground.borrow().base.outline_color_property.clone()
    }

    fn outline_width(&self) -> f64 {
        self.ground.borrow().base.outline_width
    }

    fn shadows_property(&self) -> Option<PropertyInput> {
        self.ground.borrow().base.shadows_property.clone()
    }

    fn distance_display_condition_property(&self) -> Option<PropertyInput> {
        self.ground.borrow().base.distance_display_condition_property.clone()
    }

    fn classification_type_property(&self) -> Option<PropertyInput> {
        self.ground.borrow().base.classification_type_property.clone()
    }

    fn terrain_offset_property(&self) -> Option<Rc<TerrainOffsetProperty>> {
        self.ground.borrow().terrain_offset_property()
    }

    fn z_index(&self) -> Option<PropertyInput> {
        self.ground.borrow().z_index()
    }

    fn is_dynamic(&self) -> bool {
        self.ground.borrow().base.dynamic
    }

    fn is_closed(&self) -> bool {
        self.ground.borrow().base.is_closed
    }

    fn on_terrain(&self) -> bool {
        self.ground.borrow().base.on_terrain
    }

    fn geometry_changed(&self) -> &super::ChangeEvent<GeometryChangedEvent> {
        &self.geometry_changed
    }

    fn is_filled(&self, time: &JulianDate) -> Result<bool, RuntimeError> {
        self.ground.borrow().base.is_filled(time)
    }

    fn is_outline_visible(&self, time: &JulianDate) -> Result<bool, RuntimeError> {
        self.ground.borrow().base.is_outline_visible(time)
    }

    fn create_fill_geometry_instance(&self, time: &JulianDate) -> Result<GeometryInstance, RuntimeError> {
        create_fill_instance(&self.ground.borrow(), &self.options.borrow(), time)
    }

    fn create_outline_geometry_instance(&self, time: &JulianDate) -> Result<GeometryInstance, RuntimeError> {
        create_outline_instance(&self.ground.borrow(), &self.options.borrow(), time)
    }

    fn create_dynamic_updater(
        &self,
        primitives: Rc<RefCell<PrimitiveCollection>>,
        ground_primitives: Rc<RefCell<OrderedGroundPrimitiveCollection>>,
    ) -> Result<Box<dyn DynamicGeometryUpdater>, RuntimeError> {
        if !self.ground.borrow().base.dynamic {
            return Err(RuntimeError::new("This instance does not represent dynamic geometry."));
        }
        let hooks = DynamicEllipseHooks { ground: Rc::clone(&self.ground), options: Rc::clone(&self.options) };
        Ok(Box::new(StandardDynamicGeometryUpdater::new(hooks, primitives, ground_primitives)))
    }

    fn bounding_rectangle(&self) -> Option<Rectangle> {
        let ellipsoid = self.ground.borrow().base.scene.ellipsoid();
        compute_ellipse_rectangle(&self.options.borrow(), &ellipsoid)
    }

    fn on_entity_property_changed(&mut self, entity: &Entity, property_name: &str) {
        let options_rc = Rc::clone(&self.options);
        {
            let mut ground = self.ground.borrow_mut();
            ground.base.on_entity_property_changed_with(
                entity,
                property_name,
                |entity, ellipse| {
                    entity.get_property("position").is_none()
                        || ellipse.get_property("semiMajorAxis").is_none()
                        || ellipse.get_property("semiMinorAxis").is_none()
                        || GeometryUpdaterBase::default_is_hidden(ellipse)
                },
                GroundGeometryUpdater::is_on_terrain,
                |base, entity, ellipse| {
                    !is_slot_constant(entity.get_property("position").as_ref())
                        || !is_slot_constant(ellipse.get_property("semiMajorAxis").as_ref())
                        || !is_slot_constant(ellipse.get_property("semiMinorAxis").as_ref())
                        || !is_slot_constant(ellipse.get_property("rotation").as_ref())
                        || !is_slot_constant(ellipse.get_property("height").as_ref())
                        || !is_slot_constant(ellipse.get_property("extrudedHeight").as_ref())
                        || !is_slot_constant(ellipse.get_property("granularity").as_ref())
                        || !is_slot_constant(ellipse.get_property("stRotation").as_ref())
                        || !is_slot_constant(ellipse.get_property("outlineWidth").as_ref())
                        || !is_slot_constant(ellipse.get_property("numberOfVerticalLines").as_ref())
                        || !is_slot_constant(ellipse.get_property("zIndex").as_ref())
                        || (base.on_terrain
                            && base
                                .material_property
                                .as_ref()
                                .is_some_and(|m| !m.is_constant() && !is_color_material_component(m)))
                },
                |base, entity, ellipse| {
                    let min_time = iso8601::minimum_value();
                    let ellipsoid = base.scene.ellipsoid();
                    let mut opts = options_rc.borrow_mut();
                    opts.vertex_format = if base.material_property.as_ref().is_some_and(is_color_material_component) {
                        PerInstanceColorAppearance::VERTEX_FORMAT
                    } else {
                        MaterialSupport::Textured.vertex_format()
                    };
                    let _ = populate_options(&mut opts, entity, ellipse, &min_time, &ellipsoid);
                },
                || {
                    let opts = options_rc.borrow();
                    GroundGeometryUpdater::get_is_closed(opts.height, opts.extruded_height)
                },
            );

            let ent = Rc::clone(&self.entity);
            ground.finish_property_changed(property_name, move |time| ent.position_at(time).ok().flatten());
        }
    }

    fn is_destroyed(&self) -> bool {
        self.ground.borrow().base.destroyed
    }

    fn destroy(&mut self) {
        self.ground.borrow_mut().destroy();
    }
}

struct DynamicEllipseHooks {
    ground: Rc<RefCell<GroundGeometryUpdater>>,
    options: Rc<RefCell<EllipseOptions>>,
}

impl DynamicGeometryHooks for DynamicEllipseHooks {
    fn entity(&self) -> EntityRef {
        Rc::clone(&self.ground.borrow().base.entity)
    }

    fn geometry_property_name(&self) -> &'static str {
        "ellipse"
    }

    fn on_terrain(&self) -> bool {
        self.ground.borrow().base.on_terrain
    }

    fn supports_materials_for_entities_on_terrain(&self) -> bool {
        self.ground.borrow().base.supports_materials_for_entities_on_terrain
    }

    fn shadows_property(&self) -> Option<PropertyInput> {
        self.ground.borrow().base.shadows_property.clone()
    }

    fn classification_type_property(&self) -> Option<PropertyInput> {
        self.ground.borrow().base.classification_type_property.clone()
    }

    fn fill_material_property(&self) -> Option<ComponentRef> {
        self.ground.borrow().base.material_property.clone()
    }

    fn z_index(&self) -> Option<PropertyInput> {
        self.ground.borrow().z_index()
    }

    fn clamp_line_width(&self, width: f64) -> f64 {
        self.ground.borrow().base.scene.clamp_line_width(width)
    }

    fn set_options(
        &mut self,
        entity: &Entity,
        geometry: &dyn EntityComponent,
        time: &JulianDate,
    ) -> Result<(), RuntimeError> {
        let ellipsoid = self.ground.borrow().base.scene.ellipsoid();
        populate_options(&mut self.options.borrow_mut(), entity, geometry, time, &ellipsoid)
    }

    fn is_hidden(
        &self,
        entity: &Entity,
        geometry: &dyn EntityComponent,
        time: &JulianDate,
    ) -> Result<bool, RuntimeError> {
        let opts = self.options.borrow();
        Ok(opts.center.is_none()
            || opts.semi_major_axis.is_none()
            || opts.semi_minor_axis.is_none()
            || dynamic_default_is_hidden(entity, geometry, time)?)
    }

    fn get_is_closed(&self) -> bool {
        let opts = self.options.borrow();
        GroundGeometryUpdater::get_is_closed(opts.height, opts.extruded_height)
    }

    fn set_vertex_format(&mut self, vertex_format: VertexFormat) {
        self.options.borrow_mut().vertex_format = vertex_format;
    }

    fn create_fill_geometry_instance(&self, time: &JulianDate) -> Result<GeometryInstance, RuntimeError> {
        create_fill_instance(&self.ground.borrow(), &self.options.borrow(), time)
    }

    fn create_outline_geometry_instance(&self, time: &JulianDate) -> Result<GeometryInstance, RuntimeError> {
        create_outline_instance(&self.ground.borrow(), &self.options.borrow(), time)
    }
}
