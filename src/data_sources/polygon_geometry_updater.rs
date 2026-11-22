//! CesiumJS `PolygonGeometryUpdater` and `DynamicPolygonGeometryUpdater`.
use super::{
    dynamic_geometry_updater::{dynamic_default_is_hidden, DynamicGeometryHooks, StandardDynamicGeometryUpdater},
    geometry_updater::{
        eval_arc_type, eval_polygon_hierarchy_opt, eval_texcoord_hierarchy_opt, is_color_material_component,
        GeometryChangedEvent, GeometryScene, GeometryUpdater, GeometryUpdaterBase,
    },
    ground_geometry_updater::{GroundExtrudedHeight, GroundGeometryUpdater},
    visualizer_values::*,
    ComponentRef, DynamicGeometryUpdater, Entity, EntityComponent, EntityRef, PropertyInput, TerrainOffsetProperty,
};
use crate::{
    core::{
        approximate_terrain_heights::DEFAULT_MIN_TERRAIN_HEIGHT, iso8601, math::RADIANS_PER_DEGREE,
        ApproximateTerrainHeights, ArcType, Cartesian2, Cartesian3, CoplanarPolygonGeometry,
        CoplanarPolygonGeometryOptions, CoplanarPolygonOutlineGeometry, Ellipsoid, EllipsoidTangentPlane,
        GeometryInstance, GeometryOffsetAttribute, JulianDate, Matrix4, PolygonGeometry, PolygonGeometryOptions,
        PolygonHierarchy, PolygonOutlineGeometry, PolygonOutlineOptions, Rectangle, RuntimeError,
        TextureCoordinateHierarchy, VertexFormat,
    },
    scene::{
        HeightReference, MaterialSupport, OrderedGroundPrimitiveCollection, PerInstanceColorAppearance,
        PrimitiveCollection,
    },
};
use std::{any::Any, cell::RefCell, rc::Rc};

const OBSERVED_PROPERTIES: &[&str] = &["availability", "polygon"];

#[derive(Clone, Debug)]
struct PolygonOptions {
    vertex_format: VertexFormat,
    polygon_hierarchy: Option<PolygonHierarchy>,
    per_position_height: bool,
    close_top: bool,
    close_bottom: bool,
    height: Option<f64>,
    extruded_height: Option<f64>,
    granularity: Option<f64>,
    st_rotation: Option<f64>,
    offset_attribute: Option<GeometryOffsetAttribute>,
    arc_type: ArcType,
    texture_coordinates: Option<TextureCoordinateHierarchy>,
}

impl Default for PolygonOptions {
    fn default() -> Self {
        Self {
            vertex_format: VertexFormat::DEFAULT,
            polygon_hierarchy: None,
            per_position_height: false,
            close_top: true,
            close_bottom: true,
            height: None,
            extruded_height: None,
            granularity: None,
            st_rotation: None,
            offset_attribute: None,
            arc_type: ArcType::Geodesic,
            texture_coordinates: None,
        }
    }
}

fn compute_polygon_rectangle(opts: &PolygonOptions, ellipsoid: &Ellipsoid) -> Option<Rectangle> {
    let hierarchy = opts.polygon_hierarchy.as_ref()?;
    if hierarchy.positions.is_empty() {
        return None;
    }
    Some(PolygonGeometry::compute_rectangle_from_positions(&hierarchy.positions, ellipsoid, opts.arc_type))
}

fn compute_polygon_center(entity: &Entity, ellipsoid: &Ellipsoid, time: &JulianDate) -> Option<Cartesian3> {
    let polygon = entity.get_property("polygon").and_then(|p| p.component())?;
    let hierarchy = eval_polygon_hierarchy_opt(polygon.get_property("hierarchy").as_ref(), time).ok().flatten()?;
    if hierarchy.positions.is_empty() {
        return None;
    }
    let tangent_plane = EllipsoidTangentPlane::from_points(&hierarchy.positions, ellipsoid)?;
    let positions_2d = tangent_plane.project_points_onto_plane(&hierarchy.positions);
    let length = positions_2d.len();
    if length == 0 {
        return None;
    }
    let mut area = 0.0;
    let mut j = length - 1;
    let mut centroid_2d = Cartesian2::ZERO;
    for (i, p1) in positions_2d.iter().enumerate() {
        let p2 = positions_2d[j];
        let f = p1.x * p2.y - p2.x * p1.y;
        let sum = p1.add(p2).multiply_by_scalar(f);
        centroid_2d = centroid_2d.add(sum);
        area += f;
        j = i;
    }
    let a = 1.0 / (area * 3.0);
    centroid_2d = centroid_2d.multiply_by_scalar(a);
    Some(tangent_plane.project_point_onto_ellipsoid(centroid_2d))
}

