//! CesiumJS `EllipsoidGeometryUpdater` and `DynamicEllipsoidGeometryUpdater`
//! (including the 3D unit-sphere scaling fast path).
use super::{
    geometry_updater::{
        ddc_equals, default_color_material, eval_ddc_or_default, eval_terrain_offset_or_zero,
        extract_material_component, instance_alpha, is_color_material_component, GeometryAppearanceKind,
        GeometryBatchPrimitive, GeometryChangedEvent, GeometryScene, GeometryUpdater, GeometryUpdaterBase,
        HeightReferenceSlotProperty,
    },
    height_reference_on_entity_property_changed::{
        height_reference_on_entity_property_changed, CenterPositionCallback, HeightReferenceUpdaterHost,
    },
    visualizer_values::*,
    BoundingSphereState, ComponentRef, DynamicGeometryUpdater, Entity, EntityRef, HeightReferenceOptions,
    MaterialProperty, PropertyInput, PropertyRef, TerrainOffsetProperty,
};
use crate::{
    core::{
        iso8601,
        math::{PI, TWO_PI},
        BoundingSphere, Cartesian3, Color, ColorGeometryInstanceAttribute, DistanceDisplayCondition,
        DistanceDisplayConditionGeometryInstanceAttribute, EllipsoidGeometry, EllipsoidOutlineGeometry,
        GeometryInstance, GeometryOffsetAttribute, JulianDate, Matrix4, OffsetGeometryInstanceAttribute, RuntimeError,
        ShowGeometryInstanceAttribute, VertexFormat,
    },
    scene::{
        HeightReference, Material, MaterialSupport, OrderedGroundPrimitiveCollection, PerInstanceColorAppearance,
        PrimitiveCollection, PrimitiveHandle, SceneMode, ScenePrimitive, ShadowMode,
    },
};
use std::{any::Any, cell::RefCell, rc::Rc};

const OBSERVED_PROPERTIES: &[&str] = &["availability", "position", "orientation", "ellipsoid"];
const UNIT_SPHERE: Cartesian3 = Cartesian3::new(1.0, 1.0, 1.0);

#[derive(Clone, Debug, Default)]
struct EllipsoidOptions {
    vertex_format: VertexFormat,
    radii: Option<Cartesian3>,
    inner_radii: Option<Cartesian3>,
    minimum_clock: Option<f64>,
    maximum_clock: Option<f64>,
    minimum_cone: Option<f64>,
    maximum_cone: Option<f64>,
    stack_partitions: Option<f64>,
    slice_partitions: Option<f64>,
    subdivisions: Option<f64>,
    offset_attribute: Option<GeometryOffsetAttribute>,
}

