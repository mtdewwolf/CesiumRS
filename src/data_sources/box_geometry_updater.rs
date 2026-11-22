//! CesiumJS `BoxGeometryUpdater` and `DynamicBoxGeometryUpdater`.
use super::{
    dynamic_geometry_updater::{dynamic_default_is_hidden, DynamicGeometryHooks, StandardDynamicGeometryUpdater},
    geometry_updater::{
        is_color_material_component, GeometryChangedEvent, GeometryScene, GeometryUpdater, GeometryUpdaterBase,
        HeightReferenceSlotProperty,
    },
    height_reference_on_entity_property_changed::{
        height_reference_on_entity_property_changed, CenterPositionCallback, HeightReferenceUpdaterHost,
    },
    visualizer_values::*,
    ComponentRef, DynamicGeometryUpdater, Entity, EntityComponent, EntityRef, HeightReferenceOptions, PropertyInput,
    PropertyRef, TerrainOffsetProperty,
};
use crate::{
    core::{
        iso8601, BoxGeometry, BoxOutlineGeometry, Cartesian3, GeometryInstance, GeometryOffsetAttribute, JulianDate,
        Matrix4, RuntimeError, VertexFormat,
    },
    scene::{
        HeightReference, MaterialSupport, OrderedGroundPrimitiveCollection, PerInstanceColorAppearance,
        PrimitiveCollection,
    },
};
use std::{any::Any, cell::RefCell, rc::Rc};

const OBSERVED_PROPERTIES: &[&str] = &["availability", "position", "orientation", "box"];

#[derive(Clone, Debug, Default)]
struct BoxGeometryOptions {
    vertex_format: VertexFormat,
    dimensions: Option<Cartesian3>,
    offset_attribute: Option<GeometryOffsetAttribute>,
}

fn create_fill_instance(
    base: &GeometryUpdaterBase,
    options: &BoxGeometryOptions,
    terrain_offset: Option<&Rc<TerrainOffsetProperty>>,
    time: &JulianDate,
) -> Result<GeometryInstance, RuntimeError> {
    if !base.fill_enabled {
        return Err(RuntimeError::new("This instance does not represent a filled geometry."));
    }
    let dimensions = options.dimensions.ok_or_else(|| RuntimeError::new("box dimensions are required."))?;
    let box_geom = BoxGeometry::from_dimensions(dimensions, options.vertex_format, options.offset_attribute)
        .ok_or_else(|| RuntimeError::new("Invalid BoxGeometry options."))?;
    let geometry = box_geom.create_geometry();

    let hr_prop = base
        .entity
        .get_property("box")
        .and_then(|p| p.component())
        .and_then(|c| c.get_property("heightReference"))
        .map(HeightReferenceSlotProperty::wrap);
    let ellipsoid = base.scene.ellipsoid();
    let model_matrix = base
        .entity
        .compute_model_matrix_for_height_reference(
            time,
            HeightReferenceOptions {
                property: hr_prop.as_deref(),
                height_offset: dimensions.z * 0.5,
                ellipsoid: &ellipsoid,
            },
            None,
        )?
        .unwrap_or(Matrix4::IDENTITY);

    let attributes = base.build_fill_attributes(time, options.offset_attribute, terrain_offset)?;
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
    options: &BoxGeometryOptions,
    terrain_offset: Option<&Rc<TerrainOffsetProperty>>,
    time: &JulianDate,
) -> Result<GeometryInstance, RuntimeError> {
    if !base.outline_enabled {
        return Err(RuntimeError::new("This instance does not represent an outlined geometry."));
    }
    let dimensions = options.dimensions.ok_or_else(|| RuntimeError::new("box dimensions are required."))?;
    let outline_geom = BoxOutlineGeometry::from_dimensions(dimensions, options.offset_attribute)
        .ok_or_else(|| RuntimeError::new("Invalid BoxOutlineGeometry options."))?;
    let geometry = outline_geom.create_geometry();

    let hr_prop = base
        .entity
        .get_property("box")
        .and_then(|p| p.component())
        .and_then(|c| c.get_property("heightReference"))
        .map(HeightReferenceSlotProperty::wrap);
    let ellipsoid = base.scene.ellipsoid();
    let model_matrix = base
        .entity
        .compute_model_matrix_for_height_reference(
            time,
            HeightReferenceOptions {
                property: hr_prop.as_deref(),
                height_offset: dimensions.z * 0.5,
                ellipsoid: &ellipsoid,
            },
            None,
        )?
        .unwrap_or(Matrix4::IDENTITY);

    let attributes = base.build_outline_attributes(time, options.offset_attribute, terrain_offset)?;
    Ok(GeometryInstance {
        geometry,
        model_matrix,
        id: Some(base.entity.id().to_owned()),
        attributes,
        west_hemisphere_geometry: None,
        east_hemisphere_geometry: None,
    })
}