fn populate_options(
    opts: &mut PolygonOptions,
    polygon: &dyn EntityComponent,
    time: &JulianDate,
    ellipsoid: &Ellipsoid,
) -> Result<(), RuntimeError> {
    let hierarchy_value = eval_polygon_hierarchy_opt(polygon.get_property("hierarchy").as_ref(), time)?;
    let mut height_value = eval_f64_opt(polygon.get_property("height").as_ref(), time)?;
    let hr_value =
        eval_height_reference(polygon.get_property("heightReference").as_ref(), time, HeightReference::None)?;
    let extruded_height_value = eval_f64_opt(polygon.get_property("extrudedHeight").as_ref(), time)?;
    let ehr_value =
        eval_height_reference(polygon.get_property("extrudedHeightReference").as_ref(), time, HeightReference::None)?;
    let per_position_height_value = eval_bool(polygon.get_property("perPositionHeight").as_ref(), time, false)?;

    height_value = GroundGeometryUpdater::get_geometry_height(height_value, hr_value);

    let mut offset_attribute = None;
    if per_position_height_value {
        if height_value.is_some() || hr_value != HeightReference::None {
            height_value = None;
        }
    } else {
        if extruded_height_value.is_some() && height_value.is_none() {
            height_value = Some(0.0);
        }
        offset_attribute = GroundGeometryUpdater::compute_geometry_offset_attribute(
            height_value,
            Some(hr_value),
            extruded_height_value,
            Some(ehr_value),
        );
    }

    opts.polygon_hierarchy = hierarchy_value;
    opts.granularity = eval_f64_opt(polygon.get_property("granularity").as_ref(), time)?;
    opts.st_rotation = eval_f64_opt(polygon.get_property("stRotation").as_ref(), time)?;
    opts.per_position_height = per_position_height_value;
    opts.close_top = eval_bool(polygon.get_property("closeTop").as_ref(), time, true)?;
    opts.close_bottom = eval_bool(polygon.get_property("closeBottom").as_ref(), time, true)?;
    opts.offset_attribute = offset_attribute;
    opts.height = height_value;
    opts.arc_type = eval_arc_type(polygon.get_property("arcType").as_ref(), time, ArcType::Geodesic)?;
    opts.texture_coordinates = eval_texcoord_hierarchy_opt(polygon.get_property("textureCoordinates").as_ref(), time)?;

    opts.extruded_height = match GroundGeometryUpdater::get_geometry_extruded_height(extruded_height_value, ehr_value) {
        Some(GroundExtrudedHeight::Height(h)) => Some(h),
        Some(GroundExtrudedHeight::ClampToGround) => {
            let min_h = compute_polygon_rectangle(opts, ellipsoid)
                .and_then(|rect| ApproximateTerrainHeights::bundled().get_minimum_maximum_heights(&rect, ellipsoid))
                .map_or(DEFAULT_MIN_TERRAIN_HEIGHT, |(min, _)| min);
            Some(min_h)
        }
        None => None,
    };
    Ok(())
}

fn is_closed_for_options(options: &PolygonOptions) -> bool {
    let height = options.height;
    let extruded_height = options.extruded_height;
    let is_extruded = extruded_height.is_some() && extruded_height != height;
    !options.per_position_height
        && ((!is_extruded && height == Some(0.0)) || (is_extruded && options.close_top && options.close_bottom))
}

