//! CesiumJS `GeometryUpdater`: abstract base contract, shared updater state,
//! scene surface trait, and batched primitive wrappers used by geometry
//! updaters, static/dynamic batches, and visualizers.
use super::{
    color_material_property::color_material_component, visualizer_values::*, BoundingSphereState, ChangeEvent,
    ColorMaterialProperty, ComponentRef, DefinitionChanged, DynamicValue, Entity, EntityComponent, EntityRef, Property,
    PropertyInput, PropertyRef, TerrainHeightCallback, TerrainHeightHandle, TerrainOffsetProperty, TerrainOffsetScene,
};
use crate::{
    core::{
        iso8601, ArcType, BoundingSphere, Cartesian2, Cartesian3, Cartographic, Color, ColorGeometryInstanceAttribute,
        CornerType, DistanceDisplayCondition, DistanceDisplayConditionGeometryInstanceAttribute, Ellipsoid,
        GeometryInstance, GeometryInstanceAttribute, GeometryOffsetAttribute, JulianDate, Matrix4,
        OffsetGeometryInstanceAttribute, Plane, PolygonHierarchy, Rectangle, RuntimeError,
        ShowGeometryInstanceAttribute, TextureCoordinateHierarchy, TypedArray,
    },
    renderer::RenderState,
    scene::{
        appearance::{default_render_state, Appearance},
        primitive::{PrimitiveOptions, RenderPrimitive},
        scene::ScenePrimitive,
        Cesium3DTilePass, ClassificationType, FrameState, HeightReference, Material, MaterialAppearance,
        MaterialAppearanceOptions, OrderedGroundPrimitiveCollection, PerInstanceColorAppearance,
        PerInstanceColorAppearanceOptions, PolylineColorAppearance, PolylineColorAppearanceOptions,
        PolylineMaterialAppearance, PolylineMaterialAppearanceOptions, PrimitiveCollection, Scene, SceneBackend,
        SceneMode, ShadowMode,
    },
};
use serde_json::Value;
use std::{
    any::Any,
    cell::{Cell, RefCell},
    collections::HashMap,
    rc::Rc,
};

/// Payload emitted by [`GeometryUpdater::geometry_changed`].
#[derive(Clone)]
pub struct GeometryChangedEvent {
    pub updater_id: String,
    pub entity: EntityRef,
}

/// Shared reference to a concrete [`GeometryUpdater`].
pub type GeometryUpdaterRef = Rc<RefCell<dyn GeometryUpdater>>;

/// Host scene contract consumed by geometry updaters, batches, and visualizers.
pub trait GeometryScene: TerrainOffsetScene {
    fn id(&self) -> &str {
        "scene"
    }
    fn mode(&self) -> SceneMode {
        SceneMode::Scene3D
    }
    fn clamp_line_width(&self, width: f64) -> f64 {
        width.clamp(1.0, 1.0)
    }
    fn supports_materials_for_entities_on_terrain(&self) -> bool {
        true
    }
    fn supports_polylines_on_terrain(&self) -> bool {
        true
    }
    fn ground_primitive_supported(&self) -> bool {
        true
    }
    fn ground_polyline_supported(&self) -> bool {
        true
    }
    fn primitives(&self) -> Rc<RefCell<PrimitiveCollection>>;
    fn ground_primitives(&self) -> Rc<RefCell<OrderedGroundPrimitiveCollection>>;
    fn as_terrain_offset_scene(self: Rc<Self>) -> Rc<dyn TerrainOffsetScene>;
}

/// Optional terrain-height sampler for [`BasicGeometryScene`].
pub type SceneHeightSampler = Rc<dyn Fn(&Cartographic, Option<HeightReference>) -> Option<f64>>;

/// Lightweight host implementation of [`GeometryScene`] suitable for tests,
/// headless visualization, and custom hosts.
pub struct BasicGeometryScene {
    pub id: String,
    pub mode: Cell<SceneMode>,
    pub has_globe: Cell<bool>,
    pub ellipsoid: Ellipsoid,
    pub max_line_width: Cell<f64>,
    pub supports_materials_for_entities_on_terrain: Cell<bool>,
    pub supports_polylines_on_terrain: Cell<bool>,
    pub ground_primitive_supported: Cell<bool>,
    pub ground_polyline_supported: Cell<bool>,
    pub primitives: Rc<RefCell<PrimitiveCollection>>,
    pub ground_primitives: Rc<RefCell<OrderedGroundPrimitiveCollection>>,
    pub height_sampler: RefCell<Option<SceneHeightSampler>>,
    pub terrain_provider_changed: DefinitionChanged,
    pub morph_complete: DefinitionChanged,
}

impl Default for BasicGeometryScene {
    fn default() -> Self {
        Self {
            id: "scene".to_owned(),
            mode: Cell::new(SceneMode::Scene3D),
            has_globe: Cell::new(true),
            ellipsoid: Ellipsoid::WGS84,
            max_line_width: Cell::new(1.0),
            supports_materials_for_entities_on_terrain: Cell::new(true),
            supports_polylines_on_terrain: Cell::new(true),
            ground_primitive_supported: Cell::new(true),
            ground_polyline_supported: Cell::new(true),
            primitives: Rc::new(RefCell::new(PrimitiveCollection::default())),
            ground_primitives: Rc::new(RefCell::new(OrderedGroundPrimitiveCollection::default())),
            height_sampler: RefCell::new(None),
            terrain_provider_changed: DefinitionChanged::new(),
            morph_complete: DefinitionChanged::new(),
        }
    }
}

impl BasicGeometryScene {
    pub fn new() -> Rc<Self> {
        Rc::new(Self::default())
    }

    /// Adds the geometry collection to a Scene. Ground shapes use ordinary
    /// geometry until the Scene renderer supports terrain classification.
    pub fn attach_to<B: SceneBackend>(scene: &mut Scene<B>) -> Rc<Self> {
        let host = Rc::new(Self {
            mode: Cell::new(scene.camera.mode()),
            has_globe: Cell::new(scene.globe.is_some()),
            ellipsoid: scene.camera.ellipsoid,
            max_line_width: Cell::new(scene.maximum_aliased_line_width),
            supports_materials_for_entities_on_terrain: Cell::new(false),
            supports_polylines_on_terrain: Cell::new(false),
            ground_primitive_supported: Cell::new(false),
            ground_polyline_supported: Cell::new(false),
            ..Self::default()
        });
        scene.primitives.push(Box::new(SharedGeometryCollection(host.primitives.clone())));
        host
    }

    pub fn set_mode(&self, mode: SceneMode) {
        self.mode.set(mode);
    }

    pub fn set_max_line_width(&self, max_width: f64) {
        self.max_line_width.set(max_width.max(1.0));
    }