fn create_fill_instance(
    base: &GeometryUpdaterBase,
    options: &EllipsoidOptions,
    terrain_offset: Option<&Rc<TerrainOffsetProperty>>,
    time: &JulianDate,
    skip_model_matrix: bool,
) -> Result<GeometryInstance, RuntimeError> {
    let radii = options.radii.ok_or_else(|| RuntimeError::new("ellipsoid radii are required."))?;
    let geom = EllipsoidGeometry::new(
        radii,
        options.inner_radii,
        options.minimum_clock.unwrap_or(0.0),
        options.maximum_clock.unwrap_or(TWO_PI),
        options.minimum_cone.unwrap_or(0.0),
        options.maximum_cone.unwrap_or(PI),
        options.stack_partitions.unwrap_or(64.0),
        options.slice_partitions.unwrap_or(64.0),
        options.vertex_format,
        options.offset_attribute,
    )
    .ok_or_else(|| RuntimeError::new("Invalid EllipsoidGeometry options."))?;
    let geometry = geom.create_geometry();

    let model_matrix = if skip_model_matrix {
        Matrix4::IDENTITY
    } else {
        let hr_prop = base
            .entity
            .get_property("ellipsoid")
            .and_then(|p| p.component())
            .and_then(|c| c.get_property("heightReference"))
            .map(HeightReferenceSlotProperty::wrap);
        let ellipsoid = base.scene.ellipsoid();
        base.entity
            .compute_model_matrix_for_height_reference(
                time,
                HeightReferenceOptions {
                    property: hr_prop.as_deref(),
                    height_offset: radii.z * 0.5,
                    ellipsoid: &ellipsoid,
                },
                None,
            )?
            .unwrap_or(Matrix4::IDENTITY)
    };

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
    options: &EllipsoidOptions,
    terrain_offset: Option<&Rc<TerrainOffsetProperty>>,
    time: &JulianDate,
    skip_model_matrix: bool,
) -> Result<GeometryInstance, RuntimeError> {
    let radii = options.radii.ok_or_else(|| RuntimeError::new("ellipsoid radii are required."))?;
    let geom = EllipsoidOutlineGeometry::new(
        radii,
        options.inner_radii,
        options.minimum_clock.unwrap_or(0.0),
        options.maximum_clock.unwrap_or(TWO_PI),
        options.minimum_cone.unwrap_or(0.0),
        options.maximum_cone.unwrap_or(PI),
        options.stack_partitions.unwrap_or(10.0),
        options.slice_partitions.unwrap_or(8.0),
        options.subdivisions.unwrap_or(128.0),
        options.offset_attribute,
    )
    .ok_or_else(|| RuntimeError::new("Invalid EllipsoidOutlineGeometry options."))?;
    let geometry = geom.create_geometry();

    let model_matrix = if skip_model_matrix {
        Matrix4::IDENTITY
    } else {
        let hr_prop = base
            .entity
            .get_property("ellipsoid")
            .and_then(|p| p.component())
            .and_then(|c| c.get_property("heightReference"))
            .map(HeightReferenceSlotProperty::wrap);
        let ellipsoid = base.scene.ellipsoid();
        base.entity
            .compute_model_matrix_for_height_reference(
                time,
                HeightReferenceOptions {
                    property: hr_prop.as_deref(),
                    height_offset: radii.z * 0.5,
                    ellipsoid: &ellipsoid,
                },
                None,
            )?
            .unwrap_or(Matrix4::IDENTITY)
    };

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

/// Port of CesiumJS `EllipsoidGeometryUpdater`.
pub struct EllipsoidGeometryUpdater {
    id: String,
    entity: EntityRef,
    geometry_changed: super::ChangeEvent<GeometryChangedEvent>,
    base: Rc<RefCell<GeometryUpdaterBase>>,
    options: Rc<RefCell<EllipsoidOptions>>,
    terrain_offset: Rc<RefCell<Option<Rc<TerrainOffsetProperty>>>>,
}

impl EllipsoidGeometryUpdater {
    pub fn create_dynamic(
        &self,
        primitives: Rc<RefCell<PrimitiveCollection>>,
    ) -> Result<DynamicEllipsoidGeometryUpdater, RuntimeError> {
        if !self.base.borrow().dynamic {
            return Err(RuntimeError::new("This instance does not represent dynamic geometry."));
        }
        Ok(DynamicEllipsoidGeometryUpdater::new(
            Rc::clone(&self.base),
            Rc::clone(&self.options),
            Rc::clone(&self.terrain_offset),
            primitives,
        ))
    }

    pub fn new(entity: EntityRef, scene: Rc<dyn GeometryScene>) -> Self {
        let base = GeometryUpdaterBase::new(Rc::clone(&entity), scene, "ellipsoid", OBSERVED_PROPERTIES);
        let id = base.id.clone();
        let geometry_changed = base.geometry_changed.clone();
        let mut updater = Self {
            id,
            entity: Rc::clone(&entity),
            geometry_changed,
            base: Rc::new(RefCell::new(base)),
            options: Rc::new(RefCell::new(EllipsoidOptions::default())),
            terrain_offset: Rc::new(RefCell::new(None)),
        };
        GeometryUpdater::on_entity_property_changed(&mut updater, &entity, "ellipsoid");
        updater
    }

    pub fn create_fill_geometry_instance_with_options(
        &self,
        time: &JulianDate,
        skip_model_matrix: bool,
    ) -> Result<GeometryInstance, RuntimeError> {
        let base = self.base.borrow();
        let options = self.options.borrow();
        let offset = self.terrain_offset.borrow();
        create_fill_instance(&base, &options, offset.as_ref(), time, skip_model_matrix)
    }

    pub fn create_outline_geometry_instance_with_options(
        &self,
        time: &JulianDate,
        skip_model_matrix: bool,
    ) -> Result<GeometryInstance, RuntimeError> {
        let base = self.base.borrow();
        let options = self.options.borrow();
        let offset = self.terrain_offset.borrow();
        create_outline_instance(&base, &options, offset.as_ref(), time, skip_model_matrix)
    }

    fn base_on_property_changed(&mut self, entity: &Entity, property_name: &str) {
        let options_rc = Rc::clone(&self.options);
        self.base.borrow_mut().on_entity_property_changed_with(
            entity,
            property_name,
            |entity, ellipsoid| {
                entity.get_property("position").is_none()
                    || ellipsoid.get_property("radii").is_none()
                    || GeometryUpdaterBase::default_is_hidden(ellipsoid)
            },
            |_base, _entity, _ellipsoid| false,
            |_base, entity, ellipsoid| {
                !is_slot_constant(entity.get_property("position").as_ref())
                    || !is_slot_constant(entity.get_property("orientation").as_ref())
                    || !is_slot_constant(ellipsoid.get_property("radii").as_ref())
                    || !is_slot_constant(ellipsoid.get_property("innerRadii").as_ref())
                    || !is_slot_constant(ellipsoid.get_property("stackPartitions").as_ref())
                    || !is_slot_constant(ellipsoid.get_property("slicePartitions").as_ref())
                    || !is_slot_constant(ellipsoid.get_property("outlineWidth").as_ref())
                    || !is_slot_constant(ellipsoid.get_property("minimumClock").as_ref())
                    || !is_slot_constant(ellipsoid.get_property("maximumClock").as_ref())
                    || !is_slot_constant(ellipsoid.get_property("minimumCone").as_ref())
                    || !is_slot_constant(ellipsoid.get_property("maximumCone").as_ref())
                    || !is_slot_constant(ellipsoid.get_property("subdivisions").as_ref())
            },
            |base, _entity, ellipsoid| {
                let min_time = iso8601::minimum_value();
                let hr = eval_height_reference(
                    ellipsoid.get_property("heightReference").as_ref(),
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
                opts.radii = eval_cartesian3_opt(ellipsoid.get_property("radii").as_ref(), &min_time).ok().flatten();
                opts.inner_radii =
                    eval_cartesian3_opt(ellipsoid.get_property("innerRadii").as_ref(), &min_time).ok().flatten();
                opts.minimum_clock =
                    eval_f64_opt(ellipsoid.get_property("minimumClock").as_ref(), &min_time).ok().flatten();
                opts.maximum_clock =
                    eval_f64_opt(ellipsoid.get_property("maximumClock").as_ref(), &min_time).ok().flatten();
                opts.minimum_cone =
                    eval_f64_opt(ellipsoid.get_property("minimumCone").as_ref(), &min_time).ok().flatten();
                opts.maximum_cone =
                    eval_f64_opt(ellipsoid.get_property("maximumCone").as_ref(), &min_time).ok().flatten();
                opts.stack_partitions =
                    eval_f64_opt(ellipsoid.get_property("stackPartitions").as_ref(), &min_time).ok().flatten();
                opts.slice_partitions =
                    eval_f64_opt(ellipsoid.get_property("slicePartitions").as_ref(), &min_time).ok().flatten();
                opts.subdivisions =
                    eval_f64_opt(ellipsoid.get_property("subdivisions").as_ref(), &min_time).ok().flatten();
                opts.offset_attribute =
                    if hr != HeightReference::None { Some(GeometryOffsetAttribute::All) } else { None };
            },
            || true,
        );
    }
}

impl HeightReferenceUpdaterHost for EllipsoidGeometryUpdater {
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
        "ellipsoid"
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

impl GeometryUpdater for EllipsoidGeometryUpdater {
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
        self.create_fill_geometry_instance_with_options(time, false)
    }

    fn create_outline_geometry_instance(&self, time: &JulianDate) -> Result<GeometryInstance, RuntimeError> {
        self.create_outline_geometry_instance_with_options(time, false)
    }

    fn create_dynamic_updater(
        &self,
        primitives: Rc<RefCell<PrimitiveCollection>>,
        _ground_primitives: Rc<RefCell<OrderedGroundPrimitiveCollection>>,
    ) -> Result<Box<dyn DynamicGeometryUpdater>, RuntimeError> {
        Ok(Box::new(self.create_dynamic(primitives)?))
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

/// Port of CesiumJS `DynamicEllipsoidGeometryUpdater`.
pub struct DynamicEllipsoidGeometryUpdater {
    base: Rc<RefCell<GeometryUpdaterBase>>,
    options: Rc<RefCell<EllipsoidOptions>>,
    terrain_offset: Rc<RefCell<Option<Rc<TerrainOffsetProperty>>>>,
    primitives: Rc<RefCell<PrimitiveCollection>>,
    primitive: Option<(Rc<RefCell<GeometryBatchPrimitive>>, PrimitiveHandle)>,
    outline_primitive: Option<(Rc<RefCell<GeometryBatchPrimitive>>, PrimitiveHandle)>,
    material: Option<Material>,
    last_scene_mode: Option<SceneMode>,
    last_show: Option<bool>,
    last_outline_show: Option<bool>,
    last_outline_width: Option<f64>,
    last_outline_color: Option<Color>,
    last_ddc: Option<DistanceDisplayCondition>,
    last_offset: Cartesian3,
    last_inner_radii: Option<Cartesian3>,
    destroyed: bool,
}

impl DynamicEllipsoidGeometryUpdater {
    fn new(
        base: Rc<RefCell<GeometryUpdaterBase>>,
        options: Rc<RefCell<EllipsoidOptions>>,
        terrain_offset: Rc<RefCell<Option<Rc<TerrainOffsetProperty>>>>,
        primitives: Rc<RefCell<PrimitiveCollection>>,
    ) -> Self {
        Self {
            base,
            options,
            terrain_offset,
            primitives,
            primitive: None,
            outline_primitive: None,
            material: None,
            last_scene_mode: None,
            last_show: None,
            last_outline_show: None,
            last_outline_width: None,
            last_outline_color: None,
            last_ddc: None,
            last_offset: Cartesian3::ZERO,
            last_inner_radii: None,
            destroyed: false,
        }
    }

    pub fn model_matrix(&self) -> Option<Matrix4> {
        self.primitive.as_ref().map(|(primitive, _)| primitive.borrow().model_matrix())
    }

    fn hide_primitives(&self) {
        if let Some((prim, _)) = &self.primitive {
            prim.borrow_mut().show = false;
        }
        if let Some((outline, _)) = &self.outline_primitive {
            outline.borrow_mut().show = false;
        }
    }
}

impl DynamicGeometryUpdater for DynamicEllipsoidGeometryUpdater {
    fn update(&mut self, time: &JulianDate) -> Result<(), RuntimeError> {
        let base = self.base.borrow();
        let entity = Rc::clone(&base.entity);
        let scene = Rc::clone(&base.scene);
        drop(base);

        let ellipsoid = match entity.get_property("ellipsoid") {
            Some(PropertyInput::Component(c)) => c,
            _ => {
                self.hide_primitives();
                return Ok(());
            }
        };

        if !entity.is_showing()
            || !entity.is_available(time)
            || !eval_bool(ellipsoid.get_property("show").as_ref(), time, true)?
        {
            self.hide_primitives();
            return Ok(());
        }

        let radii = eval_cartesian3_opt(ellipsoid.get_property("radii").as_ref(), time)?;
        let Some(mut radii) = radii else {
            self.hide_primitives();
            return Ok(());
        };

        let hr_slot = ellipsoid.get_property("heightReference");
        let hr_prop = hr_slot.clone().map(HeightReferenceSlotProperty::wrap);
        let scene_ellipsoid = scene.ellipsoid();
        let Some(mut model_matrix) = entity.compute_model_matrix_for_height_reference(
            time,
            HeightReferenceOptions {
                property: hr_prop.as_deref(),
                height_offset: radii.z * 0.5,
                ellipsoid: &scene_ellipsoid,
            },
            None,
        )?
        else {
            self.hide_primitives();
            return Ok(());
        };

        let show_fill = eval_bool(ellipsoid.get_property("fill").as_ref(), time, true)?;
        let show_outline = eval_bool(ellipsoid.get_property("outline").as_ref(), time, false)?;
        let outline_color = eval_color(ellipsoid.get_property("outlineColor").as_ref(), time, Color::BLACK)?;
        let mat_prop =
            extract_material_component(ellipsoid.get_property("material")).unwrap_or_else(default_color_material);
        let material = MaterialProperty::get_value(Some(time), Some(&*mat_prop), self.material.take())?;
        self.material = Some(material.clone());

        let inner_radii = eval_cartesian3_opt(ellipsoid.get_property("innerRadii").as_ref(), time)?;
        let minimum_clock = eval_f64_opt(ellipsoid.get_property("minimumClock").as_ref(), time)?;
        let maximum_clock = eval_f64_opt(ellipsoid.get_property("maximumClock").as_ref(), time)?;
        let minimum_cone = eval_f64_opt(ellipsoid.get_property("minimumCone").as_ref(), time)?;
        let maximum_cone = eval_f64_opt(ellipsoid.get_property("maximumCone").as_ref(), time)?;
        let stack_partitions = eval_f64_opt(ellipsoid.get_property("stackPartitions").as_ref(), time)?;
        let slice_partitions = eval_f64_opt(ellipsoid.get_property("slicePartitions").as_ref(), time)?;
        let subdivisions = eval_f64_opt(ellipsoid.get_property("subdivisions").as_ref(), time)?;
        let outline_width = eval_f64(ellipsoid.get_property("outlineWidth").as_ref(), time, 1.0)?;
        let height_reference = eval_height_reference(hr_slot.as_ref(), time, HeightReference::None)?;
        let offset_attribute =
            if height_reference != HeightReference::None { Some(GeometryOffsetAttribute::All) } else { None };

        let scene_mode = scene.mode();
        let in_3d = scene_mode == SceneMode::Scene3D && height_reference == HeightReference::None;

        let base = self.base.borrow();
        let shadows = eval_shadow_mode(base.shadows_property.as_ref(), time, ShadowMode::Disabled)?;
        let ddc = eval_ddc_or_default(base.distance_display_condition_property.as_ref(), time)?;
        let offset = eval_terrain_offset_or_zero(self.terrain_offset.borrow().as_ref(), time)?;

        let rebuild_primitives = {
            let opts = self.options.borrow();
            !in_3d
                || self.last_scene_mode != Some(scene_mode)
                || self.primitive.is_none()
                || opts.stack_partitions != stack_partitions
                || opts.slice_partitions != slice_partitions
                || self.last_inner_radii != inner_radii
                || opts.minimum_clock != minimum_clock
                || opts.maximum_clock != maximum_clock
                || opts.minimum_cone != minimum_cone
                || opts.maximum_cone != maximum_cone
                || opts.subdivisions != subdivisions
                || self.last_outline_width != Some(outline_width)
                || opts.offset_attribute != offset_attribute
        };

        if rebuild_primitives {
            if let Some((_, handle)) = self.primitive.take() {
                let _ = self.primitives.borrow_mut().remove_and_destroy(&handle);
            }
            if let Some((_, handle)) = self.outline_primitive.take() {
                let _ = self.primitives.borrow_mut().remove_and_destroy(&handle);
            }
            self.last_scene_mode = Some(scene_mode);
            self.last_outline_width = Some(outline_width);
            self.last_inner_radii = inner_radii;

            {
                let mut opts = self.options.borrow_mut();
                opts.stack_partitions = stack_partitions;
                opts.slice_partitions = slice_partitions;
                opts.subdivisions = subdivisions;
                opts.offset_attribute = offset_attribute;
                opts.radii = Some(if in_3d { UNIT_SPHERE } else { radii });
                opts.inner_radii = inner_radii.map(|ir| {
                    if in_3d {
                        Cartesian3::new(ir.x / radii.x, ir.y / radii.y, ir.z / radii.z)
                    } else {
                        ir
                    }
                });
                opts.minimum_clock = minimum_clock;
                opts.maximum_clock = maximum_clock;
                opts.minimum_cone = minimum_cone;
                opts.maximum_cone = maximum_cone;
            }

            let appearance =
                GeometryAppearanceKind::Material.build(material.is_translucent(), true, false, None, Some(material));
            self.options.borrow_mut().vertex_format = appearance.vertex_format();

            let offset_ref = self.terrain_offset.borrow();
            let fill_instance = create_fill_instance(&base, &self.options.borrow(), offset_ref.as_ref(), time, in_3d)?;
            let prim = Rc::new(RefCell::new(GeometryBatchPrimitive::new(
                vec![fill_instance],
                Some(appearance),
                None,
                true,
                false,
                shadows,
            )));
            let handle = PrimitiveHandle::from_rc(Rc::clone(&prim) as Rc<RefCell<dyn ScenePrimitive>>);
            self.primitives.borrow_mut().add(handle.clone(), None)?;
            self.primitive = Some((prim, handle));

            let outline_instance =
                create_outline_instance(&base, &self.options.borrow(), offset_ref.as_ref(), time, in_3d)?;
            let outline_translucent = instance_alpha(&outline_instance) != 255;
            let outline_appearance = GeometryAppearanceKind::PerInstanceColor.build(
                outline_translucent,
                false,
                true,
                Some(scene.clamp_line_width(outline_width)),
                None,
            );
            let outline_prim = Rc::new(RefCell::new(GeometryBatchPrimitive::new(
                vec![outline_instance],
                Some(outline_appearance),
                None,
                true,
                false,
                shadows,
            )));
            let outline_handle = PrimitiveHandle::from_rc(Rc::clone(&outline_prim) as Rc<RefCell<dyn ScenePrimitive>>);
            self.primitives.borrow_mut().add(outline_handle.clone(), None)?;
            self.outline_primitive = Some((outline_prim, outline_handle));

            self.last_show = Some(show_fill);
            self.last_outline_show = Some(show_outline);
            self.last_outline_color = Some(outline_color);
            self.last_ddc = Some(ddc);
            self.last_offset = offset;
        } else if let (Some((prim_rc, _)), Some((outline_rc, _))) = (&self.primitive, &self.outline_primitive) {
            if prim_rc.borrow().is_ready() {
                let mut prim = prim_rc.borrow_mut();
                let mut outline_prim = outline_rc.borrow_mut();
                prim.show = true;
                outline_prim.show = true;
                prim.set_appearance_material(material);

                let entity_id = entity.id();
                if self.last_show != Some(show_fill) {
                    if let Some(attr) = prim.get_attributes_mut(entity_id) {
                        attr.show = ShowGeometryInstanceAttribute::to_value(show_fill);
                    }
                    self.last_show = Some(show_fill);
                }

                if self.last_outline_show != Some(show_outline) {
                    if let Some(attr) = outline_prim.get_attributes_mut(entity_id) {
                        attr.show = ShowGeometryInstanceAttribute::to_value(show_outline);
                    }
                    self.last_outline_show = Some(show_outline);
                }

                if self.last_outline_color != Some(outline_color) {
                    if let Some(attr) = outline_prim.get_attributes_mut(entity_id) {
                        attr.color = Some(ColorGeometryInstanceAttribute::to_value(&outline_color));
                    }
                    self.last_outline_color = Some(outline_color);
                }

                if !ddc_equals(self.last_ddc, ddc) {
                    let val = DistanceDisplayConditionGeometryInstanceAttribute::to_value(&ddc);
                    if let Some(attr) = prim.get_attributes_mut(entity_id) {
                        attr.distance_display_condition = Some(val);
                    }
                    if let Some(attr) = outline_prim.get_attributes_mut(entity_id) {
                        attr.distance_display_condition = Some(val);
                    }
                    self.last_ddc = Some(ddc);
                }

                if self.last_offset != offset {
                    let val = OffsetGeometryInstanceAttribute::to_value(&offset);
                    if let Some(attr) = prim.get_attributes_mut(entity_id) {
                        attr.offset = Some(val);
                    }
                    if let Some(attr) = outline_prim.get_attributes_mut(entity_id) {
                        attr.offset = Some(val);
                    }
                    self.last_offset = offset;
                }
            }
        }

        if in_3d {
            radii.x = radii.x.max(0.001);
            radii.y = radii.y.max(0.001);
            radii.z = radii.z.max(0.001);
            model_matrix = model_matrix.multiply_by_scale(radii);
            if let Some((prim, _)) = &self.primitive {
                prim.borrow_mut().set_model_matrix(model_matrix);
            }
            if let Some((outline, _)) = &self.outline_primitive {
                outline.borrow_mut().set_model_matrix(model_matrix);
            }
        }

        Ok(())
    }

    fn get_bounding_sphere(&self, result: &mut BoundingSphere) -> BoundingSphereState {
        let entity_id = self.base.borrow().entity.id().to_owned();
        if let Some((prim, _)) = &self.primitive {
            let prim = prim.borrow();
            if prim.show && prim.is_ready() {
                if let Some(bs) = prim.bounding_sphere_for(&entity_id, false) {
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
        let pending = self.primitive.as_ref().is_some_and(|(p, _)| !p.borrow().is_ready())
            || self.outline_primitive.as_ref().is_some_and(|(p, _)| !p.borrow().is_ready());
        if pending {
            BoundingSphereState::Pending
        } else {
            BoundingSphereState::Failed
        }
    }

    fn is_destroyed(&self) -> bool {
        self.destroyed
    }

    fn destroy(&mut self) {
        if !self.destroyed {
            if let Some((_, handle)) = self.primitive.take() {
                let _ = self.primitives.borrow_mut().remove_and_destroy(&handle);
            }
            if let Some((_, handle)) = self.outline_primitive.take() {
                let _ = self.primitives.borrow_mut().remove_and_destroy(&handle);
            }
            self.destroyed = true;
        }
    }
}
