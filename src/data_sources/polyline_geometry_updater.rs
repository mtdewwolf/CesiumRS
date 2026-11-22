//! CesiumJS `PolylineGeometryUpdater` and its dynamic polyline updater.
use super::{
    geometry_updater::{
        default_color_material, eval_arc_type, eval_arc_type_opt, eval_classification_type, eval_color_material_color,
        eval_ddc_or_default, eval_positions_opt, extract_material_component, is_color_material_component,
        GeometryAppearanceKind, GeometryChangedEvent, GeometryScene, GeometryUpdater, GroundGeometryPrimitive,
        SharedGroundPrimitive,
    },
    visualizer_values::*,
    BoundingSphereState, ChangeEvent, ColorMaterialProperty, ComponentRef, DynamicGeometryUpdater, Entity,
    EntityChange, EntityRef, MaterialProperty, PropertyInput, Subscription,
};
use crate::{
    core::{
        iso8601,
        math::RADIANS_PER_DEGREE,
        polyline_pipeline::{self, ArcOptions, PolylineHeight},
        ArcType, BoundingSphere, Cartesian3, ColorGeometryInstanceAttribute, DistanceDisplayCondition,
        DistanceDisplayConditionGeometryInstanceAttribute, GeometryInstance, GroundPolylineGeometry, JulianDate,
        Matrix4, PolylineGeometry, PolylineGeometryOptions, RuntimeError, ShowGeometryInstanceAttribute, VertexFormat,
    },
    scene::{
        polyline::{Polyline, PolylineOptions as RenderPolylineOptions},
        polyline_collection::RenderPolylines,
        Cesium3DTilePass, ClassificationType, FrameState, Material, OrderedGroundPrimitiveCollection,
        PolylineColorAppearance, PolylineMaterialAppearance, PrimitiveCollection, PrimitiveHandle, ScenePrimitive,
        ShadowMode,
    },
};
use std::{
    any::Any,
    cell::RefCell,
    collections::{BTreeMap, HashMap},
    rc::{Rc, Weak},
};

#[derive(Clone, Debug)]
struct PolylineOptions {
    vertex_format: VertexFormat,
    positions: Option<Vec<Cartesian3>>,
    width: Option<f64>,
    arc_type: Option<ArcType>,
    granularity: Option<f64>,
}

impl Default for PolylineOptions {
    fn default() -> Self {
        Self { vertex_format: VertexFormat::DEFAULT, positions: None, width: None, arc_type: None, granularity: None }
    }
}

#[derive(Clone, Debug, Default)]
struct GroundPolylineOptions {
    positions: Option<Vec<Cartesian3>>,
    width: Option<f64>,
    arc_type: Option<ArcType>,
    granularity: Option<f64>,
}

struct PolylineUpdaterState {
    id: String,
    entity: EntityRef,
    scene: Rc<dyn GeometryScene>,
    fill_enabled: bool,
    dynamic: bool,
    geometry_changed: ChangeEvent<GeometryChangedEvent>,
    show_property: Option<PropertyInput>,
    material_property: Option<ComponentRef>,
    depth_fail_material_property: Option<ComponentRef>,
    shadows_property: Option<PropertyInput>,
    distance_display_condition_property: Option<PropertyInput>,
    classification_type_property: Option<PropertyInput>,
    geometry_options: PolylineOptions,
    ground_geometry_options: GroundPolylineOptions,
    clamp_to_ground: bool,
    supports_polylines_on_terrain: bool,
    z_index: Option<PropertyInput>,
    arc_type: Option<ArcType>,
    destroyed: bool,
}

impl PolylineUpdaterState {
    fn raise_changed(&self) {
        self.geometry_changed
            .raise(GeometryChangedEvent { updater_id: self.id.clone(), entity: Rc::clone(&self.entity) });
    }

    fn clamp_to_ground(&self) -> bool {
        self.clamp_to_ground && self.supports_polylines_on_terrain
    }

    fn has_constant_fill(&self) -> bool {
        !self.fill_enabled || (self.entity.availability().is_none() && is_slot_constant(self.show_property.as_ref()))
    }