    pub fn set_height_sampler(&self, sampler: Option<SceneHeightSampler>) {
        *self.height_sampler.borrow_mut() = sampler;
    }
}

struct SharedGeometryCollection(Rc<RefCell<PrimitiveCollection>>);

impl ScenePrimitive for SharedGeometryCollection {
    fn pre_passes_update(&mut self, frame: &mut FrameState) -> Result<(), RuntimeError> {
        self.0.borrow_mut().pre_passes_update(frame)
    }

    fn update(&mut self, frame: &mut FrameState) -> Result<(), RuntimeError> {
        self.0.borrow_mut().update(frame)
    }

    fn post_passes_update(&mut self, frame: &mut FrameState) -> Result<(), RuntimeError> {
        self.0.borrow_mut().post_passes_update(frame)
    }

    fn update_for_pass(&mut self, frame: &mut FrameState, pass: Cesium3DTilePass) -> Result<(), RuntimeError> {
        self.0.borrow_mut().update_for_pass(frame, pass)
    }

    fn destroy(&mut self) -> Result<(), RuntimeError> {
        self.0.borrow_mut().destroy()
    }
}

impl TerrainOffsetScene for BasicGeometryScene {
    fn has_globe(&self) -> bool {
        self.has_globe.get()
    }

    fn ellipsoid(&self) -> Ellipsoid {
        self.ellipsoid
    }

    fn get_height(&self, cartographic: &Cartographic, height_reference: Option<HeightReference>) -> Option<f64> {
        self.height_sampler.borrow().as_ref().and_then(|f| f(cartographic, height_reference))
    }

    fn update_height(
        &self,
        _cartographic: &Cartographic,
        _callback: TerrainHeightCallback,
        _height_reference: Option<HeightReference>,
    ) -> Option<TerrainHeightHandle> {
        None
    }

    fn terrain_provider_changed(&self) -> Option<&DefinitionChanged> {
        Some(&self.terrain_provider_changed)
    }

    fn morph_complete(&self) -> Option<&DefinitionChanged> {
        Some(&self.morph_complete)
    }
}

impl GeometryScene for BasicGeometryScene {
    fn id(&self) -> &str {
        &self.id
    }

    fn mode(&self) -> SceneMode {
        self.mode.get()
    }

    fn clamp_line_width(&self, width: f64) -> f64 {
        width.clamp(1.0, self.max_line_width.get().max(1.0))
    }

    fn supports_materials_for_entities_on_terrain(&self) -> bool {
        self.supports_materials_for_entities_on_terrain.get()
    }

    fn supports_polylines_on_terrain(&self) -> bool {
        self.supports_polylines_on_terrain.get()
    }

    fn ground_primitive_supported(&self) -> bool {
        self.ground_primitive_supported.get()
    }

    fn ground_polyline_supported(&self) -> bool {
        self.ground_polyline_supported.get()
    }

    fn primitives(&self) -> Rc<RefCell<PrimitiveCollection>> {
        Rc::clone(&self.primitives)
    }

    fn ground_primitives(&self) -> Rc<RefCell<OrderedGroundPrimitiveCollection>> {
        Rc::clone(&self.ground_primitives)
    }

    fn as_terrain_offset_scene(self: Rc<Self>) -> Rc<dyn TerrainOffsetScene> {
        self
    }
}

/// Appearance constructor discriminator matching CesiumJS batch appearance types.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum GeometryAppearanceKind {
    PerInstanceColor,
    Material,
    PolylineColor,
    PolylineMaterial,
}

impl GeometryAppearanceKind {
    pub fn build(
        self,
        translucent: bool,
        closed: bool,
        flat: bool,
        line_width: Option<f64>,
        material: Option<Material>,
    ) -> Box<dyn Appearance> {
        let render_state = line_width.map(|w| {
            let mut rs: RenderState = default_render_state(translucent, closed, None);
            rs.line_width = w;
            rs
        });
        match self {
            Self::PerInstanceColor => Box::new(PerInstanceColorAppearance::new(PerInstanceColorAppearanceOptions {
                flat: Some(flat),
                translucent: Some(translucent),
                closed: Some(closed),
                render_state,
                ..Default::default()
            })),
            Self::Material => Box::new(MaterialAppearance::new(MaterialAppearanceOptions {
                material,
                translucent: Some(translucent),
                closed: Some(closed),
                render_state,
                ..Default::default()
            })),
            Self::PolylineColor => Box::new(PolylineColorAppearance::new(PolylineColorAppearanceOptions {
                translucent: Some(translucent),
                render_state,
                ..Default::default()
            })),
            Self::PolylineMaterial => Box::new(PolylineMaterialAppearance::new(PolylineMaterialAppearanceOptions {
                material,
                translucent: Some(translucent),
                render_state,
                ..Default::default()
            })),
        }
    }
}

/// Per-instance mutable attribute record stored on batched and ground primitives.
#[derive(Clone, Debug)]
pub struct InstanceAttributeRecord {
    pub show: [u8; 1],
    pub color: Option<[u8; 4]>,
    pub depth_fail_color: Option<[u8; 4]>,
    pub distance_display_condition: Option<[f32; 2]>,
    pub offset: Option<[f32; 3]>,
    pub local_bounding_sphere: Option<BoundingSphere>,
    pub instance_model_matrix: Matrix4,
    pub offset_attribute: Option<GeometryOffsetAttribute>,
    pub last_color: Option<Color>,
    pub last_depth_fail_color: Option<Color>,
    pub last_distance_display_condition: Option<DistanceDisplayCondition>,
    pub last_offset: Option<Cartesian3>,
}

impl InstanceAttributeRecord {
    pub fn from_instance(instance: &GeometryInstance) -> Self {
        let mut show = [1u8];
        let mut color = None;
        let mut depth_fail_color = None;
        let mut distance_display_condition = None;
        let mut offset = None;

        for (name, attr) in &instance.attributes {
            match name.as_str() {
                "show" => {
                    if let TypedArray::Uint8(v) = &attr.value {
                        if let Some(&b) = v.first() {
                            show = [b];
                        }
                    }
                }
                "color" => {
                    if let TypedArray::Uint8(v) = &attr.value {
                        if v.len() >= 4 {
                            color = Some([v[0], v[1], v[2], v[3]]);
                        }
                    }
                }
                "depthFailColor" => {
                    if let TypedArray::Uint8(v) = &attr.value {
                        if v.len() >= 4 {
                            depth_fail_color = Some([v[0], v[1], v[2], v[3]]);
                        }
                    }
                }
                "distanceDisplayCondition" => {
                    if let TypedArray::Float32(v) = &attr.value {
                        if v.len() >= 2 {
                            distance_display_condition = Some([v[0], v[1]]);
                        }
                    }
                }
                "offset" => {
                    if let TypedArray::Float32(v) = &attr.value {
                        if v.len() >= 3 {
                            offset = Some([v[0], v[1], v[2]]);
                        }
                    }
                }
                _ => {}
            }
        }

        let (local_bounding_sphere, offset_attribute) = match &instance.geometry {
            Some(geom) => (geom.bounding_sphere, geom.offset_attribute),
            None => (None, None),
        };

        Self {
            show,
            color,
            depth_fail_color,
            distance_display_condition,
            offset,
            local_bounding_sphere,
            instance_model_matrix: instance.model_matrix,
            offset_attribute,
            last_color: None,
            last_depth_fail_color: None,
            last_distance_display_condition: None,
            last_offset: None,
        }
    }