fn create_fill_instance(
    ground: &GroundGeometryUpdater,
    options: &PolygonOptions,
    time: &JulianDate,
) -> Result<GeometryInstance, RuntimeError> {
    let base = &ground.base;
    if !base.fill_enabled {
        return Err(RuntimeError::new("This instance does not represent a filled geometry."));
    }
    let hierarchy =
        options.polygon_hierarchy.clone().ok_or_else(|| RuntimeError::new("polygon hierarchy is required."))?;

    let geometry = if options.per_position_height && options.extruded_height.is_none() {
        let mut geom_opts = CoplanarPolygonGeometryOptions::new(hierarchy);
        geom_opts.ellipsoid = base.scene.ellipsoid();
        geom_opts.vertex_format = options.vertex_format;
        geom_opts.st_rotation = options.st_rotation.unwrap_or(0.0);
        geom_opts.texture_coordinates = options.texture_coordinates.clone();
        CoplanarPolygonGeometry::new(geom_opts).create_geometry()
    } else {
        let mut geom_opts = PolygonGeometryOptions::new(hierarchy);
        geom_opts.ellipsoid = base.scene.ellipsoid();
        geom_opts.vertex_format = options.vertex_format;
        geom_opts.height = options.height.unwrap_or(0.0);
        geom_opts.extruded_height = options.extruded_height;
        geom_opts.granularity = options.granularity.unwrap_or(RADIANS_PER_DEGREE);
        geom_opts.st_rotation = options.st_rotation.unwrap_or(0.0);
        geom_opts.per_position_height = options.per_position_height;
        geom_opts.close_top = options.close_top;
        geom_opts.close_bottom = options.close_bottom;
        geom_opts.arc_type = options.arc_type;
        geom_opts.offset_attribute = options.offset_attribute;
        geom_opts.texture_coordinates = options.texture_coordinates.clone();
        PolygonGeometry::new(geom_opts).create_geometry()
    };

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
    options: &PolygonOptions,
    time: &JulianDate,
) -> Result<GeometryInstance, RuntimeError> {
    let base = &ground.base;
    if !base.outline_enabled {
        return Err(RuntimeError::new("This instance does not represent an outlined geometry."));
    }
    let hierarchy =
        options.polygon_hierarchy.clone().ok_or_else(|| RuntimeError::new("polygon hierarchy is required."))?;

    let geometry = if options.per_position_height && options.extruded_height.is_none() {
        CoplanarPolygonOutlineGeometry::new(hierarchy).create_geometry()
    } else {
        let mut geom_opts = PolygonOutlineOptions::new(hierarchy);
        geom_opts.ellipsoid = base.scene.ellipsoid();
        geom_opts.height = options.height.unwrap_or(0.0);
        geom_opts.extruded_height = options.extruded_height;
        geom_opts.granularity = options.granularity.unwrap_or(RADIANS_PER_DEGREE);
        geom_opts.per_position_height = options.per_position_height;
        geom_opts.arc_type = options.arc_type;
        geom_opts.offset_attribute = options.offset_attribute;
        PolygonOutlineGeometry::new(geom_opts).create_geometry()
    };

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

/// Port of CesiumJS `PolygonGeometryUpdater`.
pub struct PolygonGeometryUpdater {
    id: String,
    entity: EntityRef,
    geometry_changed: super::ChangeEvent<GeometryChangedEvent>,
    ground: Rc<RefCell<GroundGeometryUpdater>>,
    options: Rc<RefCell<PolygonOptions>>,
}

impl PolygonGeometryUpdater {
    pub fn new(entity: EntityRef, scene: Rc<dyn GeometryScene>) -> Self {
        let ground = GroundGeometryUpdater::new(Rc::clone(&entity), scene, "polygon", OBSERVED_PROPERTIES);
        let id = ground.base.id.clone();
        let geometry_changed = ground.base.geometry_changed.clone();
        let mut updater = Self {
            id,
            entity: Rc::clone(&entity),
            geometry_changed,
            ground: Rc::new(RefCell::new(ground)),
            options: Rc::new(RefCell::new(PolygonOptions::default())),
        };
        updater.on_entity_property_changed(&entity, "polygon");
        updater
    }

    pub fn compute_center(&self, time: &JulianDate) -> Option<Cartesian3> {
        let ellipsoid = self.ground.borrow().base.scene.ellipsoid();
        compute_polygon_center(&self.entity, &ellipsoid, time)
    }
}

impl GeometryUpdater for PolygonGeometryUpdater {
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
        if !self.is_dynamic() {
            return Err(RuntimeError::new("This instance does not represent dynamic geometry."));
        }
        Ok(Box::new(StandardDynamicGeometryUpdater::new(
            DynamicPolygonHooks { ground: Rc::clone(&self.ground), options: Rc::clone(&self.options) },
            primitives,
            ground_primitives,
        )))
    }

    fn bounding_rectangle(&self) -> Option<Rectangle> {
        let ellipsoid = self.ground.borrow().base.scene.ellipsoid();
        compute_polygon_rectangle(&self.options.borrow(), &ellipsoid)
    }

    fn on_entity_property_changed(&mut self, entity: &Entity, property_name: &str) {
        let options_ref = Rc::clone(&self.options);
        {
            let mut ground = self.ground.borrow_mut();
            ground.base.on_entity_property_changed_with(
                entity,
                property_name,
                |_e, polygon| {
                    polygon.get_property("hierarchy").is_none() || GeometryUpdaterBase::default_is_hidden(polygon)
                },
                |base, e, polygon| {
                    let on_terrain = GroundGeometryUpdater::is_on_terrain(base, e, polygon);
                    let per_pos_prop = polygon.get_property("perPositionHeight");
                    let per_pos_enabled = match per_pos_prop.as_ref() {
                        Some(slot) if is_slot_constant(Some(slot)) => {
                            eval_bool(Some(slot), &iso8601::minimum_value(), false).unwrap_or(false)
                        }
                        Some(_) => true,
                        None => false,
                    };
                    on_terrain && !per_pos_enabled
                },
                |base, _e, polygon| {
                    !is_slot_constant(polygon.get_property("hierarchy").as_ref())
                        || !is_slot_constant(polygon.get_property("height").as_ref())
                        || !is_slot_constant(polygon.get_property("extrudedHeight").as_ref())
                        || !is_slot_constant(polygon.get_property("granularity").as_ref())
                        || !is_slot_constant(polygon.get_property("stRotation").as_ref())
                        || !is_slot_constant(polygon.get_property("textureCoordinates").as_ref())
                        || !is_slot_constant(polygon.get_property("outlineWidth").as_ref())
                        || !is_slot_constant(polygon.get_property("perPositionHeight").as_ref())
                        || !is_slot_constant(polygon.get_property("closeTop").as_ref())
                        || !is_slot_constant(polygon.get_property("closeBottom").as_ref())
                        || !is_slot_constant(polygon.get_property("zIndex").as_ref())
                        || !is_slot_constant(polygon.get_property("arcType").as_ref())
                        || (base.on_terrain
                            && base
                                .material_property
                                .as_ref()
                                .is_some_and(|m| !m.is_constant() && !is_color_material_component(m)))
                },
                |base, _e, polygon| {
                    let is_color = base.material_property.as_ref().is_some_and(is_color_material_component);
                    let mut opts = options_ref.borrow_mut();
                    opts.vertex_format = if is_color {
                        PerInstanceColorAppearance::VERTEX_FORMAT
                    } else {
                        MaterialSupport::Textured.vertex_format()
                    };
                    let min_time = iso8601::minimum_value();
                    let ellipsoid = base.scene.ellipsoid();
                    let _ = populate_options(&mut opts, polygon, &min_time, &ellipsoid);
                },
                || is_closed_for_options(&options_ref.borrow()),
            );
        }

        let center_entity = Rc::clone(&self.entity);
        let ellipsoid = self.ground.borrow().base.scene.ellipsoid();
        self.ground.borrow_mut().finish_property_changed(property_name, move |time| {
            compute_polygon_center(&center_entity, &ellipsoid, time)
        });
    }

    fn is_destroyed(&self) -> bool {
        self.ground.borrow().base.destroyed
    }

    fn destroy(&mut self) {
        self.ground.borrow_mut().destroy();
    }
}