    fn is_filled(&self, time: &JulianDate) -> Result<bool, RuntimeError> {
        Ok(self.fill_enabled && self.entity.is_available(time) && eval_bool(self.show_property.as_ref(), time, true)?)
    }

    fn on_entity_property_changed(&mut self, _entity: &Entity, property_name: &str) {
        if !(property_name == "availability" || property_name == "polyline") {
            return;
        }

        let polyline = match self.entity.get_property("polyline") {
            Some(PropertyInput::Component(c)) => c,
            _ => {
                if self.fill_enabled {
                    self.fill_enabled = false;
                    self.raise_changed();
                }
                return;
            }
        };

        let positions_property = polyline.get_property("positions");
        let show_property = polyline.get_property("show");
        let min_time = iso8601::minimum_value();
        let show_const_false = show_property.as_ref().is_some_and(|slot| {
            is_slot_constant(Some(slot)) && !eval_bool(Some(slot), &min_time, true).unwrap_or(true)
        });

        if show_const_false || positions_property.is_none() {
            if self.fill_enabled {
                self.fill_enabled = false;
                self.raise_changed();
            }
            return;
        }

        let z_index = polyline.get_property("zIndex");
        let material =
            extract_material_component(polyline.get_property("material")).unwrap_or_else(default_color_material);
        let is_color_material = is_color_material_component(&material);
        self.material_property = Some(material);
        self.depth_fail_material_property = extract_material_component(polyline.get_property("depthFailMaterial"));
        self.show_property = show_property.or_else(|| Some(PropertyInput::value(true)));
        self.shadows_property = polyline
            .get_property("shadows")
            .or_else(|| Some(PropertyInput::value(ShadowMode::Disabled.value() as f64)));
        self.distance_display_condition_property = polyline.get_property("distanceDisplayCondition");
        self.classification_type_property = polyline
            .get_property("classificationType")
            .or_else(|| Some(PropertyInput::value(ClassificationType::Both.value() as f64)));
        self.fill_enabled = true;
        self.z_index = z_index.or_else(|| Some(PropertyInput::value(0.0)));

        let width = polyline.get_property("width");
        let arc_type = polyline.get_property("arcType");
        let clamp_to_ground = polyline.get_property("clampToGround");
        let granularity = polyline.get_property("granularity");

        if !is_slot_constant(positions_property.as_ref())
            || !is_slot_constant(width.as_ref())
            || !is_slot_constant(arc_type.as_ref())
            || !is_slot_constant(granularity.as_ref())
            || !is_slot_constant(clamp_to_ground.as_ref())
            || !is_slot_constant(self.z_index.as_ref())
        {
            if !self.dynamic {
                self.dynamic = true;
                self.raise_changed();
            }
        } else {
            let positions = eval_positions_opt(positions_property.as_ref(), &min_time).ok().flatten();
            let Some(positions) = positions.filter(|p| p.len() >= 2) else {
                if self.fill_enabled {
                    self.fill_enabled = false;
                    self.raise_changed();
                }
                return;
            };

            let vertex_format = if is_color_material
                && self.depth_fail_material_property.as_ref().is_none_or(is_color_material_component)
            {
                PolylineColorAppearance::VERTEX_FORMAT
            } else {
                PolylineMaterialAppearance::VERTEX_FORMAT
            };

            let width_val = eval_f64_opt(width.as_ref(), &min_time).ok().flatten();
            let arc_val = eval_arc_type_opt(arc_type.as_ref(), &min_time).ok().flatten();
            let gran_val = eval_f64_opt(granularity.as_ref(), &min_time).ok().flatten();

            self.geometry_options.vertex_format = vertex_format;
            self.geometry_options.positions = Some(positions.clone());
            self.geometry_options.width = width_val;
            self.geometry_options.arc_type = arc_val;
            self.geometry_options.granularity = gran_val;

            self.ground_geometry_options.positions = Some(positions);
            self.ground_geometry_options.width = width_val;
            self.ground_geometry_options.arc_type = arc_val;
            self.ground_geometry_options.granularity = gran_val;

            self.arc_type = arc_val;
            self.clamp_to_ground = eval_bool(clamp_to_ground.as_ref(), &min_time, false).unwrap_or(false);
            self.dynamic = false;
            self.raise_changed();
        }
    }