    pub fn bounding_sphere(&self, primitive_model_matrix: &Matrix4) -> Option<BoundingSphere> {
        let mut sphere = self.local_bounding_sphere?;
        sphere = sphere.transform(&self.instance_model_matrix);
        if let Some([ox, oy, oz]) = self.offset {
            let offset_vec = Cartesian3::new(f64::from(ox), f64::from(oy), f64::from(oz));
            let shifted = BoundingSphere::new(sphere.center.add(offset_vec), sphere.radius);
            sphere = match self.offset_attribute {
                Some(GeometryOffsetAttribute::Top) => sphere.union(&shifted),
                Some(GeometryOffsetAttribute::All) => shifted,
                _ => sphere,
            };
        }
        Some(sphere.transform(primitive_model_matrix))
    }
}

/// Host-independent batched primitive implementing [`ScenePrimitive`], used by
/// static geometry batches and [`DynamicGeometryUpdater`](super::DynamicGeometryUpdater).
pub struct GeometryBatchPrimitive {
    pub show: bool,
    pub asynchronous: bool,
    pub shadows: ShadowMode,
    model_matrix: Matrix4,
    appearance: Option<Box<dyn Appearance>>,
    depth_fail_appearance: Option<Box<dyn Appearance>>,
    geometry_instances: Vec<GeometryInstance>,
    attributes: HashMap<String, InstanceAttributeRecord>,
    renderer: Option<Box<dyn RenderPrimitive>>,
    ready: bool,
    destroyed: bool,
}

impl GeometryBatchPrimitive {
    pub fn new(
        geometry_instances: Vec<GeometryInstance>,
        appearance: Option<Box<dyn Appearance>>,
        depth_fail_appearance: Option<Box<dyn Appearance>>,
        show: bool,
        asynchronous: bool,
        shadows: ShadowMode,
    ) -> Self {
        let mut attributes = HashMap::with_capacity(geometry_instances.len());
        for instance in &geometry_instances {
            if let Some(id) = &instance.id {
                attributes.insert(id.clone(), InstanceAttributeRecord::from_instance(instance));
            }
        }
        Self {
            show,
            asynchronous,
            shadows,
            model_matrix: Matrix4::IDENTITY,
            appearance,
            depth_fail_appearance,
            geometry_instances,
            attributes,
            renderer: None,
            ready: !asynchronous,
            destroyed: false,
        }
    }

    pub fn is_ready(&self) -> bool {
        self.renderer.as_ref().map_or(self.ready, |renderer| renderer.ready())
    }

    pub fn mark_ready(&mut self) {
        if !self.destroyed {
            self.ready = true;
        }
    }

    pub fn is_destroyed(&self) -> bool {
        self.destroyed
    }

    pub fn geometry_instances(&self) -> &[GeometryInstance] {
        &self.geometry_instances
    }

    pub fn get_attributes(&self, entity_id: &str) -> Option<&InstanceAttributeRecord> {
        self.attributes.get(entity_id)
    }

    pub fn get_attributes_mut(&mut self, entity_id: &str) -> Option<&mut InstanceAttributeRecord> {
        self.attributes.get_mut(entity_id)
    }

    pub fn model_matrix(&self) -> Matrix4 {
        self.model_matrix
    }

    /// Update the transform of a batch built with identity instance matrices.
    pub(crate) fn set_model_matrix(&mut self, matrix: Matrix4) {
        debug_assert!(self.geometry_instances.iter().all(|instance| instance.model_matrix == Matrix4::IDENTITY));
        self.model_matrix = matrix;
        if let Some(renderer) = &mut self.renderer {
            renderer.set_model_matrix(matrix);
        }
    }

    pub fn set_appearance_material(&mut self, material: Material) {
        if let Some(renderer) = &mut self.renderer {
            renderer.set_material(material, false);
        } else if let Some(slot) = self.appearance.as_deref_mut().and_then(Appearance::material_mut) {
            *slot = material;
        }
    }

    pub fn set_depth_fail_material(&mut self, material: Material) {
        if let Some(renderer) = &mut self.renderer {
            renderer.set_material(material, true);
        } else if let Some(slot) = self.depth_fail_appearance.as_deref_mut().and_then(Appearance::material_mut) {
            *slot = material;
        }
    }

    pub fn bounding_sphere_for(&self, entity_id: &str, check_show: bool) -> Option<BoundingSphere> {
        let attr = self.attributes.get(entity_id)?;
        if check_show && attr.show[0] == 0 {
            return None;
        }
        attr.bounding_sphere(&self.model_matrix)
    }
}

impl ScenePrimitive for GeometryBatchPrimitive {
    fn pre_passes_update(&mut self, _: &mut FrameState) -> Result<(), RuntimeError> {
        Ok(())
    }

    fn update(&mut self, frame: &mut FrameState) -> Result<(), RuntimeError> {
        if self.destroyed {
            return Ok(());
        }
        let Some(create) = frame.create_primitive else {
            self.ready = true;
            return Ok(());
        };
        let renderer = self.renderer.get_or_insert_with(|| {
            create(PrimitiveOptions {
                geometry_instances: self.geometry_instances.clone(),
                appearance: self.appearance.take(),
                depth_fail_appearance: self.depth_fail_appearance.take(),
                show: Some(self.show),
                model_matrix: Some(self.model_matrix),
                asynchronous: Some(self.asynchronous),
                shadows: Some(self.shadows),
                ..Default::default()
            })
        });
        renderer.set_properties(self.show, self.shadows);
        if renderer.ready() {
            for (id, record) in &self.attributes {
                renderer.set_instance_attribute(id, "show", TypedArray::Uint8(record.show.to_vec()))?;
                for (name, value) in [("color", record.color), ("depthFailColor", record.depth_fail_color)] {
                    if let Some(value) = value {
                        renderer.set_instance_attribute(id, name, TypedArray::Uint8(value.to_vec()))?;
                    }
                }
                if let Some(value) = record.distance_display_condition {
                    renderer.set_instance_attribute(
                        id,
                        "distanceDisplayCondition",
                        TypedArray::Float32(value.to_vec()),
                    )?;
                }
                if let Some(value) = record.offset {
                    renderer.set_instance_attribute(id, "offset", TypedArray::Float32(value.to_vec()))?;
                }
            }
        }
        renderer.update(frame)
    }