struct DynamicPolygonHooks {
    ground: Rc<RefCell<GroundGeometryUpdater>>,
    options: Rc<RefCell<PolygonOptions>>,
}

impl DynamicGeometryHooks for DynamicPolygonHooks {
    fn entity(&self) -> EntityRef {
        Rc::clone(&self.ground.borrow().base.entity)
    }

    fn geometry_property_name(&self) -> &'static str {
        "polygon"
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
        _entity: &Entity,
        geometry: &dyn EntityComponent,
        time: &JulianDate,
    ) -> Result<(), RuntimeError> {
        let ellipsoid = self.ground.borrow().base.scene.ellipsoid();
        let mut opts = self.options.borrow_mut();
        populate_options(&mut opts, geometry, time, &ellipsoid)
    }

    fn is_hidden(
        &self,
        entity: &Entity,
        geometry: &dyn EntityComponent,
        time: &JulianDate,
    ) -> Result<bool, RuntimeError> {
        let opts = self.options.borrow();
        let has_hierarchy = opts.polygon_hierarchy.as_ref().is_some_and(|h| !h.positions.is_empty());
        Ok(!has_hierarchy || dynamic_default_is_hidden(entity, geometry, time)?)
    }

    fn get_is_closed(&self) -> bool {
        is_closed_for_options(&self.options.borrow())
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