    fn create_fill_geometry_instance(&self, time: &JulianDate) -> Result<GeometryInstance, RuntimeError> {
        if !self.fill_enabled {
            return Err(RuntimeError::new("This instance does not represent a filled geometry."));
        }

        let is_available = self.entity.is_available(time);
        let show = is_available && self.entity.is_showing() && eval_bool(self.show_property.as_ref(), time, true)?;
        let ddc = eval_ddc_or_default(self.distance_display_condition_property.as_ref(), time)?;

        let mut attributes = vec![
            ("show".to_owned(), ShowGeometryInstanceAttribute::new(Some(show)).into()),
            (
                "distanceDisplayCondition".to_owned(),
                DistanceDisplayConditionGeometryInstanceAttribute::from_distance_display_condition(&ddc).into(),
            ),
        ];

        if let Some(mat) = &self.material_property {
            if let Some(color_mat) = mat.as_any().downcast_ref::<ColorMaterialProperty>() {
                let current_color = eval_color_material_color(color_mat, time, is_available)?;
                attributes
                    .push(("color".to_owned(), ColorGeometryInstanceAttribute::from_color(&current_color).into()));
            }
        }

        if self.clamp_to_ground() {
            let positions = self
                .ground_geometry_options
                .positions
                .clone()
                .ok_or_else(|| RuntimeError::new("polyline positions are required."))?;
            let geometry = GroundPolylineGeometry::new(positions).and_then(|mut g| {
                g.ellipsoid = self.scene.ellipsoid();
                g.width = self.ground_geometry_options.width.unwrap_or(1.0);
                g.arc_type = self.ground_geometry_options.arc_type.unwrap_or(ArcType::Geodesic);
                g.granularity = self.ground_geometry_options.granularity.unwrap_or(9999.0);
                g.create_geometry()
            });
            return Ok(GeometryInstance {
                geometry,
                model_matrix: Matrix4::IDENTITY,
                id: Some(self.entity.id().to_owned()),
                attributes,
                west_hemisphere_geometry: None,
                east_hemisphere_geometry: None,
            });
        }

        if let Some(df_mat) = &self.depth_fail_material_property {
            if let Some(color_mat) = df_mat.as_any().downcast_ref::<ColorMaterialProperty>() {
                let current_color = eval_color_material_color(color_mat, time, is_available)?;
                attributes.push((
                    "depthFailColor".to_owned(),
                    ColorGeometryInstanceAttribute::from_color(&current_color).into(),
                ));
            }
        }

        let positions = self
            .geometry_options
            .positions
            .clone()
            .ok_or_else(|| RuntimeError::new("polyline positions are required."))?;
        let mut geom_opts = PolylineGeometryOptions::new(positions);
        geom_opts.ellipsoid = self.scene.ellipsoid();
        geom_opts.vertex_format = self.geometry_options.vertex_format;
        geom_opts.width = self.geometry_options.width.unwrap_or(1.0);
        geom_opts.arc_type = self.geometry_options.arc_type.unwrap_or(ArcType::Geodesic);
        geom_opts.granularity = self.geometry_options.granularity.unwrap_or(RADIANS_PER_DEGREE);

        Ok(GeometryInstance {
            geometry: PolylineGeometry::new(geom_opts).create_geometry(),
            model_matrix: Matrix4::IDENTITY,
            id: Some(self.entity.id().to_owned()),
            attributes,
            west_hemisphere_geometry: None,
            east_hemisphere_geometry: None,
        })
    }
}

/// Port of CesiumJS `PolylineGeometryUpdater`.
pub struct PolylineGeometryUpdater {
    id: String,
    entity: EntityRef,
    geometry_changed: ChangeEvent<GeometryChangedEvent>,
    state: Rc<RefCell<PolylineUpdaterState>>,
    entity_subscription: Option<Subscription>,
}