    fn post_passes_update(&mut self, _: &mut FrameState) -> Result<(), RuntimeError> {
        Ok(())
    }

    fn update_for_pass(&mut self, _: &mut FrameState, _: Cesium3DTilePass) -> Result<(), RuntimeError> {
        Ok(())
    }

    fn destroy(&mut self) -> Result<(), RuntimeError> {
        self.destroyed = true;
        self.ready = false;
        self.geometry_instances.clear();
        self.attributes.clear();
        self.appearance = None;
        self.depth_fail_appearance = None;
        self.renderer.take().map_or(Ok(()), |mut primitive| primitive.destroy())
    }
}

/// Ground primitive wrapper (representing CesiumJS `GroundPrimitive` and
/// `GroundPolylinePrimitive`) implementing [`ScenePrimitive`].
pub struct GroundGeometryPrimitive {
    pub show: bool,
    pub asynchronous: bool,
    pub classification_type: ClassificationType,
    pub shadows: ShadowMode,
    pub is_polyline: bool,
    pub appearance: Option<Box<dyn Appearance>>,
    geometry_instances: Vec<GeometryInstance>,
    attributes: HashMap<String, InstanceAttributeRecord>,
    ready: bool,
    destroyed: bool,
}

impl GroundGeometryPrimitive {
    pub fn new(
        geometry_instances: Vec<GeometryInstance>,
        appearance: Option<Box<dyn Appearance>>,
        classification_type: ClassificationType,
        shadows: ShadowMode,
        show: bool,
        asynchronous: bool,
        is_polyline: bool,
    ) -> Self {
        let mut attributes = HashMap::with_capacity(geometry_instances.len());
        for instance in &geometry_instances {
            if let Some(id) = &instance.id {
                attributes.insert(id.clone(), InstanceAttributeRecord::from_instance(instance));
            }
        }
        Self {
            show,
            asynchronous,
            classification_type,
            shadows,
            is_polyline,
            appearance,
            geometry_instances,
            attributes,
            ready: !asynchronous,
            destroyed: false,
        }
    }

    pub fn is_ready(&self) -> bool {
        self.ready
    }

    pub fn mark_ready(&mut self) {
        if !self.destroyed {
            self.ready = true;
        }
    }

    pub fn is_destroyed(&self) -> bool {
        self.destroyed
    }

    pub fn geometry_instances(&self) -> &[GeometryInstance] {
        &self.geometry_instances
    }

    pub fn get_attributes(&self, entity_id: &str) -> Option<&InstanceAttributeRecord> {
        self.attributes.get(entity_id)
    }

    pub fn get_attributes_mut(&mut self, entity_id: &str) -> Option<&mut InstanceAttributeRecord> {
        self.attributes.get_mut(entity_id)
    }

    pub fn set_appearance_material(&mut self, material: Material) {
        if let Some(app) = self.appearance.as_deref_mut() {
            if let Some(slot) = app.material_mut() {
                *slot = material;
            }
        }
    }

    pub fn bounding_sphere_for(&self, entity_id: &str, check_show: bool) -> Option<BoundingSphere> {
        let attr = self.attributes.get(entity_id)?;
        if check_show && attr.show[0] == 0 {
            return None;
        }
        attr.bounding_sphere(&Matrix4::IDENTITY)
    }
}

/// Adapter storing a shared [`GroundGeometryPrimitive`] inside an
/// [`OrderedGroundPrimitiveCollection`].
pub struct SharedGroundPrimitive(pub Rc<RefCell<GroundGeometryPrimitive>>);

impl ScenePrimitive for SharedGroundPrimitive {
    fn pre_passes_update(&mut self, _: &mut FrameState) -> Result<(), RuntimeError> {
        Ok(())
    }

    fn update(&mut self, _: &mut FrameState) -> Result<(), RuntimeError> {
        let mut inner = self.0.borrow_mut();
        if !inner.destroyed {
            inner.ready = true;
        }
        Ok(())
    }

    fn post_passes_update(&mut self, _: &mut FrameState) -> Result<(), RuntimeError> {
        Ok(())
    }

    fn update_for_pass(&mut self, _: &mut FrameState, _: Cesium3DTilePass) -> Result<(), RuntimeError> {
        Ok(())
    }

    fn destroy(&mut self) -> Result<(), RuntimeError> {
        let mut inner = self.0.borrow_mut();
        inner.destroyed = true;
        inner.ready = false;
        inner.geometry_instances.clear();
        inner.attributes.clear();
        inner.appearance = None;
        Ok(())
    }
}

/// Port of CesiumJS abstract `GeometryUpdater` interface.
pub trait GeometryUpdater: Any {
    fn as_any(&self) -> &dyn Any;
    fn as_any_mut(&mut self) -> &mut dyn Any;

    fn id(&self) -> &str;
    fn entity(&self) -> EntityRef;
    fn scene(&self) -> Rc<dyn GeometryScene>;

    fn fill_enabled(&self) -> bool;
    fn has_constant_fill(&self) -> bool;
    fn fill_material_property(&self) -> Option<ComponentRef>;
    fn depth_fail_material_property(&self) -> Option<ComponentRef> {
        None
    }

    fn outline_enabled(&self) -> bool;
    fn has_constant_outline(&self) -> bool;
    fn outline_color_property(&self) -> Option<PropertyInput>;
    fn outline_width(&self) -> f64;

    fn shadows_property(&self) -> Option<PropertyInput>;
    fn distance_display_condition_property(&self) -> Option<PropertyInput>;
    fn classification_type_property(&self) -> Option<PropertyInput>;
    fn terrain_offset_property(&self) -> Option<Rc<TerrainOffsetProperty>> {
        None
    }
    fn z_index(&self) -> Option<PropertyInput> {
        None
    }

    fn is_dynamic(&self) -> bool;
    fn is_closed(&self) -> bool;
    fn on_terrain(&self) -> bool;
    fn clamp_to_ground(&self) -> bool {
        false
    }

    fn geometry_changed(&self) -> &ChangeEvent<GeometryChangedEvent>;
    fn is_filled(&self, time: &JulianDate) -> Result<bool, RuntimeError>;
    fn is_outline_visible(&self, time: &JulianDate) -> Result<bool, RuntimeError>;