/// Port of CesiumJS `BoxGeometryUpdater`.
pub struct BoxGeometryUpdater {
    id: String,
    entity: EntityRef,
    geometry_changed: super::ChangeEvent<GeometryChangedEvent>,
    base: Rc<RefCell<GeometryUpdaterBase>>,
    options: Rc<RefCell<BoxGeometryOptions>>,
    terrain_offset: Rc<RefCell<Option<Rc<TerrainOffsetProperty>>>>,
}

impl BoxGeometryUpdater {
    pub fn new(entity: EntityRef, scene: Rc<dyn GeometryScene>) -> Self {
        let base = GeometryUpdaterBase::new(Rc::clone(&entity), scene, "box", OBSERVED_PROPERTIES);
        let id = base.id.clone();
        let geometry_changed = base.geometry_changed.clone();
        let mut updater = Self {
            id,
            entity: Rc::clone(&entity),
            geometry_changed,
            base: Rc::new(RefCell::new(base)),
            options: Rc::new(RefCell::new(BoxGeometryOptions::default())),
            terrain_offset: Rc::new(RefCell::new(None)),
        };
        GeometryUpdater::on_entity_property_changed(&mut updater, &entity, "box");
        updater
    }

    fn base_on_property_changed(&mut self, entity: &Entity, property_name: &str) {
        let options_rc = Rc::clone(&self.options);
        self.base.borrow_mut().on_entity_property_changed_with(
            entity,
            property_name,
            |entity, box_g| {
                box_g.get_property("dimensions").is_none()
                    || entity.get_property("position").is_none()
                    || GeometryUpdaterBase::default_is_hidden(box_g)
            },
            |_base, _entity, _box_g| false,
            |_base, entity, box_g| {
                !is_slot_constant(entity.get_property("position").as_ref())
                    || !is_slot_constant(entity.get_property("orientation").as_ref())
                    || !is_slot_constant(box_g.get_property("dimensions").as_ref())
                    || !is_slot_constant(box_g.get_property("outlineWidth").as_ref())
            },
            |base, _entity, box_g| {
                let min_time = iso8601::minimum_value();
                let hr = eval_height_reference(
                    box_g.get_property("heightReference").as_ref(),
                    &min_time,
                    HeightReference::None,
                )
                .unwrap_or(HeightReference::None);
                let mut opts = options_rc.borrow_mut();
                opts.vertex_format = if base.material_property.as_ref().is_some_and(is_color_material_component) {
                    PerInstanceColorAppearance::VERTEX_FORMAT
                } else {
                    MaterialSupport::Textured.vertex_format()
                };
                opts.dimensions =
                    eval_cartesian3_opt(box_g.get_property("dimensions").as_ref(), &min_time).ok().flatten();
                opts.offset_attribute =
                    if hr != HeightReference::None { Some(GeometryOffsetAttribute::All) } else { None };
            },
            || true,
        );
    }
}

impl HeightReferenceUpdaterHost for BoxGeometryUpdater {
    type TerrainOffset = Rc<TerrainOffsetProperty>;

    fn on_entity_property_changed(&mut self, entity: &Entity, property_name: &str) {
        self.base_on_property_changed(entity, property_name);
    }

    fn observed_property_names(&self) -> &[&str] {
        OBSERVED_PROPERTIES
    }

    fn entity(&self) -> &Entity {
        &self.entity
    }

    fn geometry_property_name(&self) -> &str {
        "box"
    }

    fn is_dynamic(&self) -> bool {
        self.base.borrow().dynamic
    }

    fn take_terrain_offset(&mut self) -> Option<Self::TerrainOffset> {
        self.terrain_offset.borrow_mut().take()
    }

    fn destroy_terrain_offset(&mut self, offset: Self::TerrainOffset) {
        offset.destroy();
    }

    fn compute_center(&self) -> CenterPositionCallback {
        let entity = Rc::clone(&self.entity);
        Rc::new(move |time| {
            let fallback = iso8601::minimum_value();
            let t = time.unwrap_or(&fallback);
            entity.position_at(t).ok().flatten()
        })
    }