impl PolylineGeometryUpdater {
    pub fn new(entity: EntityRef, scene: Rc<dyn GeometryScene>) -> Self {
        let id = format!("polyline-{}", entity.id());
        let geometry_changed = ChangeEvent::new();
        let supports_polylines = scene.supports_polylines_on_terrain();
        let state = Rc::new(RefCell::new(PolylineUpdaterState {
            id: id.clone(),
            entity: Rc::clone(&entity),
            scene,
            fill_enabled: false,
            dynamic: false,
            geometry_changed: geometry_changed.clone(),
            show_property: None,
            material_property: None,
            depth_fail_material_property: None,
            shadows_property: None,
            distance_display_condition_property: None,
            classification_type_property: None,
            geometry_options: PolylineOptions::default(),
            ground_geometry_options: GroundPolylineOptions::default(),
            clamp_to_ground: false,
            supports_polylines_on_terrain: supports_polylines,
            z_index: Some(PropertyInput::value(0.0)),
            arc_type: None,
            destroyed: false,
        }));

        let weak_state: Weak<RefCell<PolylineUpdaterState>> = Rc::downgrade(&state);
        let entity_subscription = entity.definition_changed().add_listener(move |change: &EntityChange| {
            let Some(state) = weak_state.upgrade() else { return };
            let Some(changed_entity) = change.entity.upgrade() else { return };
            state.borrow_mut().on_entity_property_changed(&changed_entity, &change.property.name);
        });

        state.borrow_mut().on_entity_property_changed(&entity, "polyline");

        Self { id, entity, geometry_changed, state, entity_subscription: Some(entity_subscription) }
    }

    pub fn arc_type(&self) -> Option<ArcType> {
        self.state.borrow().arc_type
    }
}

impl GeometryUpdater for PolylineGeometryUpdater {
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
        Rc::clone(&self.state.borrow().scene)
    }

    fn fill_enabled(&self) -> bool {
        self.state.borrow().fill_enabled
    }

    fn has_constant_fill(&self) -> bool {
        self.state.borrow().has_constant_fill()
    }

    fn fill_material_property(&self) -> Option<ComponentRef> {
        self.state.borrow().material_property.clone()
    }

    fn depth_fail_material_property(&self) -> Option<ComponentRef> {
        self.state.borrow().depth_fail_material_property.clone()
    }

    fn outline_enabled(&self) -> bool {
        false
    }

    fn has_constant_outline(&self) -> bool {
        true
    }

    fn outline_color_property(&self) -> Option<PropertyInput> {
        None
    }

    fn outline_width(&self) -> f64 {
        1.0
    }

    fn shadows_property(&self) -> Option<PropertyInput> {
        self.state.borrow().shadows_property.clone()
    }

    fn distance_display_condition_property(&self) -> Option<PropertyInput> {
        self.state.borrow().distance_display_condition_property.clone()
    }

    fn classification_type_property(&self) -> Option<PropertyInput> {
        self.state.borrow().classification_type_property.clone()
    }

    fn z_index(&self) -> Option<PropertyInput> {
        self.state.borrow().z_index.clone()
    }

    fn is_dynamic(&self) -> bool {
        self.state.borrow().dynamic
    }

    fn is_closed(&self) -> bool {
        false
    }

    fn on_terrain(&self) -> bool {
        false
    }

    fn clamp_to_ground(&self) -> bool {
        self.state.borrow().clamp_to_ground()
    }

    fn geometry_changed(&self) -> &ChangeEvent<GeometryChangedEvent> {
        &self.geometry_changed
    }

    fn is_filled(&self, time: &JulianDate) -> Result<bool, RuntimeError> {
        self.state.borrow().is_filled(time)
    }

    fn is_outline_visible(&self, _time: &JulianDate) -> Result<bool, RuntimeError> {
        Ok(false)
    }

    fn create_fill_geometry_instance(&self, time: &JulianDate) -> Result<GeometryInstance, RuntimeError> {
        self.state.borrow().create_fill_geometry_instance(time)
    }

    fn create_outline_geometry_instance(&self, _time: &JulianDate) -> Result<GeometryInstance, RuntimeError> {
        Err(RuntimeError::new("This instance does not represent an outlined geometry."))
    }

    fn create_dynamic_updater(
        &self,
        primitives: Rc<RefCell<PrimitiveCollection>>,
        ground_primitives: Rc<RefCell<OrderedGroundPrimitiveCollection>>,
    ) -> Result<Box<dyn DynamicGeometryUpdater>, RuntimeError> {
        if !self.is_dynamic() {
            return Err(RuntimeError::new("This instance does not represent dynamic geometry."));
        }
        Ok(Box::new(DynamicPolylineGeometryUpdater {
            state: Rc::clone(&self.state),
            primitives,
            ground_primitives,
            line_id: None,
            ground_polyline_primitive: None,
            material: None,
            destroyed: false,
        }))
    }

    fn on_entity_property_changed(&mut self, entity: &Entity, property_name: &str) {
        self.state.borrow_mut().on_entity_property_changed(entity, property_name);
    }

    fn is_destroyed(&self) -> bool {
        self.state.borrow().destroyed
    }

    fn destroy(&mut self) {
        self.entity_subscription = None;
        self.state.borrow_mut().destroyed = true;
    }
}