    fn create_fill_geometry_instance(&self, time: &JulianDate) -> Result<GeometryInstance, RuntimeError>;
    fn create_outline_geometry_instance(&self, time: &JulianDate) -> Result<GeometryInstance, RuntimeError>;
    fn create_dynamic_updater(
        &self,
        primitives: Rc<RefCell<PrimitiveCollection>>,
        ground_primitives: Rc<RefCell<OrderedGroundPrimitiveCollection>>,
    ) -> Result<Box<dyn super::DynamicGeometryUpdater>, RuntimeError>;

    /// Cartographic bounding rectangle for ground-clamped geometry batches.
    fn bounding_rectangle(&self) -> Option<Rectangle> {
        None
    }

    fn on_entity_property_changed(&mut self, entity: &Entity, property_name: &str);

    fn is_destroyed(&self) -> bool;
    fn destroy(&mut self);
}

/// Shared base state for concrete [`GeometryUpdater`] implementations (port of
/// CesiumJS `GeometryUpdater` constructor fields and property-change logic).
pub struct GeometryUpdaterBase {
    pub entity: EntityRef,
    pub scene: Rc<dyn GeometryScene>,
    pub fill_enabled: bool,
    pub is_closed: bool,
    pub on_terrain: bool,
    pub dynamic: bool,
    pub outline_enabled: bool,
    pub geometry_changed: ChangeEvent<GeometryChangedEvent>,
    pub show_property: Option<PropertyInput>,
    pub fill_property: Option<PropertyInput>,
    pub material_property: Option<ComponentRef>,
    pub show_outline_property: Option<PropertyInput>,
    pub outline_color_property: Option<PropertyInput>,
    pub outline_width: f64,
    pub shadows_property: Option<PropertyInput>,
    pub distance_display_condition_property: Option<PropertyInput>,
    pub classification_type_property: Option<PropertyInput>,
    pub geometry_property_name: &'static str,
    pub id: String,
    pub observed_property_names: &'static [&'static str],
    pub supports_materials_for_entities_on_terrain: bool,
    pub destroyed: bool,
}