    fn create_terrain_offset(
        &mut self,
        center_position: PropertyRef<Cartesian3>,
        height_reference: PropertyInput,
    ) -> Self::TerrainOffset {
        let scene = Rc::clone(&self.base.borrow().scene).as_terrain_offset_scene();
        let hr = Some(HeightReferenceSlotProperty::wrap(height_reference));
        TerrainOffsetProperty::new(scene, center_position, hr, None)
    }

    fn set_terrain_offset(&mut self, offset: Option<Self::TerrainOffset>) {
        *self.terrain_offset.borrow_mut() = offset;
    }
}

impl GeometryUpdater for BoxGeometryUpdater {
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

    fn terrain_offset_property(&self) -> Option<Rc<TerrainOffsetProperty>> {
        self.terrain_offset.borrow().clone()
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
        let base = self.base.borrow();
        let options = self.options.borrow();
        let offset = self.terrain_offset.borrow();
        create_fill_instance(&base, &options, offset.as_ref(), time)
    }

    fn create_outline_geometry_instance(&self, time: &JulianDate) -> Result<GeometryInstance, RuntimeError> {
        let base = self.base.borrow();
        let options = self.options.borrow();
        let offset = self.terrain_offset.borrow();
        create_outline_instance(&base, &options, offset.as_ref(), time)
    }

    fn create_dynamic_updater(
        &self,
        primitives: Rc<RefCell<PrimitiveCollection>>,
        ground_primitives: Rc<RefCell<OrderedGroundPrimitiveCollection>>,
    ) -> Result<Box<dyn DynamicGeometryUpdater>, RuntimeError> {
        if !self.base.borrow().dynamic {
            return Err(RuntimeError::new("This instance does not represent dynamic geometry."));
        }
        let hooks = DynamicBoxHooks {
            base: Rc::clone(&self.base),
            options: Rc::clone(&self.options),
            terrain_offset: Rc::clone(&self.terrain_offset),
        };
        Ok(Box::new(StandardDynamicGeometryUpdater::new(hooks, primitives, ground_primitives)))
    }

    fn on_entity_property_changed(&mut self, entity: &Entity, property_name: &str) {
        height_reference_on_entity_property_changed(self, entity, property_name);
    }

    fn is_destroyed(&self) -> bool {
        self.base.borrow().destroyed
    }

    fn destroy(&mut self) {
        if let Some(offset) = self.terrain_offset.borrow_mut().take() {
            offset.destroy();
        }
        self.base.borrow_mut().destroy();
    }
}

struct DynamicBoxHooks {
    base: Rc<RefCell<GeometryUpdaterBase>>,
    options: Rc<RefCell<BoxGeometryOptions>>,
    terrain_offset: Rc<RefCell<Option<Rc<TerrainOffsetProperty>>>>,
}

impl DynamicGeometryHooks for DynamicBoxHooks {
    fn entity(&self) -> EntityRef {
        Rc::clone(&self.base.borrow().entity)
    }

    fn geometry_property_name(&self) -> &'static str {
        "box"
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
        let hr = eval_height_reference(geometry.get_property("heightReference").as_ref(), time, HeightReference::None)?;
        let mut opts = self.options.borrow_mut();
        opts.dimensions = eval_cartesian3_opt(geometry.get_property("dimensions").as_ref(), time)?;
        opts.offset_attribute = if hr != HeightReference::None { Some(GeometryOffsetAttribute::All) } else { None };
        Ok(())
    }

    fn is_hidden(
        &self,
        entity: &Entity,
        geometry: &dyn EntityComponent,
        time: &JulianDate,
    ) -> Result<bool, RuntimeError> {
        let position = entity.position_at(time)?;
        let has_dims = self.options.borrow().dimensions.is_some();
        Ok(position.is_none() || !has_dims || dynamic_default_is_hidden(entity, geometry, time)?)
    }

    fn get_is_closed(&self) -> bool {
        true
    }

    fn set_vertex_format(&mut self, vertex_format: VertexFormat) {
        self.options.borrow_mut().vertex_format = vertex_format;
    }

    fn create_fill_geometry_instance(&self, time: &JulianDate) -> Result<GeometryInstance, RuntimeError> {
        let base = self.base.borrow();
        let options = self.options.borrow();
        let offset = self.terrain_offset.borrow();
        create_fill_instance(&base, &options, offset.as_ref(), time)
    }

    fn create_outline_geometry_instance(&self, time: &JulianDate) -> Result<GeometryInstance, RuntimeError> {
        let base = self.base.borrow();
        let options = self.options.borrow();
        let offset = self.terrain_offset.borrow();
        create_outline_instance(&base, &options, offset.as_ref(), time)
    }
}