// ---------------------------------------------------------------------------
// Shared per-(scene, PrimitiveCollection) dynamic polyline collection
// ---------------------------------------------------------------------------

/// Entry stored inside [`DynamicPolylineCollection`].
#[derive(Clone, Debug)]
pub struct DynamicPolylineEntry {
    pub id: String,
    pub show: bool,
    pub positions: Vec<Cartesian3>,
    pub material: Option<Material>,
    pub width: f64,
    pub distance_display_condition: Option<DistanceDisplayCondition>,
}

/// Host-independent collection of dynamic non-clamped polylines implementing
/// [`ScenePrimitive`], mirroring CesiumJS's per-scene `PolylineCollection`.
pub struct DynamicPolylineCollection {
    next_id: u64,
    lines: BTreeMap<u64, DynamicPolylineEntry>,
    renderer: Option<Box<dyn RenderPolylines>>,
    rendered_lines: HashMap<u64, Polyline>,
    destroyed: bool,
}

impl DynamicPolylineCollection {
    fn new() -> Self {
        Self { next_id: 1, lines: BTreeMap::new(), renderer: None, rendered_lines: HashMap::new(), destroyed: false }
    }

    fn add(&mut self, entity_id: String) -> u64 {
        let id = self.next_id;
        self.next_id += 1;
        self.lines.insert(
            id,
            DynamicPolylineEntry {
                id: entity_id,
                show: true,
                positions: Vec::new(),
                material: None,
                width: 1.0,
                distance_display_condition: None,
            },
        );
        id
    }

    fn get(&self, id: u64) -> Option<&DynamicPolylineEntry> {
        self.lines.get(&id)
    }

    fn get_mut(&mut self, id: u64) -> Option<&mut DynamicPolylineEntry> {
        self.lines.get_mut(&id)
    }

    fn remove(&mut self, id: u64) -> bool {
        if let (Some(renderer), Some(line)) = (&mut self.renderer, self.rendered_lines.remove(&id)) {
            let _ = renderer.remove_polyline(&line);
        }
        self.lines.remove(&id).is_some()
    }

    pub fn len(&self) -> usize {
        self.lines.len()
    }

    pub fn is_empty(&self) -> bool {
        self.lines.is_empty()
    }

    pub fn is_destroyed(&self) -> bool {
        self.destroyed
    }
}

impl ScenePrimitive for DynamicPolylineCollection {
    fn pre_passes_update(&mut self, _: &mut FrameState) -> Result<(), RuntimeError> {
        Ok(())
    }