impl GeometryUpdaterBase {
    pub fn new(
        entity: EntityRef,
        scene: Rc<dyn GeometryScene>,
        geometry_property_name: &'static str,
        observed_property_names: &'static [&'static str],
    ) -> Self {
        let id = format!("{geometry_property_name}-{}", entity.id());
        let supports_materials = scene.supports_materials_for_entities_on_terrain();
        Self {
            entity,
            scene,
            fill_enabled: false,
            is_closed: false,
            on_terrain: false,
            dynamic: false,
            outline_enabled: false,
            geometry_changed: ChangeEvent::new(),
            show_property: None,
            fill_property: None,
            material_property: None,
            show_outline_property: None,
            outline_color_property: None,
            outline_width: 1.0,
            shadows_property: None,
            distance_display_condition_property: None,
            classification_type_property: None,
            geometry_property_name,
            id,
            observed_property_names,
            supports_materials_for_entities_on_terrain: supports_materials,
            destroyed: false,
        }
    }

    pub fn has_constant_fill(&self) -> bool {
        !self.fill_enabled
            || (self.entity.availability().is_none()
                && is_slot_constant(self.show_property.as_ref())
                && is_slot_constant(self.fill_property.as_ref()))
    }

    pub fn has_constant_outline(&self) -> bool {
        !self.outline_enabled
            || (self.entity.availability().is_none()
                && is_slot_constant(self.show_property.as_ref())
                && is_slot_constant(self.show_outline_property.as_ref()))
    }

    pub fn is_filled(&self, time: &JulianDate) -> Result<bool, RuntimeError> {
        Ok(self.fill_enabled
            && self.entity.is_available(time)
            && eval_bool(self.show_property.as_ref(), time, true)?
            && eval_bool(self.fill_property.as_ref(), time, true)?)
    }

    pub fn is_outline_visible(&self, time: &JulianDate) -> Result<bool, RuntimeError> {
        Ok(self.outline_enabled
            && self.entity.is_available(time)
            && eval_bool(self.show_property.as_ref(), time, true)?
            && eval_bool(self.show_outline_property.as_ref(), time, false)?)
    }

    pub fn default_is_hidden(geometry: &dyn EntityComponent) -> bool {
        let show = geometry.get_property("show");
        if let Some(slot) = show.as_ref() {
            if is_slot_constant(Some(slot)) {
                let min_time = iso8601::minimum_value();
                return !eval_bool(Some(slot), &min_time, true).unwrap_or(true);
            }
        }
        false
    }

    pub fn raise_changed(&self) {
        self.geometry_changed
            .raise(GeometryChangedEvent { updater_id: self.id.clone(), entity: Rc::clone(&self.entity) });
    }

    /// Runs CesiumJS `GeometryUpdater.prototype._onEntityPropertyChanged` with
    /// subclass hooks for hidden, on-terrain, dynamic, static options, and
    /// closed checks.
    #[allow(clippy::too_many_arguments)]
    pub fn on_entity_property_changed_with<FHidden, FOnTerrain, FDynamic, FStatic, FClosed>(
        &mut self,
        entity: &Entity,
        property_name: &str,
        is_hidden: FHidden,
        is_on_terrain: FOnTerrain,
        is_dynamic: FDynamic,
        mut set_static_options: FStatic,
        get_is_closed: FClosed,
    ) where
        FHidden: FnOnce(&Entity, &dyn EntityComponent) -> bool,
        FOnTerrain: FnOnce(&Self, &Entity, &dyn EntityComponent) -> bool,
        FDynamic: FnOnce(&Self, &Entity, &dyn EntityComponent) -> bool,
        FStatic: FnMut(&Self, &Entity, &dyn EntityComponent),
        FClosed: FnOnce() -> bool,
    {
        if !self.observed_property_names.contains(&property_name) {
            return;
        }

        let geometry = match self.entity.get_property(self.geometry_property_name) {
            Some(PropertyInput::Component(c)) => c,
            _ => {
                if self.fill_enabled || self.outline_enabled {
                    self.fill_enabled = false;
                    self.outline_enabled = false;
                    self.raise_changed();
                }
                return;
            }
        };

        let min_time = iso8601::minimum_value();
        let fill_property = geometry.get_property("fill");
        let fill_enabled = match fill_property.as_ref() {
            Some(slot) if is_slot_constant(Some(slot)) => eval_bool(Some(slot), &min_time, true).unwrap_or(true),
            _ => true,
        };

        let outline_property = geometry.get_property("outline");
        let mut outline_enabled = outline_property.is_some();
        if let Some(slot) = outline_property.as_ref() {
            if is_slot_constant(Some(slot)) {
                outline_enabled = eval_bool(Some(slot), &min_time, false).unwrap_or(false);
            }
        }

        if !fill_enabled && !outline_enabled {
            if self.fill_enabled || self.outline_enabled {
                self.fill_enabled = false;
                self.outline_enabled = false;
                self.raise_changed();
            }
            return;
        }

        if is_hidden(entity, &*geometry) {
            if self.fill_enabled || self.outline_enabled {
                self.fill_enabled = false;
                self.outline_enabled = false;
                self.raise_changed();
            }
            return;
        }

        let material_prop =
            extract_material_component(geometry.get_property("material")).unwrap_or_else(|| default_color_material());
        self.material_property = Some(material_prop);
        self.fill_property = fill_property.or_else(|| Some(PropertyInput::value(true)));
        self.show_property = geometry.get_property("show").or_else(|| Some(PropertyInput::value(true)));
        self.show_outline_property = outline_property.or_else(|| Some(PropertyInput::value(false)));
        self.outline_color_property = if outline_enabled {
            geometry.get_property("outlineColor").or_else(|| Some(PropertyInput::value(Color::BLACK)))
        } else {
            None
        };
        self.shadows_property = geometry
            .get_property("shadows")
            .or_else(|| Some(PropertyInput::value(ShadowMode::Disabled.value() as f64)));
        self.distance_display_condition_property = geometry.get_property("distanceDisplayCondition");
        self.classification_type_property = geometry
            .get_property("classificationType")
            .or_else(|| Some(PropertyInput::value(ClassificationType::Both.value() as f64)));

        self.fill_enabled = fill_enabled;

        let is_color_mat = self.material_property.as_ref().is_some_and(is_color_material_component);
        let on_terrain = is_on_terrain(self, entity, &*geometry)
            && (self.supports_materials_for_entities_on_terrain || is_color_mat);

        if outline_enabled && on_terrain {
            outline_enabled = false;
        }

        self.on_terrain = on_terrain;
        self.outline_enabled = outline_enabled;

        if is_dynamic(self, entity, &*geometry) {
            if !self.dynamic {
                self.dynamic = true;
                self.raise_changed();
            }
        } else {
            set_static_options(self, entity, &*geometry);
            self.is_closed = get_is_closed();
            self.outline_width =
                eval_f64_opt(geometry.get_property("outlineWidth").as_ref(), &min_time).ok().flatten().unwrap_or(1.0);
            self.dynamic = false;
            self.raise_changed();
        }
    }

    pub fn build_fill_attributes(
        &self,
        time: &JulianDate,
        offset_attribute: Option<GeometryOffsetAttribute>,
        terrain_offset: Option<&Rc<TerrainOffsetProperty>>,
    ) -> Result<Vec<(String, GeometryInstanceAttribute)>, RuntimeError> {
        let is_available = self.entity.is_available(time);
        let show = is_available
            && self.entity.is_showing()
            && eval_bool(self.show_property.as_ref(), time, true)?
            && eval_bool(self.fill_property.as_ref(), time, true)?;
        let ddc = eval_ddc_or_default(self.distance_display_condition_property.as_ref(), time)?;

        let mut attrs = vec![
            ("show".to_owned(), ShowGeometryInstanceAttribute::new(Some(show)).into()),
            (
                "distanceDisplayCondition".to_owned(),
                DistanceDisplayConditionGeometryInstanceAttribute::from_distance_display_condition(&ddc).into(),
            ),
        ];

        if let Some(mat) = &self.material_property {
            if let Some(color_mat) = mat.as_any().downcast_ref::<ColorMaterialProperty>() {
                let current_color = eval_color_material_color(color_mat, time, is_available)?;
                attrs.push(("color".to_owned(), ColorGeometryInstanceAttribute::from_color(&current_color).into()));
            }
        }

        if offset_attribute.is_some() {
            let offset = eval_terrain_offset_or_zero(terrain_offset, time)?;
            attrs.push(("offset".to_owned(), OffsetGeometryInstanceAttribute::from_cartesian3(&offset).into()));
        }

        Ok(attrs)
    }

    pub fn build_outline_attributes(
        &self,
        time: &JulianDate,
        offset_attribute: Option<GeometryOffsetAttribute>,
        terrain_offset: Option<&Rc<TerrainOffsetProperty>>,
    ) -> Result<Vec<(String, GeometryInstanceAttribute)>, RuntimeError> {
        let is_available = self.entity.is_available(time);
        let show = is_available
            && self.entity.is_showing()
            && eval_bool(self.show_property.as_ref(), time, true)?
            && eval_bool(self.show_outline_property.as_ref(), time, false)?;
        let outline_color = eval_color(self.outline_color_property.as_ref(), time, Color::BLACK)?;
        let ddc = eval_ddc_or_default(self.distance_display_condition_property.as_ref(), time)?;

        let mut attrs = vec![
            ("show".to_owned(), ShowGeometryInstanceAttribute::new(Some(show)).into()),
            ("color".to_owned(), ColorGeometryInstanceAttribute::from_color(&outline_color).into()),
            (
                "distanceDisplayCondition".to_owned(),
                DistanceDisplayConditionGeometryInstanceAttribute::from_distance_display_condition(&ddc).into(),
            ),
        ];

        if offset_attribute.is_some() {
            let offset = eval_terrain_offset_or_zero(terrain_offset, time)?;
            attrs.push(("offset".to_owned(), OffsetGeometryInstanceAttribute::from_cartesian3(&offset).into()));
        }

        Ok(attrs)
    }

    pub fn destroy(&mut self) {
        self.destroyed = true;
    }
}

// ---------------------------------------------------------------------------
// Shared property & value helpers for geometry updaters and batches
// ---------------------------------------------------------------------------

pub(crate) fn default_color_material() -> ComponentRef {
    color_material_component(Color::WHITE).expect("default white color material")
}

pub(crate) fn default_ddc() -> DistanceDisplayCondition {
    DistanceDisplayCondition::new(0.0, f64::MAX)
}

pub(crate) fn ddc_equals(a: Option<DistanceDisplayCondition>, b: DistanceDisplayCondition) -> bool {
    a.is_some_and(|a| a.near == b.near && a.far == b.far)
}

pub(crate) fn is_color_material_component(component: &ComponentRef) -> bool {
    component.as_any().is::<ColorMaterialProperty>()
}

pub(crate) fn extract_material_component(slot: Option<PropertyInput>) -> Option<ComponentRef> {
    match slot {
        Some(PropertyInput::Component(c)) => Some(c),
        Some(PropertyInput::Value(DynamicValue::Color(color))) => color_material_component(color).ok(),
        _ => None,
    }
}

pub(crate) fn eval_color_material_color(
    material: &ColorMaterialProperty,
    time: &JulianDate,
    is_available: bool,
) -> Result<Color, RuntimeError> {
    let color_slot = material.color();
    let current = if let Some(slot) = color_slot.as_ref() {
        if is_slot_constant(Some(slot)) || is_available {
            eval_color_opt(Some(slot), time)?
        } else {
            None
        }
    } else {
        None
    };
    Ok(current.unwrap_or(Color::WHITE))
}

pub(crate) fn eval_ddc_or_default(
    slot: Option<&PropertyInput>,
    time: &JulianDate,
) -> Result<DistanceDisplayCondition, RuntimeError> {
    Ok(eval_distance_display_condition_opt(slot, time)?.unwrap_or_else(default_ddc))
}

pub(crate) fn eval_terrain_offset_or_zero(
    prop: Option<&Rc<TerrainOffsetProperty>>,
    time: &JulianDate,
) -> Result<Cartesian3, RuntimeError> {
    match prop {
        Some(p) => Ok(p.get_value(Some(time), None)?.unwrap_or(Cartesian3::ZERO)),
        None => Ok(Cartesian3::ZERO),
    }
}

pub(crate) fn eval_classification_type(
    slot: Option<&PropertyInput>,
    time: &JulianDate,
    default: ClassificationType,
) -> Result<ClassificationType, RuntimeError> {
    let n = match eval_dynamic(slot, time)? {
        Some(DynamicValue::Number(n)) => n as i32,
        _ => return Ok(default),
    };
    Ok(ClassificationType::from_value(n).unwrap_or(default))
}

pub(crate) fn eval_arc_type(
    slot: Option<&PropertyInput>,
    time: &JulianDate,
    default: ArcType,
) -> Result<ArcType, RuntimeError> {
    let n = match eval_dynamic(slot, time)? {
        Some(DynamicValue::Number(n)) => n as i32,
        _ => return Ok(default),
    };
    Ok(match n {
        0 => ArcType::None,
        1 => ArcType::Geodesic,
        2 => ArcType::Rhumb,
        _ => default,
    })
}

pub(crate) fn eval_arc_type_opt(
    slot: Option<&PropertyInput>,
    time: &JulianDate,
) -> Result<Option<ArcType>, RuntimeError> {
    let n = match eval_dynamic(slot, time)? {
        Some(DynamicValue::Number(n)) => n as i32,
        _ => return Ok(None),
    };
    Ok(match n {
        0 => Some(ArcType::None),
        1 => Some(ArcType::Geodesic),
        2 => Some(ArcType::Rhumb),
        _ => None,
    })
}

pub(crate) fn eval_corner_type_opt(
    slot: Option<&PropertyInput>,
    time: &JulianDate,
) -> Result<Option<CornerType>, RuntimeError> {
    let n = match eval_dynamic(slot, time)? {
        Some(DynamicValue::Number(n)) => n as i32,
        _ => return Ok(None),
    };
    Ok(match n {
        0 => Some(CornerType::Rounded),
        1 => Some(CornerType::Mitered),
        2 => Some(CornerType::Beveled),
        _ => None,
    })
}

pub(crate) fn json_to_cartesian3(v: &Value) -> Option<Cartesian3> {
    if let Some(arr) = v.as_array() {
        if arr.len() >= 3 {
            return Some(Cartesian3::new(arr[0].as_f64()?, arr[1].as_f64()?, arr[2].as_f64()?));
        }
    }
    Some(Cartesian3::new(v.get("x")?.as_f64()?, v.get("y")?.as_f64()?, v.get("z")?.as_f64()?))
}

pub(crate) fn json_to_cartesian2(v: &Value) -> Option<Cartesian2> {
    if let Some(arr) = v.as_array() {
        if arr.len() >= 2 {
            return Some(Cartesian2::new(arr[0].as_f64()?, arr[1].as_f64()?));
        }
    }
    Some(Cartesian2::new(v.get("x")?.as_f64()?, v.get("y")?.as_f64()?))
}

pub(crate) fn eval_positions_opt(
    slot: Option<&PropertyInput>,
    time: &JulianDate,
) -> Result<Option<Vec<Cartesian3>>, RuntimeError> {
    Ok(match eval_dynamic(slot, time)? {
        Some(DynamicValue::Positions(p)) => Some((*p).clone()),
        Some(DynamicValue::Json(j)) => {
            j.value().as_array().map(|arr| arr.iter().filter_map(json_to_cartesian3).collect())
        }
        _ => None,
    })
}

pub(crate) fn eval_cartesian2_array_opt(
    slot: Option<&PropertyInput>,
    time: &JulianDate,
) -> Result<Option<Vec<Cartesian2>>, RuntimeError> {
    Ok(match eval_dynamic(slot, time)? {
        Some(DynamicValue::Json(j)) => {
            j.value().as_array().map(|arr| arr.iter().filter_map(json_to_cartesian2).collect())
        }
        _ => None,
    })
}

pub(crate) fn eval_f64_array_opt(
    slot: Option<&PropertyInput>,
    time: &JulianDate,
) -> Result<Option<Vec<f64>>, RuntimeError> {
    Ok(match eval_dynamic(slot, time)? {
        Some(DynamicValue::Json(j)) => j.value().as_array().map(|arr| arr.iter().filter_map(Value::as_f64).collect()),
        _ => None,
    })
}

pub(crate) fn eval_rectangle_opt(
    slot: Option<&PropertyInput>,
    time: &JulianDate,
) -> Result<Option<Rectangle>, RuntimeError> {
    Ok(match eval_dynamic(slot, time)? {
        Some(DynamicValue::Cartesian4(v)) => Some(Rectangle::new(v.x, v.y, v.z, v.w)),
        Some(DynamicValue::Json(j)) => {
            let v = j.value();
            if let Some(arr) = v.as_array() {
                if arr.len() >= 4 {
                    match (arr[0].as_f64(), arr[1].as_f64(), arr[2].as_f64(), arr[3].as_f64()) {
                        (Some(w), Some(s), Some(e), Some(n)) => return Ok(Some(Rectangle::new(w, s, e, n))),
                        _ => return Ok(None),
                    }
                }
            }
            match (
                v.get("west").and_then(Value::as_f64),
                v.get("south").and_then(Value::as_f64),
                v.get("east").and_then(Value::as_f64),
                v.get("north").and_then(Value::as_f64),
            ) {
                (Some(w), Some(s), Some(e), Some(n)) => Some(Rectangle::new(w, s, e, n)),
                _ => None,
            }
        }
        _ => None,
    })
}