    fn update(&mut self, frame: &mut FrameState) -> Result<(), RuntimeError> {
        let Some(create) = frame.create_polylines.filter(|_| !self.destroyed) else { return Ok(()) };
        let renderer = self.renderer.get_or_insert_with(create);
        for (&id, entry) in &self.lines {
            if let Some(line) = self.rendered_lines.get(&id) {
                line.set_show(entry.show);
                line.set_width(entry.width);
                line.set_distance_display_condition(entry.distance_display_condition)?;
                if line.positions() != entry.positions {
                    line.set_positions(entry.positions.clone());
                }
                if let Some(material) = &entry.material {
                    let current = line.material();
                    if current.borrow().type_name == material.type_name {
                        *current.borrow_mut() = material.clone();
                    } else {
                        line.set_material(Rc::new(RefCell::new(material.clone())));
                    }
                }
            } else {
                let line = renderer.add_polyline(RenderPolylineOptions {
                    id: Some(entry.id.clone()),
                    show: Some(entry.show),
                    width: Some(entry.width),
                    positions: entry.positions.clone(),
                    distance_display_condition: entry.distance_display_condition,
                    material: entry.material.clone().map(|material| Rc::new(RefCell::new(material))),
                    ..Default::default()
                })?;
                self.rendered_lines.insert(id, line);
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
        self.lines.clear();
        self.rendered_lines.clear();
        self.renderer.take().map_or(Ok(()), |mut renderer| renderer.destroy())
    }
}

struct SharedPolylineSlot {
    collection: Weak<RefCell<DynamicPolylineCollection>>,
    handle: PrimitiveHandle,
}

thread_local! {
    static POLYLINE_COLLECTIONS: RefCell<HashMap<(String, usize), SharedPolylineSlot>> = RefCell::new(HashMap::new());
}

fn collection_key(scene: &Rc<dyn GeometryScene>, primitives: &Rc<RefCell<PrimitiveCollection>>) -> (String, usize) {
    (scene.id().to_owned(), Rc::as_ptr(primitives) as usize)
}

fn get_or_create_collection(
    scene: &Rc<dyn GeometryScene>,
    primitives: &Rc<RefCell<PrimitiveCollection>>,
) -> Result<Rc<RefCell<DynamicPolylineCollection>>, RuntimeError> {
    let key = collection_key(scene, primitives);
    let existing = POLYLINE_COLLECTIONS.with(|map| {
        map.borrow().get(&key).and_then(|slot| {
            let col = slot.collection.upgrade()?;
            if col.borrow().is_destroyed() {
                return None;
            }
            Some((col, slot.handle.clone()))
        })
    });

    if let Some((col, handle)) = existing {
        if !primitives.borrow().contains(&handle) {
            primitives.borrow_mut().add(handle, None)?;
        }
        return Ok(col);
    }

    let col = Rc::new(RefCell::new(DynamicPolylineCollection::new()));
    let handle = PrimitiveHandle::from_rc(Rc::clone(&col) as Rc<RefCell<dyn ScenePrimitive>>);
    primitives.borrow_mut().add(handle.clone(), None)?;
    POLYLINE_COLLECTIONS.with(|map| {
        map.borrow_mut().insert(key, SharedPolylineSlot { collection: Rc::downgrade(&col), handle });
    });
    Ok(col)
}

/// Dynamic updater created by [`PolylineGeometryUpdater::create_dynamic_updater`].
pub struct DynamicPolylineGeometryUpdater {
    state: Rc<RefCell<PolylineUpdaterState>>,
    primitives: Rc<RefCell<PrimitiveCollection>>,
    ground_primitives: Rc<RefCell<OrderedGroundPrimitiveCollection>>,
    line_id: Option<u64>,
    ground_polyline_primitive: Option<(Rc<RefCell<GroundGeometryPrimitive>>, u64)>,
    material: Option<Material>,
    destroyed: bool,
}

impl DynamicPolylineGeometryUpdater {
    fn ensure_line(&mut self) -> Result<(Rc<RefCell<DynamicPolylineCollection>>, u64), RuntimeError> {
        let (scene, entity_id) = {
            let st = self.state.borrow();
            (Rc::clone(&st.scene), st.entity.id().to_owned())
        };
        let col = get_or_create_collection(&scene, &self.primitives)?;
        if let Some(id) = self.line_id {
            if col.borrow().get(id).is_some() {
                return Ok((col, id));
            }
        }
        let id = col.borrow_mut().add(entity_id);
        self.line_id = Some(id);
        Ok((col, id))
    }
}

impl DynamicGeometryUpdater for DynamicPolylineGeometryUpdater {
    fn update(&mut self, time: &JulianDate) -> Result<(), RuntimeError> {
        let entity = Rc::clone(&self.state.borrow().entity);
        let polyline = match entity.get_property("polyline") {
            Some(PropertyInput::Component(c)) => c,
            _ => return Ok(()),
        };

        let mut positions = eval_positions_opt(polyline.get_property("positions").as_ref(), time)?;
        let clamp = eval_bool(polyline.get_property("clampToGround").as_ref(), time, false)?;
        let width = eval_f64(polyline.get_property("width").as_ref(), time, 1.0)?;
        let arc_type = eval_arc_type(polyline.get_property("arcType").as_ref(), time, ArcType::Geodesic)?;
        let ground_granularity = eval_f64(polyline.get_property("granularity").as_ref(), time, 9999.0)?;

        {
            let mut st = self.state.borrow_mut();
            st.clamp_to_ground = clamp;
            st.ground_geometry_options.positions = positions.clone();
            st.ground_geometry_options.width = Some(width);
            st.ground_geometry_options.arc_type = Some(arc_type);
            st.ground_geometry_options.granularity = Some(ground_granularity);
        }

        if let Some((_, handle)) = self.ground_polyline_primitive.take() {
            let _ = self.ground_primitives.borrow_mut().remove(handle, false);
        }

        let show_prop = eval_bool(polyline.get_property("show").as_ref(), time, true)?;
        let is_visible = entity.is_showing() && entity.is_available(time) && show_prop;

        let clamped_and_supported = self.state.borrow().clamp_to_ground();
        if clamped_and_supported {
            if !is_visible || positions.as_ref().is_none_or(|p| p.len() < 2) {
                return Ok(());
            }

            let (fill_mat, classification, z_index, instance) = {
                let st = self.state.borrow();
                let mat = st.material_property.clone();
                let cls =
                    eval_classification_type(st.classification_type_property.as_ref(), time, ClassificationType::Both)?;
                let z = eval_f64_opt(st.z_index.as_ref(), time)?.unwrap_or(0.0);
                let inst = st.create_fill_geometry_instance(time)?;
                (mat, cls, z, inst)
            };

            let appearance = if fill_mat.as_ref().is_some_and(is_color_material_component) {
                GeometryAppearanceKind::PolylineColor.build(true, false, false, None, None)
            } else {
                let mat = MaterialProperty::get_value(Some(time), fill_mat.as_deref(), self.material.take())?;
                let translucent = mat.is_translucent();
                self.material = Some(mat.clone());
                GeometryAppearanceKind::PolylineMaterial.build(translucent, false, false, None, Some(mat))
            };

            let gp = Rc::new(RefCell::new(GroundGeometryPrimitive::new(
                vec![instance],
                Some(appearance),
                classification,
                ShadowMode::Disabled,
                true,
                false,
                true,
            )));
            let handle =
                self.ground_primitives.borrow_mut().add(Box::new(SharedGroundPrimitive(Rc::clone(&gp))), z_index)?;
            self.ground_polyline_primitive = Some((gp, handle));

            if let Some(id) = self.line_id {
                let scene = Rc::clone(&self.state.borrow().scene);
                let key = collection_key(&scene, &self.primitives);
                POLYLINE_COLLECTIONS.with(|map| {
                    if let Some(col) = map.borrow().get(&key).and_then(|s| s.collection.upgrade()) {
                        if let Some(line) = col.borrow_mut().get_mut(id) {
                            line.show = false;
                        }
                    }
                });
            }
            return Ok(());
        }

        let (col, line_id) = self.ensure_line()?;
        if !is_visible {
            if let Some(line) = col.borrow_mut().get_mut(line_id) {
                line.show = false;
            }
            return Ok(());
        }
        let Some(mut pts) = positions.take().filter(|p| p.len() >= 2) else {
            if let Some(line) = col.borrow_mut().get_mut(line_id) {
                line.show = false;
            }
            return Ok(());
        };

        let (has_globe, ellipsoid, fill_mat) = {
            let st = self.state.borrow();
            (st.scene.has_globe(), st.scene.ellipsoid(), st.material_property.clone())
        };
        if arc_type != ArcType::None && has_globe {
            let granularity = eval_f64_opt(polyline.get_property("granularity").as_ref(), time)?;
            let heights = polyline_pipeline::extract_heights(&pts, &ellipsoid);
            let arc_opts = ArcOptions {
                positions: pts.clone(),
                ellipsoid,
                height: PolylineHeight::PerPosition(heights),
                granularity,
                min_distance: None,
            };
            let generated = if arc_type == ArcType::Geodesic {
                polyline_pipeline::generate_cartesian_arc(&arc_opts)
            } else {
                polyline_pipeline::generate_cartesian_rhumb_arc(&arc_opts)
            };
            if let Some(densified) = generated {
                pts = densified;
            }
        }

        let ddc =
            eval_distance_display_condition_opt(polyline.get_property("distanceDisplayCondition").as_ref(), time)?;
        let mut col_ref = col.borrow_mut();
        if let Some(line) = col_ref.get_mut(line_id) {
            line.show = true;
            line.positions = pts;
            line.material = Some(MaterialProperty::get_value(Some(time), fill_mat.as_deref(), line.material.take())?);
            line.width = width;
            line.distance_display_condition = ddc;
        }
        Ok(())
    }

    fn get_bounding_sphere(&self, result: &mut BoundingSphere) -> BoundingSphereState {
        let clamped = self.state.borrow().clamp_to_ground();
        if !clamped {
            let scene = Rc::clone(&self.state.borrow().scene);
            let key = collection_key(&scene, &self.primitives);
            if let Some(line_id) = self.line_id {
                let found = POLYLINE_COLLECTIONS.with(|map| {
                    let col = map.borrow().get(&key)?.collection.upgrade()?;
                    let col = col.borrow();
                    let line = col.get(line_id)?;
                    if line.show && !line.positions.is_empty() {
                        Some(BoundingSphere::from_points(&line.positions))
                    } else {
                        None
                    }
                });
                if let Some(bs) = found {
                    *result = bs;
                    return BoundingSphereState::Done;
                }
            }
            return BoundingSphereState::Failed;
        }

        if let Some((gp, _)) = &self.ground_polyline_primitive {
            let gp = gp.borrow();
            let entity_id = self.state.borrow().entity.id().to_owned();
            if gp.show && gp.is_ready() {
                if let Some(bs) = gp.bounding_sphere_for(&entity_id, false) {
                    *result = bs;
                    return BoundingSphereState::Done;
                }
            }
            if !gp.is_ready() {
                return BoundingSphereState::Pending;
            }
        }
        BoundingSphereState::Done
    }

    fn is_destroyed(&self) -> bool {
        self.destroyed
    }

    fn destroy(&mut self) {
        if !self.destroyed {
            let scene = Rc::clone(&self.state.borrow().scene);
            let key = collection_key(&scene, &self.primitives);
            let remove_handle = POLYLINE_COLLECTIONS.with(|map| {
                let mut map = map.borrow_mut();
                let mut empty_handle = None;
                if let Some(slot) = map.get(&key) {
                    if let Some(col) = slot.collection.upgrade() {
                        if let Some(line_id) = self.line_id.take() {
                            col.borrow_mut().remove(line_id);
                        }
                        if col.borrow().is_empty() {
                            empty_handle = Some(slot.handle.clone());
                        }
                    }
                }
                if empty_handle.is_some() {
                    map.remove(&key);
                }
                empty_handle
            });
            if let Some(handle) = remove_handle {
                let _ = self.primitives.borrow_mut().remove_and_destroy(&handle);
            }
            if let Some((_, handle)) = self.ground_polyline_primitive.take() {
                let _ = self.ground_primitives.borrow_mut().remove(handle, false);
            }
            self.destroyed = true;
        }
    }
}