pub(crate) fn eval_plane_opt(slot: Option<&PropertyInput>, time: &JulianDate) -> Result<Option<Plane>, RuntimeError> {
    Ok(match eval_dynamic(slot, time)? {
        Some(DynamicValue::Json(j)) => {
            let v = j.value();
            match (v.get("normal").and_then(json_to_cartesian3), v.get("distance").and_then(Value::as_f64)) {
                (Some(normal), Some(distance)) => Some(Plane::new(normal, distance)),
                _ => None,
            }
        }
        Some(DynamicValue::Record(fields)) => {
            let mut normal = None;
            let mut distance = None;
            for (k, val) in fields {
                let Some(val) = val else { continue };
                match k.as_str() {
                    "normal" => normal = dynamic_to_cartesian3(&val),
                    "distance" => {
                        if let DynamicValue::Number(d) = val {
                            distance = Some(d);
                        }
                    }
                    _ => {}
                }
            }
            match (normal, distance) {
                (Some(n), Some(d)) => Some(Plane::new(n, d)),
                _ => None,
            }
        }
        _ => None,
    })
}

fn json_to_polygon_hierarchy(v: &Value) -> Option<PolygonHierarchy> {
    if let Some(arr) = v.as_array() {
        let positions: Vec<Cartesian3> = arr.iter().filter_map(json_to_cartesian3).collect();
        return Some(PolygonHierarchy::new(positions, Vec::new()));
    }
    let positions: Vec<Cartesian3> = v.get("positions")?.as_array()?.iter().filter_map(json_to_cartesian3).collect();
    let holes: Vec<PolygonHierarchy> = v
        .get("holes")
        .and_then(Value::as_array)
        .map(|arr| arr.iter().filter_map(json_to_polygon_hierarchy).collect())
        .unwrap_or_default();
    Some(PolygonHierarchy::new(positions, holes))
}

pub(crate) fn eval_polygon_hierarchy_opt(
    slot: Option<&PropertyInput>,
    time: &JulianDate,
) -> Result<Option<PolygonHierarchy>, RuntimeError> {
    Ok(match eval_dynamic(slot, time)? {
        Some(DynamicValue::PolygonHierarchy(h)) => Some((*h).clone()),
        Some(DynamicValue::Positions(p)) => Some(PolygonHierarchy::new((*p).clone(), Vec::new())),
        Some(DynamicValue::Json(j)) => json_to_polygon_hierarchy(&j.value()),
        _ => None,
    })
}

fn json_to_texcoord_hierarchy(v: &Value) -> Option<TextureCoordinateHierarchy> {
    let positions: Vec<Cartesian2> = v.get("positions")?.as_array()?.iter().filter_map(json_to_cartesian2).collect();
    let holes: Vec<TextureCoordinateHierarchy> = v
        .get("holes")
        .and_then(Value::as_array)
        .map(|arr| arr.iter().filter_map(json_to_texcoord_hierarchy).collect())
        .unwrap_or_default();
    Some(TextureCoordinateHierarchy { positions, holes })
}

pub(crate) fn eval_texcoord_hierarchy_opt(
    slot: Option<&PropertyInput>,
    time: &JulianDate,
) -> Result<Option<TextureCoordinateHierarchy>, RuntimeError> {
    Ok(match eval_dynamic(slot, time)? {
        Some(DynamicValue::Json(j)) => json_to_texcoord_hierarchy(&j.value()),
        _ => None,
    })
}

/// Adapts a graphics [`PropertyInput`] slot into a typed `Property<HeightReference>`
/// for [`TerrainOffsetProperty`] and [`Entity::compute_model_matrix_for_height_reference`].
pub(crate) struct HeightReferenceSlotProperty {
    slot: PropertyInput,
    definition_changed: DefinitionChanged,
}

impl HeightReferenceSlotProperty {
    pub(crate) fn wrap(slot: PropertyInput) -> PropertyRef<HeightReference> {
        Rc::new(Self { slot, definition_changed: DefinitionChanged::new() })
    }
}

impl Property<HeightReference> for HeightReferenceSlotProperty {
    fn is_constant(&self) -> bool {
        is_slot_constant(Some(&self.slot))
    }

    fn definition_changed(&self) -> &DefinitionChanged {
        match &self.slot {
            PropertyInput::Component(c) => c.definition_changed(),
            PropertyInput::Value(_) => &self.definition_changed,
        }
    }

    fn get_value(
        &self,
        time: Option<&JulianDate>,
        result: Option<&mut HeightReference>,
    ) -> Result<Option<HeightReference>, RuntimeError> {
        let fallback = iso8601::minimum_value();
        let time = time.unwrap_or(&fallback);
        let value = eval_dynamic(Some(&self.slot), time)?.as_ref().and_then(dynamic_to_height_reference);
        Ok(super::property::write_result(value, result))
    }

    fn equals(&self, other: &dyn Property<HeightReference>) -> Result<bool, RuntimeError> {
        let Some(other) = other.as_any().downcast_ref::<Self>() else { return Ok(false) };
        super::entity_component::component_equals(Some(&self.slot), Some(&other.slot))
    }

    fn as_any(&self) -> &dyn Any {
        self
    }
}

pub(crate) fn bounding_sphere_from_primitive_attr(
    primitive: &GeometryBatchPrimitive,
    entity_id: &str,
    check_show: bool,
    result: &mut BoundingSphere,
) -> BoundingSphereState {
    if !primitive.is_ready() {
        return BoundingSphereState::Pending;
    }
    match primitive.bounding_sphere_for(entity_id, check_show) {
        Some(bs) => {
            *result = bs;
            BoundingSphereState::Done
        }
        None => BoundingSphereState::Failed,
    }
}

pub(crate) fn bounding_sphere_from_ground_attr(
    primitive: &GroundGeometryPrimitive,
    entity_id: &str,
    check_show: bool,
    result: &mut BoundingSphere,
) -> BoundingSphereState {
    if !primitive.is_ready() {
        return BoundingSphereState::Pending;
    }
    match primitive.bounding_sphere_for(entity_id, check_show) {
        Some(bs) => {
            *result = bs;
            BoundingSphereState::Done
        }
        None => BoundingSphereState::Failed,
    }
}

pub(crate) fn instance_alpha(instance: &GeometryInstance) -> u8 {
    for (name, attr) in &instance.attributes {
        if name == "color" {
            if let TypedArray::Uint8(v) = &attr.value {
                if v.len() >= 4 {
                    return v[3];
                }
            }
        }
    }
    255
}

pub(crate) fn instance_set_show_value(instance: &mut GeometryInstance, show: u8) {
    for (name, attr) in &mut instance.attributes {
        if name == "show" {
            if let TypedArray::Uint8(v) = &mut attr.value {
                if let Some(first) = v.first_mut() {
                    *first = show;
                }
            }
            break;
        }
    }
}
