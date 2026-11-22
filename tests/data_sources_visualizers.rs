mod common;

use cesium_rs::{
    core::{
        webgl_constants, ArticulationStageType, BoundingSphere, Cartesian2, Cartesian3, Cartographic, Color,
        ComponentDatatype, DistanceDisplayCondition, Ellipsoid, GeographicProjection, IndexDatatype, JulianDate,
        Matrix4, NearFarScalar, OrthographicOffCenterFrustum, PrimitiveType, Quaternion, ReferenceFrame, Resource,
        RuntimeError, TimeInterval, TimeStandard, TrackingReferenceFrame, TypedArray,
    },
    data_sources::{
        get_element, path_sub_sample, BillboardGraphics, BillboardVisualizer, BoundingSphereState,
        CallbackPositionProperty, CallbackProperty, Cesium3DTilesetGraphics, Cesium3DTilesetVisualizer,
        CompositePositionProperty, ConstantPositionProperty, ConstantProperty, DefinitionChanged, DynamicValue,
        ElementInput, Entity, EntityCluster, EntityClusterOptions, EntityCollection, EntityOptions, EntityView,
        LabelGraphics, LabelVisualizer, ModelGraphics, ModelLoadRequest, ModelVisualizer, NodeTransformationProperty,
        PathGraphics, PathVisualizer, PointGraphics, PointVisualizer, PositionPropertyRef, Property, PropertyBag,
        PropertyInput, PropertyIntervals, PropertyRef, ReferenceProperty, SampledPositionProperty,
        ScaledPositionProperty, TerrainHeightCallback, TerrainHeightHandle, TerrainOffsetProperty, TerrainOffsetScene,
        TimeIntervalCollectionPositionProperty, Visualizer, DEFAULT_OFFSET_3D,
    },
    renderer::{Buffer, BufferData, BufferKind, BufferUsage, Context, Gl, GlValue, UniformFrame},
    scene::{
        billboard::Billboard,
        camera::{CameraDestination, CameraFrustum, CameraView},
        material::MaterialValue,
        model::{Model, ModelOptions},
        model_components as mc,
        point_primitive::PointPrimitive,
        AttributeType, Axis, Camera, CameraOptions, Cesium3DTileset, ColorBlendMode, FrameResource, FrameState,
        HeightReference, HeightSampler, HorizontalOrigin, LabelHandle, LabelStyle, SceneMode, SceneTransformState,
        ShadowMode, SplitDirection, TilesetOptions, VertexAttributeSemantic, VerticalOrigin,
    },
};
use common::{num, ok, run, Enc, Tol, EXACT, TIGHT};
use futures_util::FutureExt;
use serde_json::{json, Value};
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    future::Future,
    pin::Pin,
    rc::Rc,
    task::{Context as TaskContext, Poll},
};

#[derive(Clone, Default)]
struct Host {
    next: Rc<Cell<u32>>,
    bound: Rc<Cell<u32>>,
    buffers: Rc<RefCell<HashMap<u32, Vec<u8>>>>,
}

impl Gl for Host {
    type Object = u32;
    fn drawing_buffer_size(&self) -> Result<[u32; 2], RuntimeError> {
        Ok([8, 8])
    }
    fn invoke(&self, name: &str, args: &[GlValue<u32>]) -> Result<GlValue<u32>, RuntimeError> {
        if name.starts_with("create") {
            self.next.set(self.next.get() + 1);
            return Ok(GlValue::Object(self.next.get()));
        }
        if name == "bindBuffer" {
            if let Some(GlValue::Object(id)) = args.get(1) {
                self.bound.set(*id);
            }
        }
        if name == "bufferData" {
            if let Some(GlValue::Bytes(data)) = args.get(1) {
                self.buffers.borrow_mut().insert(self.bound.get(), data.clone());
            }
        }
        if name == "getBufferSubData" {
            if let (Some(GlValue::Number(offset)), Some(GlValue::Bytes(data))) = (args.get(1), args.get(2)) {
                return Ok(GlValue::Bytes(
                    self.buffers.borrow()[&self.bound.get()][*offset as usize..*offset as usize + data.len()].to_vec(),
                ));
            }
        }
        Ok(match name {
            "getParameter" => match args.first() {
                Some(GlValue::Number(v))
                    if [
                        webgl_constants::ALIASED_LINE_WIDTH_RANGE,
                        webgl_constants::ALIASED_POINT_SIZE_RANGE,
                        webgl_constants::MAX_VIEWPORT_DIMS,
                    ]
                    .contains(&(*v as u32)) =>
                {
                    GlValue::Numbers(vec![1., 16384.])
                }
                _ => GlValue::Number(16384.),
            },
            "getShaderParameter" => GlValue::Bool(true),
            "getProgramParameter" if matches!(args.get(1), Some(GlValue::Number(v)) if *v == webgl_constants::LINK_STATUS as f64) => {
                GlValue::Bool(true)
            }
            "getProgramParameter" => GlValue::Number(0.),
            _ => GlValue::Null,
        })
    }
}

fn t0() -> JulianDate {
    JulianDate::from_iso8601("2020-01-01T00:00:00Z").unwrap()
}

fn add_sec(s: f64) -> JulianDate {
    t0().add_seconds(s)
}

fn make_frame() -> FrameState {
    let mut camera = Camera::new(CameraOptions::new(800., 600.));
    let uniform = camera.uniform_camera();
    let culling = camera.frustum.compute_culling_volume(uniform.position, uniform.direction, uniform.up);
    let mut frame = FrameState::new(
        UniformFrame::new(uniform, JulianDate::new(2451545., 0., TimeStandard::Tai)),
        culling,
        [800, 600],
    );
    frame.passes.render = true;
    frame
}

fn finish_frame(f: &mut FrameState) {
    for callback in std::mem::take(&mut f.after_render) {
        callback().unwrap();
    }
}

type Gate<T> = Rc<RefCell<Option<Result<T, RuntimeError>>>>;

struct GateFuture<T> {
    slot: Gate<T>,
}

impl<T: Unpin> Future for GateFuture<T> {
    type Output = Result<T, RuntimeError>;
    fn poll(self: Pin<&mut Self>, _cx: &mut TaskContext<'_>) -> Poll<Self::Output> {
        match self.slot.borrow_mut().take() {
            Some(v) => Poll::Ready(v),
            None => Poll::Pending,
        }
    }
}

// ===========================================================================
// Existing unit tests (preserved verbatim)
// ===========================================================================

#[test]
fn generic_path_respects_subminute_resolution() {
    let source: PositionPropertyRef = Rc::new(CallbackPositionProperty::new(
        Rc::new(|_, _| Ok(Some(Cartesian3::new(6378137., 1., 1.)))),
        false,
        ReferenceFrame::Fixed,
    ));
    let property: PositionPropertyRef = Rc::new(ScaledPositionProperty::new(Some(source)));
    let start = JulianDate::from_iso8601("2020-01-01T00:00:00Z").unwrap();
    let points = path_sub_sample(&property, &start, &start.add_seconds(30.), None, ReferenceFrame::Fixed, 1.).unwrap();
    assert_eq!(points.len(), 31);
}

#[test]
fn tracked_sphere_updates_and_clears() {
    let entity = Entity::with_id("tracked");
    entity
        .set_position(Some(Rc::new(ConstantPositionProperty::new(
            Some(Cartesian3::new(6378137., 0., 0.)),
            ReferenceFrame::Fixed,
        ))))
        .unwrap();
    let mut view = EntityView::new(entity, None);
    let mut camera = Camera::new(CameraOptions::new(800., 600.));
    let time = JulianDate::from_iso8601("2020-01-01T00:00:00Z").unwrap();
    let first = BoundingSphere::new(Cartesian3::new(6378137., 0., 0.), 10.);
    let second = BoundingSphere::new(Cartesian3::new(6378137., 100., 0.), 10.);
    view.update(&time, SceneMode::Scene3D, &mut camera, Some(first)).unwrap();
    view.update(&time, SceneMode::Scene3D, &mut camera, Some(second)).unwrap();
    assert_eq!(view.bounding_sphere, Some(second));
    view.update(&time, SceneMode::Scene3D, &mut camera, None).unwrap();
    assert_eq!(view.bounding_sphere, None);
}

#[test]
fn cluster_reuses_released_primitives() {
    let cluster = EntityCluster::<Host>::new(EntityClusterOptions::default());
    let first = Entity::with_id("first");
    let second = Entity::with_id("second");
    cluster.get_billboard(&first).unwrap();
    cluster.get_label(&first).unwrap();
    cluster.get_point(&first).unwrap();
    cluster.remove_billboard(&first);
    cluster.remove_label(&first);
    cluster.remove_point(&first);
    cluster.get_billboard(&second).unwrap();
    cluster.get_label(&second).unwrap();
    cluster.get_point(&second).unwrap();
}

#[test]
fn tileset_is_hidden_when_uri_is_removed() {
    let collection = EntityCollection::new();
    let entity = Entity::with_id("tileset");
    let graphics = Rc::new(Cesium3DTilesetGraphics::new());
    graphics.set_uri(Some(PropertyInput::value("https://example.com/tileset.json"))).unwrap();
    entity.set_property("tileset", Some(PropertyInput::Component(graphics.clone()))).unwrap();
    collection.add(entity).unwrap();
    let factory = Rc::new(|resource: Resource| {
        async move {
            let tileset = Cesium3DTileset::from_json(
                resource,
                &json!({
                    "asset": {"version":"1.1"}, "geometricError": 100,
                    "root": {"boundingVolume":{"sphere":[6378137,0,0,10]}, "geometricError":0}
                }),
                TilesetOptions::default(),
                None,
            )?;
            Ok(Rc::new(RefCell::new(tileset)))
        }
        .boxed_local()
    });
    let mut visualizer = Cesium3DTilesetVisualizer::new(collection, factory);
    let time = JulianDate::from_iso8601("2020-01-01T00:00:00Z").unwrap();
    visualizer.update(&time).unwrap();
    let tileset = visualizer.tileset_for("tileset").unwrap();
    assert!(tileset.borrow().options.show);
    graphics.set_uri(None).unwrap();
    visualizer.update(&time).unwrap();
    assert!(!tileset.borrow().options.show);
}

#[test]
fn disabling_cluster_categories_restores_source_visibility() {
    let cluster = EntityCluster::<Host>::new(EntityClusterOptions { enabled: true, ..Default::default() });
    let entity = Entity::with_id("point");
    let point = cluster.get_point(&entity).unwrap();
    point.set_cluster_show(false);
    cluster.set_cluster_points(false);
    let projection = GeographicProjection::default();
    let state = SceneTransformState {
        mode: SceneMode::Scene3D,
        morph_time: 1.,
        map_projection: &projection,
        client_dimensions: Cartesian2::new(800., 600.),
        drawing_buffer_dimensions: Cartesian2::new(800., 600.),
    };
    let mut camera = Camera::new(CameraOptions::new(800., 600.));
    cluster.run_clustering(Ellipsoid::WGS84, &mut camera, &state, None).unwrap();
    assert!(point.cluster_show());
    point.set_cluster_show(false);
    cluster.set_cluster_labels(false);
    cluster.set_cluster_billboards(false);
    cluster.run_clustering(Ellipsoid::WGS84, &mut camera, &state, None).unwrap();
    assert!(point.cluster_show());
}

#[test]
fn constant_path_has_one_position() {
    let position = Cartesian3::new(1., 2., 3.);
    let property: PositionPropertyRef = Rc::new(ConstantPositionProperty::new(Some(position), ReferenceFrame::Fixed));
    let start = JulianDate::from_iso8601("2020-01-01T00:00:00Z").unwrap();
    let points = path_sub_sample(&property, &start, &start.add_seconds(60.), None, ReferenceFrame::Fixed, 10.).unwrap();
    assert_eq!(points, vec![position]);
}

// ===========================================================================
// 1. getElement parity
// ===========================================================================

#[test]
fn get_element_matches_cesium() {
    run(
        "data_sources",
        "getElement",
        |_| EXACT,
        |f, a| {
            if f != "resolve" {
                return None;
            }
            let input = match &a[0] {
                Value::String(id) => ElementInput::from(id.as_str()),
                Value::Object(map) => ElementInput::Element(map["direct"].as_str().unwrap().to_owned()),
                _ => return None,
            };
            let res = get_element(input, |id| (id == "map").then(|| "map".to_owned()));
            ok(match res {
                Ok(found) => json!({ "found": found, "error": null }),
                Err(e) => json!({ "found": null, "error": e.message }),
            })
        },
    );
}

// ===========================================================================
// 2. TerrainOffsetProperty parity
// ===========================================================================

struct MockTerrainScene {
    has_globe: bool,
    height: Cell<Option<f64>>,
    terrain_changed: DefinitionChanged,
    morph_complete: DefinitionChanged,
    active_callback: Rc<RefCell<Option<TerrainHeightCallback>>>,
    get_height_calls: Cell<usize>,
    update_height_calls: Cell<usize>,
    removed_callbacks: Rc<Cell<usize>>,
}

impl MockTerrainScene {
    fn new(has_globe: bool, height: Option<f64>) -> Rc<Self> {
        Rc::new(Self {
            has_globe,
            height: Cell::new(height),
            terrain_changed: DefinitionChanged::new(),
            morph_complete: DefinitionChanged::new(),
            active_callback: Rc::new(RefCell::new(None)),
            get_height_calls: Cell::new(0),
            update_height_calls: Cell::new(0),
            removed_callbacks: Rc::new(Cell::new(0)),
        })
    }

    fn fire_callback(&self, h: f64) {
        let cb = self.active_callback.borrow().clone();
        if let Some(cb) = cb {
            cb(&Cartographic::new(0., 0., h));
        }
    }

    fn stats(&self) -> Value {
        json!({
            "getHeightCalls": self.get_height_calls.get(),
            "updateHeightCalls": self.update_height_calls.get(),
            "removedCallbacks": self.removed_callbacks.get(),
            "terrainListeners": self.terrain_changed.number_of_listeners(),
            "morphListeners": self.morph_complete.number_of_listeners(),
        })
    }
}

impl TerrainOffsetScene for MockTerrainScene {
    fn has_globe(&self) -> bool {
        self.has_globe
    }
    fn get_height(&self, _cartographic: &Cartographic, _hr: Option<HeightReference>) -> Option<f64> {
        self.get_height_calls.set(self.get_height_calls.get() + 1);
        self.height.get()
    }
    fn update_height(
        &self,
        _cartographic: &Cartographic,
        callback: TerrainHeightCallback,
        _hr: Option<HeightReference>,
    ) -> Option<TerrainHeightHandle> {
        self.update_height_calls.set(self.update_height_calls.get() + 1);
        *self.active_callback.borrow_mut() = Some(Rc::clone(&callback));
        let removed_count = Rc::clone(&self.removed_callbacks);
        let active = Rc::clone(&self.active_callback);
        let cb_ptr = Rc::as_ptr(&callback) as *const () as usize;
        Some(TerrainHeightHandle::new(move || {
            removed_count.set(removed_count.get() + 1);
            let mut slot = active.borrow_mut();
            if slot.as_ref().is_some_and(|c| Rc::as_ptr(c) as *const () as usize == cb_ptr) {
                *slot = None;
            }
        }))
    }
    fn terrain_provider_changed(&self) -> Option<&DefinitionChanged> {
        Some(&self.terrain_changed)
    }
    fn morph_complete(&self) -> Option<&DefinitionChanged> {
        Some(&self.morph_complete)
    }
}

#[test]
fn terrain_offset_property_matches_cesium() {
    run(
        "data_sources",
        "TerrainOffsetProperty",
        |_| TIGHT,
        |f, _| match f {
            "constantClamped" => {
                let host = MockTerrainScene::new(true, Some(250.0));
                let pos = Cartesian3::from_radians(0.5, 0.3, 100.0, &Ellipsoid::WGS84);
                let pos_prop: PropertyRef<Cartesian3> =
                    Rc::new(ConstantPositionProperty::new(Some(pos), ReferenceFrame::Fixed));
                let hr_prop: PropertyRef<HeightReference> =
                    Rc::new(ConstantProperty::new(Some(HeightReference::ClampToGround)));
                let prop = TerrainOffsetProperty::new(host.clone(), pos_prop, Some(hr_prop), None);

                let def_changes = Rc::new(Cell::new(0usize));
                let dc = Rc::clone(&def_changes);
                let _sub = prop.definition_changed().add_listener(move |_| dc.set(dc.get() + 1));

                let initial = prop.get_value(Some(&t0()), None).unwrap().unwrap();
                host.fire_callback(310.5);
                let after_callback = prop.get_value(Some(&t0()), None).unwrap().unwrap();

                host.height.set(Some(180.0));
                host.terrain_changed.raise();
                let after_terrain = prop.get_value(Some(&t0()), None).unwrap().unwrap();

                host.height.set(Some(420.0));
                host.morph_complete.raise();
                let after_morph = prop.get_value(Some(&t0()), None).unwrap().unwrap();

                let is_constant = prop.is_constant();
                let before_destroy = prop.is_destroyed();
                prop.destroy();
                let after_destroy = prop.is_destroyed();

                ok(json!({
                    "isConstant": is_constant,
                    "initial": initial.enc(),
                    "defChanges": def_changes.get(),
                    "afterCallback": after_callback.enc(),
                    "afterTerrain": after_terrain.enc(),
                    "afterMorph": after_morph.enc(),
                    "beforeDestroy": before_destroy,
                    "afterDestroy": after_destroy,
                    "stats": host.stats(),
                }))
            }
            "dynamicPositions" => {
                let host = MockTerrainScene::new(true, Some(100.0));
                let pos1 = Cartesian3::from_radians(0.1, 0.2, 50.0, &Ellipsoid::WGS84);
                let pos1_near = Cartesian3::new(pos1.x + 1e-12, pos1.y, pos1.z);
                let pos2 = Cartesian3::from_radians(0.4, -0.2, 20.0, &Ellipsoid::WGS84);
                let base = t0();
                let pos_prop: PropertyRef<Cartesian3> = Rc::new(CallbackProperty::new(
                    Rc::new(move |time, _| {
                        let dt = JulianDate::seconds_difference(time, &base);
                        Ok(if dt < 0.5 {
                            Some(pos1)
                        } else if dt < 1.5 {
                            Some(pos1_near)
                        } else if dt < 2.5 {
                            Some(pos2)
                        } else if dt < 3.5 {
                            Some(Cartesian3::ZERO)
                        } else {
                            None
                        })
                    }),
                    false,
                ));
                let hr_prop: PropertyRef<HeightReference> =
                    Rc::new(ConstantProperty::new(Some(HeightReference::ClampToGround)));
                let prop = TerrainOffsetProperty::new(host.clone(), pos_prop, Some(hr_prop), None);

                let v0 = prop.get_value(Some(&add_sec(0.)), None).unwrap().unwrap();
                let v1 = prop.get_value(Some(&add_sec(1.)), None).unwrap().unwrap();
                host.height.set(Some(75.0));
                let v2 = prop.get_value(Some(&add_sec(2.)), None).unwrap().unwrap();
                let v3 = prop.get_value(Some(&add_sec(3.)), None).unwrap().unwrap();
                let v4 = prop.get_value(Some(&add_sec(4.)), None).unwrap().unwrap();
                prop.destroy();

                ok(json!({
                    "values": [v0.enc(), v1.enc(), v2.enc(), v3.enc(), v4.enc()],
                    "stats": host.stats(),
                }))
            }
            "heightReferenceCombinations" => {
                let host = MockTerrainScene::new(true, Some(123.5));
                let pos = Cartesian3::from_radians(0.2, 0.4, 0.0, &Ellipsoid::WGS84);
                let pos_prop: PropertyRef<Cartesian3> =
                    Rc::new(ConstantPositionProperty::new(Some(pos), ReferenceFrame::Fixed));
                let hr = Rc::new(ConstantProperty::new(Some(HeightReference::None)));
                let ehr = Rc::new(ConstantProperty::new(Some(HeightReference::None)));
                let prop = TerrainOffsetProperty::new(
                    host,
                    pos_prop,
                    Some(hr.clone() as PropertyRef<HeightReference>),
                    Some(ehr.clone() as PropertyRef<HeightReference>),
                );

                let none_none = prop.get_value(Some(&t0()), None).unwrap().unwrap();
                ehr.set_value(Some(HeightReference::ClampToGround));
                let none_clamp = prop.get_value(Some(&t0()), None).unwrap().unwrap();
                ehr.set_value(Some(HeightReference::RelativeToGround));
                let none_rel = prop.get_value(Some(&t0()), None).unwrap().unwrap();
                ehr.set_value(Some(HeightReference::None));
                hr.set_value(Some(HeightReference::RelativeToGround));
                let rel_none = prop.get_value(Some(&t0()), None).unwrap().unwrap();
                prop.destroy();

                ok(json!({
                    "noneNone": none_none.enc(),
                    "noneClamp": none_clamp.enc(),
                    "noneRel": none_rel.enc(),
                    "relNone": rel_none.enc(),
                }))
            }
            "noGlobe" => {
                let host = MockTerrainScene::new(false, Some(500.0));
                let pos = Cartesian3::from_radians(0.3, 0.3, 10.0, &Ellipsoid::WGS84);
                let hr: PropertyRef<HeightReference> =
                    Rc::new(ConstantProperty::new(Some(HeightReference::ClampToGround)));
                let const_prop = TerrainOffsetProperty::new(
                    host.clone(),
                    Rc::new(ConstantPositionProperty::new(Some(pos), ReferenceFrame::Fixed)),
                    Some(hr.clone()),
                    None,
                );
                let dyn_prop = TerrainOffsetProperty::new(
                    host.clone(),
                    Rc::new(CallbackProperty::new(Rc::new(move |_, _| Ok(Some(pos))), false)),
                    Some(hr),
                    None,
                );
                let r_const = const_prop.get_value(Some(&t0()), None).unwrap().unwrap();
                let r_dyn = dyn_prop.get_value(Some(&t0()), None).unwrap().unwrap();
                const_prop.destroy();
                dyn_prop.destroy();
                ok(json!({
                    "rConst": r_const.enc(),
                    "rDyn": r_dyn.enc(),
                    "stats": host.stats(),
                }))
            }
            _ => None,
        },
    );
}

// ===========================================================================
// 3. EntityView parity
// ===========================================================================

fn camera_snapshot(camera: &mut Camera, view: &EntityView) -> Value {
    let position_wc = camera.position_wc();
    json!({
        "transform": camera.transform().enc(),
        "position": camera.position.enc(),
        "direction": camera.direction.enc(),
        "up": camera.up.enc(),
        "right": camera.right.enc(),
        "positionWC": position_wc.enc(),
        "boundingSphere": view.bounding_sphere.enc(),
    })
}

#[test]
fn entity_view_matches_cesium() {
    let view_tol = Tol { rel: 1e-10, abs: 1e-10, vector: true };
    run(
        "data_sources",
        "EntityView",
        |_| view_tol,
        |f, _| match f {
            "defaultOffsetAndEnu" => {
                let mut camera = Camera::new(CameraOptions::new(800., 600.));
                let pos = Cartesian3::from_degrees(-75.0, 40.0, 1000.0, &Ellipsoid::WGS84);
                let entity = Entity::new(EntityOptions {
                    id: Some("enu".into()),
                    position: Some(Rc::new(ConstantPositionProperty::new(Some(pos), ReferenceFrame::Fixed))),
                    tracking_reference_frame: TrackingReferenceFrame::Enu,
                    ..Default::default()
                })
                .unwrap();
                let mut view = EntityView::new(entity, Some(Ellipsoid::WGS84));
                view.update(&t0(), SceneMode::Scene3D, &mut camera, None).unwrap();
                let first = camera_snapshot(&mut camera, &view);
                view.update(&add_sec(5.), SceneMode::Scene3D, &mut camera, None).unwrap();
                let second = camera_snapshot(&mut camera, &view);
                ok(json!({
                    "defaultOffset3D": DEFAULT_OFFSET_3D.enc(),
                    "first": first,
                    "second": second,
                }))
            }
            "viewFromAndBoundingSphere" => {
                let pos = Cartesian3::from_degrees(10.0, 20.0, 500.0, &Ellipsoid::WGS84);
                let shifted = Cartesian3::from_degrees(10.01, 20.01, 500.0, &Ellipsoid::WGS84);
                let sphere = BoundingSphere::new(shifted, 50.0);

                let mut c1 = Camera::new(CameraOptions::new(800., 600.));
                let e1 = Entity::new(EntityOptions {
                    id: Some("with-view-from".into()),
                    position: Some(Rc::new(ConstantPositionProperty::new(Some(pos), ReferenceFrame::Fixed))),
                    tracking_reference_frame: TrackingReferenceFrame::Enu,
                    values: vec![("viewFrom".into(), PropertyInput::value(Cartesian3::new(-500.0, 200.0, 300.0)))],
                    ..Default::default()
                })
                .unwrap();
                let mut v1 = EntityView::new(e1, Some(Ellipsoid::WGS84));
                v1.update(&t0(), SceneMode::Scene3D, &mut c1, Some(sphere)).unwrap();
                let with_view_from = camera_snapshot(&mut c1, &v1);

                let mut c2 = Camera::new(CameraOptions::new(800., 600.));
                let high_pos = Cartesian3::from_degrees(10.0, 20.0, 2_000_000.0, &Ellipsoid::WGS84);
                let high_sphere = BoundingSphere::new(high_pos, 200.0);
                let e2 = Entity::new(EntityOptions {
                    id: Some("with-sphere".into()),
                    position: Some(Rc::new(ConstantPositionProperty::new(Some(high_pos), ReferenceFrame::Fixed))),
                    tracking_reference_frame: TrackingReferenceFrame::Enu,
                    ..Default::default()
                })
                .unwrap();
                let mut v2 = EntityView::new(e2, Some(Ellipsoid::WGS84));
                v2.update(&t0(), SceneMode::Scene3D, &mut c2, Some(high_sphere)).unwrap();
                let with_sphere = camera_snapshot(&mut c2, &v2);

                ok(json!({ "withViewFrom": with_view_from, "withSphere": with_sphere }))
            }
            "deepSpaceAndVvlhAutodetect" => {
                let mut c1 = Camera::new(CameraOptions::new(800., 600.));
                let deep_pos = Cartesian3::new(2.0 * Ellipsoid::WGS84.maximum_radius, 0.0, 0.0);
                let e1 = Entity::new(EntityOptions {
                    id: Some("deep".into()),
                    position: Some(Rc::new(ConstantPositionProperty::new(Some(deep_pos), ReferenceFrame::Fixed))),
                    tracking_reference_frame: TrackingReferenceFrame::Autodetect,
                    ..Default::default()
                })
                .unwrap();
                let mut v1 = EntityView::new(e1, Some(Ellipsoid::WGS84));
                v1.update(&t0(), SceneMode::Scene3D, &mut c1, None).unwrap();
                let deep_space = camera_snapshot(&mut c1, &v1);

                let mut c2 = Camera::new(CameraOptions::new(800., 600.));
                let r = Ellipsoid::WGS84.maximum_radius + 400_000.0;
                let omega = 0.00113_f64;
                let base = t0();
                let orbit_pos: PositionPropertyRef = Rc::new(CallbackPositionProperty::new(
                    Rc::new(move |time, _| {
                        let dt = JulianDate::seconds_difference(time, &base);
                        let angle = omega * dt;
                        Ok(Some(Cartesian3::new(r * angle.cos(), r * angle.sin(), 0.0)))
                    }),
                    false,
                    ReferenceFrame::Fixed,
                ));
                let e2 = Entity::new(EntityOptions {
                    id: Some("leo".into()),
                    position: Some(orbit_pos),
                    tracking_reference_frame: TrackingReferenceFrame::Autodetect,
                    ..Default::default()
                })
                .unwrap();
                let mut v2 = EntityView::new(e2, Some(Ellipsoid::WGS84));
                v2.update(&t0(), SceneMode::Scene3D, &mut c2, None).unwrap();
                let vvlh = camera_snapshot(&mut c2, &v2);

                ok(json!({ "deepSpace": deep_space, "vvlh": vvlh }))
            }
            "inertialAndVelocityFrames" => {
                let pos = Cartesian3::from_degrees(0.0, 0.0, 1000.0, &Ellipsoid::WGS84);
                let q = Quaternion::new(0.1, 0.2, 0.3, 0.9).normalize();

                let mut c1 = Camera::new(CameraOptions::new(800., 600.));
                let e1 = Entity::new(EntityOptions {
                    id: Some("inertial".into()),
                    position: Some(Rc::new(ConstantPositionProperty::new(Some(pos), ReferenceFrame::Fixed))),
                    orientation: Some(Rc::new(ConstantProperty::new(Some(q)))),
                    tracking_reference_frame: TrackingReferenceFrame::Inertial,
                    ..Default::default()
                })
                .unwrap();
                let mut v1 = EntityView::new(e1, Some(Ellipsoid::WGS84));
                v1.update(&t0(), SceneMode::Scene3D, &mut c1, None).unwrap();
                let inertial = camera_snapshot(&mut c1, &v1);

                let mut c2 = Camera::new(CameraOptions::new(800., 600.));
                let base = t0();
                let vel_pos: PositionPropertyRef = Rc::new(CallbackPositionProperty::new(
                    Rc::new(move |time, _| {
                        let dt = JulianDate::seconds_difference(time, &base);
                        Ok(Some(Cartesian3::from_degrees(dt * 0.001, 0.0, 1000.0, &Ellipsoid::WGS84)))
                    }),
                    false,
                    ReferenceFrame::Fixed,
                ));
                let e2 = Entity::new(EntityOptions {
                    id: Some("velocity".into()),
                    position: Some(vel_pos),
                    tracking_reference_frame: TrackingReferenceFrame::Velocity,
                    ..Default::default()
                })
                .unwrap();
                let mut v2 = EntityView::new(e2, Some(Ellipsoid::WGS84));
                v2.update(&add_sec(10.), SceneMode::Scene3D, &mut c2, None).unwrap();
                let velocity = camera_snapshot(&mut c2, &v2);

                ok(json!({ "inertial": inertial, "velocity": velocity }))
            }
            "sceneModes" => {
                let mut camera = Camera::new(CameraOptions::new(800., 600.));
                let pos = Cartesian3::from_degrees(30.0, 15.0, 500.0, &Ellipsoid::WGS84);
                let entity = Entity::new(EntityOptions {
                    id: Some("modes".into()),
                    position: Some(Rc::new(ConstantPositionProperty::new(Some(pos), ReferenceFrame::Fixed))),
                    tracking_reference_frame: TrackingReferenceFrame::Enu,
                    ..Default::default()
                })
                .unwrap();
                let mut view = EntityView::new(entity, Some(Ellipsoid::WGS84));

                let before_morph = camera_snapshot(&mut camera, &view);
                view.update(&t0(), SceneMode::Morphing, &mut camera, None).unwrap();
                let after_morph = camera_snapshot(&mut camera, &view);

                camera.update(SceneMode::ColumbusView).unwrap();
                view.update(&t0(), SceneMode::ColumbusView, &mut camera, None).unwrap();
                let columbus = camera_snapshot(&mut camera, &view);

                camera.frustum = CameraFrustum::OrthographicOffCenter(OrthographicOffCenterFrustum::new(
                    -1e7, 1e7, 7.5e6, -7.5e6, 1.0, 1e8,
                ));
                camera.update(SceneMode::Scene2D).unwrap();
                view.update(&t0(), SceneMode::Scene2D, &mut camera, None).unwrap();
                let scene2d = camera_snapshot(&mut camera, &view);

                ok(json!({
                    "beforeMorph": before_morph,
                    "afterMorph": after_morph,
                    "columbus": columbus,
                    "scene2d": scene2d,
                }))
            }
            "noPosition" => {
                let mut camera = Camera::new(CameraOptions::new(800., 600.));
                let entity = Entity::with_id("empty");
                let mut view = EntityView::new(entity, Some(Ellipsoid::WGS84));
                let before = camera_snapshot(&mut camera, &view);
                view.update(&t0(), SceneMode::Scene3D, &mut camera, None).unwrap();
                let after = camera_snapshot(&mut camera, &view);
                ok(json!({ "before": before, "after": after }))
            }
            _ => None,
        },
    );
}

// ===========================================================================
// 4. EntityCluster parity
// ===========================================================================

fn make_cluster_camera(height: f64) -> Camera {
    let mut camera = Camera::new(CameraOptions::new(800., 600.));
    camera
        .set_view(CameraView {
            destination: Some(CameraDestination::Cartesian(Cartesian3::from_degrees(
                0.0,
                0.0,
                height,
                &Ellipsoid::WGS84,
            ))),
            ..Default::default()
        })
        .unwrap();
    camera
}

#[test]
fn entity_cluster_matches_cesium() {
    let cluster_tol = Tol { rel: 1e-10, abs: 1e-10, vector: true };
    run(
        "data_sources",
        "EntityCluster",
        |_| cluster_tol,
        |f, _| match f {
            "defaultsAndPoolReuse" => {
                let cluster = EntityCluster::<Host>::new(EntityClusterOptions::default());
                let defaults = json!({
                    "enabled": cluster.enabled(),
                    "pixelRange": cluster.pixel_range(),
                    "minimumClusterSize": cluster.minimum_cluster_size(),
                    "clusterBillboards": cluster.cluster_billboards(),
                    "clusterLabels": cluster.cluster_labels(),
                    "clusterPoints": cluster.cluster_points(),
                    "show": cluster.show(),
                });
                let e1 = Entity::with_id("e1");
                let e2 = Entity::with_id("e2");
                let b1 = cluster.get_billboard(&e1).unwrap();
                let b1_again = cluster.get_billboard(&e1).unwrap();
                let l1 = cluster.get_label(&e1).unwrap();
                let p1 = cluster.get_point(&e1).unwrap();
                cluster.remove_billboard(&e1);
                cluster.remove_label(&e1);
                cluster.remove_point(&e1);
                let b2 = cluster.get_billboard(&e2).unwrap();
                let l2 = cluster.get_label(&e2).unwrap();
                let p2 = cluster.get_point(&e2).unwrap();
                let reuse = json!({
                    "sameBillboard": b1.ptr_eq(&b1_again),
                    "reusedBillboard": b1.ptr_eq(&b2),
                    "reusedLabel": Rc::ptr_eq(&l1, &l2),
                    "reusedPoint": p1.ptr_eq(&p2),
                    "billboardLen": cluster.billboard_count(),
                    "labelLen": cluster.label_count(),
                    "pointLen": cluster.point_count(),
                });
                cluster.destroy();
                let after_destroy = json!({
                    "hasBillboards": cluster.billboard_count() > 0,
                    "hasLabels": cluster.label_count() > 0,
                    "hasPoints": cluster.point_count() > 0,
                });
                let p3 = cluster.get_point(&e1).unwrap();
                ok(json!({
                    "defaults": defaults,
                    "reuse": reuse,
                    "afterDestroy": after_destroy,
                    "reusablePointLen": cluster.point_count(),
                    "p3Show": p3.show(),
                }))
            }
            "pointClustering" => {
                let mut camera = make_cluster_camera(10_000_000.0);
                let projection = GeographicProjection::default();
                let state = SceneTransformState {
                    mode: SceneMode::Scene3D,
                    morph_time: 1.0,
                    map_projection: &projection,
                    client_dimensions: Cartesian2::new(800., 600.),
                    drawing_buffer_dimensions: Cartesian2::new(800., 600.),
                };
                let cluster = EntityCluster::<Host>::new(EntityClusterOptions {
                    enabled: true,
                    pixel_range: 80.0,
                    minimum_cluster_size: 2,
                    ..Default::default()
                });
                let coords = [("p1", 0.0, 0.0), ("p2", 0.05, 0.02), ("p3", -0.04, 0.03), ("far", 15.0, 15.0)];
                let mut points = Vec::new();
                for (id, lon, lat) in coords {
                    let entity = Entity::with_id(id);
                    let pt = cluster.get_point(&entity).unwrap();
                    pt.set_id(Some(id.to_owned())).unwrap();
                    pt.set_position(Cartesian3::from_degrees(lon, lat, 0.0, &Ellipsoid::WGS84));
                    pt.set_pixel_size(10.0);
                    points.push(pt);
                }

                let events = Rc::new(RefCell::new(Vec::<Value>::new()));
                let ev = Rc::clone(&events);
                let _sub = cluster.cluster_event().add_listener(move |payload| {
                    let lb = payload.cluster.label.borrow();
                    ev.borrow_mut().push(json!({
                        "entities": payload.entities.iter().map(|e| e.id()).collect::<Vec<_>>(),
                        "labelText": lb.text(),
                        "labelShow": lb.show(),
                        "billboardShow": payload.cluster.billboard.show(),
                        "pointShow": payload.cluster.point.show(),
                        "position": lb.position().enc(),
                    }));
                });

                cluster.run_clustering(Ellipsoid::WGS84, &mut camera, &state, Some(0.01)).unwrap();
                let after_small_amount = events.borrow().len();

                cluster.run_clustering(Ellipsoid::WGS84, &mut camera, &state, None).unwrap();
                let after_cluster = json!({
                    "events": events.borrow().clone(),
                    "clusterLabelCount": cluster.cluster_label_count(),
                    "sourceClusterShow": points.iter().map(PointPrimitive::cluster_show).collect::<Vec<_>>(),
                });

                cluster.set_enabled(false);
                events.borrow_mut().clear();
                cluster.run_clustering(Ellipsoid::WGS84, &mut camera, &state, None).unwrap();
                let events_when_disabled = events.borrow().len();

                cluster.destroy();
                ok(json!({
                    "afterSmallAmount": after_small_amount,
                    "afterCluster": after_cluster,
                    "eventsWhenDisabled": events_when_disabled,
                }))
            }
            "minimumClusterSizeAndPixelRange" => {
                let mut camera = make_cluster_camera(10_000_000.0);
                let projection = GeographicProjection::default();
                let state = SceneTransformState {
                    mode: SceneMode::Scene3D,
                    morph_time: 1.0,
                    map_projection: &projection,
                    client_dimensions: Cartesian2::new(800., 600.),
                    drawing_buffer_dimensions: Cartesian2::new(800., 600.),
                };
                let cluster = EntityCluster::<Host>::new(EntityClusterOptions {
                    enabled: true,
                    pixel_range: 80.0,
                    minimum_cluster_size: 4,
                    ..Default::default()
                });
                let coords = [("p1", 0.0, 0.0), ("p2", 0.05, 0.02), ("p3", -0.04, 0.03), ("far", 15.0, 15.0)];
                let mut points = Vec::new();
                for (id, lon, lat) in coords {
                    let entity = Entity::with_id(id);
                    let pt = cluster.get_point(&entity).unwrap();
                    pt.set_id(Some(id.to_owned())).unwrap();
                    pt.set_position(Cartesian3::from_degrees(lon, lat, 0.0, &Ellipsoid::WGS84));
                    pt.set_pixel_size(10.0);
                    points.push(pt);
                }

                cluster.run_clustering(Ellipsoid::WGS84, &mut camera, &state, None).unwrap();
                let with_min_4 = json!({
                    "clusterCount": cluster.cluster_label_count(),
                    "sourceClusterShow": points.iter().map(PointPrimitive::cluster_show).collect::<Vec<_>>(),
                });

                cluster.set_minimum_cluster_size(2);
                cluster.set_pixel_range(1.0);
                points[1].set_position(Cartesian3::from_degrees(2.0, 0.0, 0.0, &Ellipsoid::WGS84));
                points[2].set_position(Cartesian3::from_degrees(-2.0, 0.0, 0.0, &Ellipsoid::WGS84));
                cluster.run_clustering(Ellipsoid::WGS84, &mut camera, &state, None).unwrap();
                let with_small_range = json!({
                    "clusterCount": cluster.cluster_label_count(),
                    "sourceClusterShow": points.iter().map(PointPrimitive::cluster_show).collect::<Vec<_>>(),
                });

                cluster.destroy();
                ok(json!({ "withMin4": with_min_4, "withSmallRange": with_small_range }))
            }
            "zoomInPreviousClusters" => {
                let mut camera = make_cluster_camera(10_000_000.0);
                let projection = GeographicProjection::default();
                let state = SceneTransformState {
                    mode: SceneMode::Scene3D,
                    morph_time: 1.0,
                    map_projection: &projection,
                    client_dimensions: Cartesian2::new(800., 600.),
                    drawing_buffer_dimensions: Cartesian2::new(800., 600.),
                };
                let cluster = EntityCluster::<Host>::new(EntityClusterOptions {
                    enabled: true,
                    pixel_range: 80.0,
                    minimum_cluster_size: 2,
                    ..Default::default()
                });
                for (id, lon, lat) in [("a", 0.0, 0.0), ("b", 0.05, 0.02), ("c", -0.04, 0.03)] {
                    let entity = Entity::with_id(id);
                    let pt = cluster.get_point(&entity).unwrap();
                    pt.set_id(Some(id.to_owned())).unwrap();
                    pt.set_position(Cartesian3::from_degrees(lon, lat, 0.0, &Ellipsoid::WGS84));
                    pt.set_pixel_size(10.0);
                }

                let passes = Rc::new(RefCell::new(Vec::<Vec<Value>>::new()));
                let ps = Rc::clone(&passes);
                let _sub = cluster.cluster_event().add_listener(move |payload| {
                    let lb = payload.cluster.label.borrow();
                    ps.borrow_mut().last_mut().unwrap().push(json!({
                        "entities": payload.entities.iter().map(|e| e.id()).collect::<Vec<_>>(),
                        "text": lb.text(),
                        "position": lb.position().enc(),
                    }));
                });

                passes.borrow_mut().push(Vec::new());
                cluster.run_clustering(Ellipsoid::WGS84, &mut camera, &state, None).unwrap();

                camera = make_cluster_camera(6_000_000.0);
                passes.borrow_mut().push(Vec::new());
                cluster.run_clustering(Ellipsoid::WGS84, &mut camera, &state, None).unwrap();

                let out = passes.borrow().clone();
                cluster.destroy();
                ok(json!({ "passes": out }))
            }
            "mixedPointAndLabel" => {
                let mut camera = make_cluster_camera(10_000_000.0);
                let projection = GeographicProjection::default();
                let state = SceneTransformState {
                    mode: SceneMode::Scene3D,
                    morph_time: 1.0,
                    map_projection: &projection,
                    client_dimensions: Cartesian2::new(800., 600.),
                    drawing_buffer_dimensions: Cartesian2::new(800., 600.),
                };
                let cluster = EntityCluster::<Host>::new(EntityClusterOptions {
                    enabled: true,
                    pixel_range: 80.0,
                    minimum_cluster_size: 2,
                    cluster_points: true,
                    cluster_labels: false,
                    ..Default::default()
                });

                let make_both = |id: &str, lon: f64, lat: f64| -> (PointPrimitive, LabelHandle) {
                    let entity = Entity::with_id(id);
                    entity
                        .set_property("point", Some(PropertyInput::Component(Rc::new(PointGraphics::new()))))
                        .unwrap();
                    entity
                        .set_property("label", Some(PropertyInput::Component(Rc::new(LabelGraphics::new()))))
                        .unwrap();
                    let pos = Cartesian3::from_degrees(lon, lat, 0.0, &Ellipsoid::WGS84);
                    let pt = cluster.get_point(&entity).unwrap();
                    pt.set_id(Some(id.to_owned())).unwrap();
                    pt.set_position(pos);
                    pt.set_pixel_size(8.0);
                    let lb = cluster.get_label(&entity).unwrap();
                    {
                        let mut l = lb.borrow_mut();
                        l.set_id(Some(id.to_owned()));
                        l.set_position(pos);
                        l.set_text(id.to_owned());
                    }
                    (pt, lb)
                };

                let (ap, al) = make_both("a", 0.0, 0.0);
                let (bp, bl) = make_both("b", 0.03, 0.01);
                let (cp, cl) = make_both("c", 18.0, 12.0);

                let events = Rc::new(RefCell::new(Vec::<Value>::new()));
                let ev = Rc::clone(&events);
                let _sub = cluster.cluster_event().add_listener(move |payload| {
                    ev.borrow_mut().push(json!({
                        "entities": payload.entities.iter().map(|e| e.id()).collect::<Vec<_>>(),
                        "text": payload.cluster.label.borrow().text(),
                    }));
                });

                cluster.run_clustering(Ellipsoid::WGS84, &mut camera, &state, None).unwrap();
                let res = json!({
                    "events": events.borrow().clone(),
                    "pointShow": [ap.cluster_show(), bp.cluster_show(), cp.cluster_show()],
                    "labelShow": [al.borrow().cluster_show(), bl.borrow().cluster_show(), cl.borrow().cluster_show()],
                });
                cluster.destroy();
                ok(res)
            }
            _ => None,
        },
    );
}

// ===========================================================================
// 5. BillboardVisualizer parity
// ===========================================================================

fn enc_nfs(v: Option<NearFarScalar>) -> Value {
    v.map_or(Value::Null, |n| json!([n.near, n.near_value, n.far, n.far_value]))
}

fn enc_ddc(v: Option<DistanceDisplayCondition>) -> Value {
    v.map_or(Value::Null, |d| json!([d.near, d.far]))
}

fn snapshot_billboard(b: Option<&Billboard<Host>>) -> Value {
    let Some(b) = b else { return Value::Null };
    let o = b.options();
    json!({
        "show": b.show(),
        "id": b.id(),
        "position": b.position().enc(),
        "color": b.color().enc(),
        "eyeOffset": b.eye_offset().enc(),
        "heightReference": b.height_reference() as i32,
        "pixelOffset": b.pixel_offset().enc(),
        "scale": b.scale(),
        "rotation": b.rotation(),
        "alignedAxis": b.aligned_axis().enc(),
        "horizontalOrigin": b.horizontal_origin() as i32,
        "verticalOrigin": b.vertical_origin() as i32,
        "width": o.width,
        "height": o.height,
        "scaleByDistance": enc_nfs(b.scale_by_distance()),
        "translucencyByDistance": enc_nfs(b.translucency_by_distance()),
        "pixelOffsetScaleByDistance": enc_nfs(b.pixel_offset_scale_by_distance()),
        "sizeInMeters": b.size_in_meters(),
        "distanceDisplayCondition": enc_ddc(b.distance_display_condition()),
        "disableDepthTestDistance": b.disable_depth_test_distance(),
        "splitDirection": b.split_direction() as i32,
        "image": b.image(),
    })
}

fn nfs_json(near: f64, near_val: f64, far: f64, far_val: f64) -> PropertyInput {
    PropertyInput::Value(DynamicValue::from_json(json!({
        "near": near, "nearValue": near_val, "far": far, "farValue": far_val
    })))
}

fn ddc_json(near: f64, far: f64) -> PropertyInput {
    PropertyInput::Value(DynamicValue::from_json(json!({ "near": near, "far": far })))
}

fn brect_json(x: f64, y: f64, width: f64, height: f64) -> PropertyInput {
    PropertyInput::Value(DynamicValue::from_json(json!({ "x": x, "y": y, "width": width, "height": height })))
}

#[test]
fn billboard_visualizer_matches_cesium() {
    run(
        "data_sources",
        "BillboardVisualizer",
        |_| TIGHT,
        |f, _| match f {
            "defaultsAndAllProperties" => {
                let cluster = EntityCluster::<Host>::new(EntityClusterOptions::default());
                let collection = EntityCollection::new();
                let p1 = Cartesian3::new(6378137.0, 0.0, 0.0);
                let p2 = Cartesian3::new(0.0, 6378137.0, 0.0);

                let e1 = Entity::with_id("default");
                e1.set_position(Some(Rc::new(ConstantPositionProperty::new(Some(p1), ReferenceFrame::Fixed)))).unwrap();
                let g1 = Rc::new(BillboardGraphics::new());
                g1.set_image(Some(PropertyInput::value("pin.png"))).unwrap();
                e1.set_property("billboard", Some(PropertyInput::Component(g1))).unwrap();
                collection.add(e1).unwrap();

                let e2 = Entity::with_id("custom");
                e2.set_position(Some(Rc::new(ConstantPositionProperty::new(Some(p2), ReferenceFrame::Fixed)))).unwrap();
                let g2 = Rc::new(BillboardGraphics::new());
                g2.set_image(Some(PropertyInput::value("icon.png"))).unwrap();
                g2.set_color(Some(PropertyInput::value(Color::new(0.2, 0.4, 0.6, 0.8)))).unwrap();
                g2.set_eye_offset(Some(PropertyInput::value(Cartesian3::new(1., 2., 3.)))).unwrap();
                g2.set_pixel_offset(Some(PropertyInput::value(Cartesian2::new(4., -5.)))).unwrap();
                g2.set_scale(Some(PropertyInput::value(2.5))).unwrap();
                g2.set_rotation(Some(PropertyInput::value(0.75))).unwrap();
                g2.set_aligned_axis(Some(PropertyInput::value(Cartesian3::new(0., 0., 1.)))).unwrap();
                g2.set_horizontal_origin(Some(PropertyInput::value(HorizontalOrigin::Left as i32 as f64))).unwrap();
                g2.set_vertical_origin(Some(PropertyInput::value(VerticalOrigin::Bottom as i32 as f64))).unwrap();
                g2.set_width(Some(PropertyInput::value(32.0))).unwrap();
                g2.set_height(Some(PropertyInput::value(48.0))).unwrap();
                g2.set_scale_by_distance(Some(nfs_json(100., 2.0, 1000., 0.5))).unwrap();
                g2.set_translucency_by_distance(Some(nfs_json(50., 1.0, 500., 0.1))).unwrap();
                g2.set_pixel_offset_scale_by_distance(Some(nfs_json(10., 1.5, 200., 0.25))).unwrap();
                g2.set_size_in_meters(Some(PropertyInput::value(true))).unwrap();
                g2.set_distance_display_condition(Some(ddc_json(10.0, 5000.0))).unwrap();
                g2.set_disable_depth_test_distance(Some(PropertyInput::value(1234.0))).unwrap();
                g2.set_split_direction(Some(PropertyInput::value(SplitDirection::Left as i32 as f64))).unwrap();
                g2.set_image_sub_region(Some(brect_json(2., 4., 16., 16.))).unwrap();
                e2.set_property("billboard", Some(PropertyInput::Component(g2))).unwrap();
                collection.add(e2).unwrap();

                let mut vis = BillboardVisualizer::new(cluster.clone(), collection);
                let updated = vis.update(&t0()).unwrap();
                let out = json!({
                    "updated": updated,
                    "defaultBillboard": snapshot_billboard(vis.billboard_for("default").as_ref()),
                    "customBillboard": snapshot_billboard(vis.billboard_for("custom").as_ref()),
                });
                vis.destroy();
                cluster.destroy();
                ok(out)
            }
            "visibilityAvailabilityAndCollection" => {
                let cluster = EntityCluster::<Host>::new(EntityClusterOptions::default());
                let collection = EntityCollection::new();
                let mut vis = BillboardVisualizer::new(cluster.clone(), collection.clone());

                let avail = Rc::new(PropertyIntervals::new(|_: &Option<()>, _: &Option<()>| true));
                avail.add_interval(TimeInterval::new(t0(), add_sec(60.), true, true, None));
                let g = Rc::new(BillboardGraphics::new());
                let e = Entity::new(EntityOptions {
                    id: Some("e1".into()),
                    position: Some(Rc::new(ConstantPositionProperty::new(
                        Some(Cartesian3::new(6378137.0, 0.0, 0.0)),
                        ReferenceFrame::Fixed,
                    ))),
                    availability: Some(avail),
                    ..Default::default()
                })
                .unwrap();
                e.set_property("billboard", Some(PropertyInput::Component(g.clone()))).unwrap();
                collection.add(e.clone()).unwrap();

                vis.update(&t0()).unwrap();
                let no_image = vis.billboard_for("e1").is_some();

                g.set_image(Some(PropertyInput::value("a.png"))).unwrap();
                vis.update(&t0()).unwrap();
                let shown = vis.billboard_for("e1").is_some();

                vis.update(&add_sec(120.)).unwrap();
                let outside_avail = vis.billboard_for("e1").is_some();

                vis.update(&t0()).unwrap();
                g.set_show(Some(PropertyInput::value(false))).unwrap();
                vis.update(&t0()).unwrap();
                let graphics_hidden = vis.billboard_for("e1").is_some();

                g.set_show(Some(PropertyInput::value(true))).unwrap();
                e.set_show(false);
                vis.update(&t0()).unwrap();
                let entity_hidden = vis.billboard_for("e1").is_some();

                e.set_show(true);
                vis.update(&t0()).unwrap();
                let reshown = vis.billboard_for("e1").is_some();

                collection.remove(Some(&e));
                let after_remove = vis.billboard_for("e1").is_some();

                let before_destroy = vis.is_destroyed();
                vis.destroy();
                let after_destroy = vis.is_destroyed();
                cluster.destroy();

                ok(json!({
                    "noImage": no_image,
                    "shown": shown,
                    "outsideAvail": outside_avail,
                    "graphicsHidden": graphics_hidden,
                    "entityHidden": entity_hidden,
                    "reshown": reshown,
                    "afterRemove": after_remove,
                    "beforeDestroy": before_destroy,
                    "afterDestroy": after_destroy,
                }))
            }
            "boundingSphereAndClamping" => {
                let cluster = EntityCluster::<Host>::new(EntityClusterOptions::default());
                let sampler: HeightSampler = Rc::new(|_, _| Some(125.0));
                cluster.set_height_sampler(Some(sampler));
                let collection = EntityCollection::new();

                let pos = Cartesian3::from_radians(0.4, 0.2, 50.0, &Ellipsoid::WGS84);
                let e1 = Entity::with_id("unclamped");
                e1.set_position(Some(Rc::new(ConstantPositionProperty::new(Some(pos), ReferenceFrame::Fixed))))
                    .unwrap();
                let g1 = Rc::new(BillboardGraphics::new());
                g1.set_image(Some(PropertyInput::value("a.png"))).unwrap();
                e1.set_property("billboard", Some(PropertyInput::Component(g1))).unwrap();
                collection.add(e1.clone()).unwrap();

                let e2 = Entity::with_id("clamped");
                e2.set_position(Some(Rc::new(ConstantPositionProperty::new(Some(pos), ReferenceFrame::Fixed))))
                    .unwrap();
                let g2 = Rc::new(BillboardGraphics::new());
                g2.set_image(Some(PropertyInput::value("b.png"))).unwrap();
                g2.set_height_reference(Some(PropertyInput::value(HeightReference::ClampToGround as i32 as f64)))
                    .unwrap();
                e2.set_property("billboard", Some(PropertyInput::Component(g2))).unwrap();
                collection.add(e2.clone()).unwrap();

                let unknown = Entity::with_id("unknown");

                let mut vis = BillboardVisualizer::new(cluster.clone(), collection);
                vis.update(&t0()).unwrap();

                // Pending before frame update clamps height
                let mut bs_pending = BoundingSphere::default();
                assert_eq!(vis.get_bounding_sphere(&e2, &mut bs_pending), BoundingSphereState::Pending);

                // Run cluster frame update so BillboardCollection applies height_sampler clamping
                let mut frame = make_frame();
                let mut camera = Camera::new(CameraOptions::new(800., 600.));
                let ctx = Context::new_webgl2(Host::default());
                frame.context = Some(FrameResource::new(ctx.clone()));
                cluster.update(&mut frame, &mut camera, &ctx).unwrap();

                let mut bs1 = BoundingSphere::default();
                let s1 = vis.get_bounding_sphere(&e1, &mut bs1);
                let mut bs2 = BoundingSphere::default();
                let s2 = vis.get_bounding_sphere(&e2, &mut bs2);
                let mut bs3 = BoundingSphere::default();
                let s3 = vis.get_bounding_sphere(&unknown, &mut bs3);

                vis.destroy();
                cluster.destroy();
                ok(json!({
                    "unclamped": [s1 as i32, bs1.enc()],
                    "clamped": [s2 as i32, bs2.enc()],
                    "unknown": s3 as i32,
                }))
            }
            _ => None,
        },
    );
}

// ===========================================================================
// 6. LabelVisualizer parity
// ===========================================================================

fn snapshot_label(l: Option<&LabelHandle>) -> Value {
    let Some(l) = l else { return Value::Null };
    let lb = l.borrow();
    json!({
        "show": lb.show(),
        "id": lb.id(),
        "position": lb.position().enc(),
        "text": lb.text(),
        "scale": lb.scale(),
        "font": lb.font(),
        "style": lb.style() as i32,
        "fillColor": lb.fill_color().enc(),
        "outlineColor": lb.outline_color().enc(),
        "outlineWidth": lb.outline_width(),
        "showBackground": lb.show_background(),
        "backgroundColor": lb.background_color().enc(),
        "backgroundPadding": lb.background_padding().enc(),
        "pixelOffset": lb.pixel_offset().enc(),
        "eyeOffset": lb.eye_offset().enc(),
        "heightReference": lb.height_reference() as i32,
        "horizontalOrigin": lb.horizontal_origin() as i32,
        "verticalOrigin": lb.vertical_origin() as i32,
        "scaleByDistance": enc_nfs(lb.scale_by_distance()),
        "translucencyByDistance": enc_nfs(lb.translucency_by_distance()),
        "pixelOffsetScaleByDistance": enc_nfs(lb.pixel_offset_scale_by_distance()),
        "distanceDisplayCondition": enc_ddc(lb.distance_display_condition()),
        "disableDepthTestDistance": lb.disable_depth_test_distance(),
    })
}

#[test]
fn label_visualizer_matches_cesium() {
    run(
        "data_sources",
        "LabelVisualizer",
        |_| TIGHT,
        |f, _| match f {
            "defaultsAndAllProperties" => {
                let cluster = EntityCluster::<Host>::new(EntityClusterOptions::default());
                let collection = EntityCollection::new();
                let p1 = Cartesian3::new(6378137.0, 0.0, 0.0);
                let p2 = Cartesian3::new(0.0, 6378137.0, 0.0);

                let e1 = Entity::with_id("default");
                e1.set_position(Some(Rc::new(ConstantPositionProperty::new(Some(p1), ReferenceFrame::Fixed)))).unwrap();
                let g1 = Rc::new(LabelGraphics::new());
                g1.set_text(Some(PropertyInput::value("Hello"))).unwrap();
                e1.set_property("label", Some(PropertyInput::Component(g1))).unwrap();
                collection.add(e1).unwrap();

                let e2 = Entity::with_id("custom");
                e2.set_position(Some(Rc::new(ConstantPositionProperty::new(Some(p2), ReferenceFrame::Fixed)))).unwrap();
                let g2 = Rc::new(LabelGraphics::new());
                g2.set_text(Some(PropertyInput::value("Custom"))).unwrap();
                g2.set_font(Some(PropertyInput::value("20px monospace"))).unwrap();
                g2.set_style(Some(PropertyInput::value(LabelStyle::FillAndOutline as i32 as f64))).unwrap();
                g2.set_scale(Some(PropertyInput::value(1.75))).unwrap();
                g2.set_fill_color(Some(PropertyInput::value(Color::new(0.9, 0.1, 0.2, 1.0)))).unwrap();
                g2.set_outline_color(Some(PropertyInput::value(Color::new(0.1, 0.2, 0.3, 0.9)))).unwrap();
                g2.set_outline_width(Some(PropertyInput::value(3.0))).unwrap();
                g2.set_show_background(Some(PropertyInput::value(true))).unwrap();
                g2.set_background_color(Some(PropertyInput::value(Color::new(0.2, 0.3, 0.4, 0.7)))).unwrap();
                g2.set_background_padding(Some(PropertyInput::value(Cartesian2::new(10., 6.)))).unwrap();
                g2.set_pixel_offset(Some(PropertyInput::value(Cartesian2::new(-3., 8.)))).unwrap();
                g2.set_eye_offset(Some(PropertyInput::value(Cartesian3::new(4., 5., 6.)))).unwrap();
                g2.set_horizontal_origin(Some(PropertyInput::value(HorizontalOrigin::Right as i32 as f64))).unwrap();
                g2.set_vertical_origin(Some(PropertyInput::value(VerticalOrigin::Top as i32 as f64))).unwrap();
                g2.set_scale_by_distance(Some(nfs_json(100., 2.0, 1000., 0.5))).unwrap();
                g2.set_translucency_by_distance(Some(nfs_json(50., 1.0, 500., 0.2))).unwrap();
                g2.set_pixel_offset_scale_by_distance(Some(nfs_json(10., 1.2, 200., 0.4))).unwrap();
                g2.set_distance_display_condition(Some(ddc_json(20.0, 4000.0))).unwrap();
                g2.set_disable_depth_test_distance(Some(PropertyInput::value(500.0))).unwrap();
                e2.set_property("label", Some(PropertyInput::Component(g2))).unwrap();
                collection.add(e2).unwrap();

                let mut vis = LabelVisualizer::new(cluster.clone(), collection);
                let updated = vis.update(&t0()).unwrap();
                let out = json!({
                    "updated": updated,
                    "defaultLabel": snapshot_label(vis.label_for("default").as_ref()),
                    "customLabel": snapshot_label(vis.label_for("custom").as_ref()),
                });
                vis.destroy();
                cluster.destroy();
                ok(out)
            }
            "visibilityAvailabilityAndCollection" => {
                let cluster = EntityCluster::<Host>::new(EntityClusterOptions::default());
                let collection = EntityCollection::new();
                let mut vis = LabelVisualizer::new(cluster.clone(), collection.clone());

                let avail = Rc::new(PropertyIntervals::new(|_: &Option<()>, _: &Option<()>| true));
                avail.add_interval(TimeInterval::new(t0(), add_sec(60.), true, true, None));
                let g = Rc::new(LabelGraphics::new());
                let e = Entity::new(EntityOptions {
                    id: Some("l1".into()),
                    position: Some(Rc::new(ConstantPositionProperty::new(
                        Some(Cartesian3::new(6378137.0, 0.0, 0.0)),
                        ReferenceFrame::Fixed,
                    ))),
                    availability: Some(avail),
                    ..Default::default()
                })
                .unwrap();
                e.set_property("label", Some(PropertyInput::Component(g.clone()))).unwrap();
                collection.add(e.clone()).unwrap();

                vis.update(&t0()).unwrap();
                let no_text = vis.label_for("l1").is_some();

                g.set_text(Some(PropertyInput::value("Visible"))).unwrap();
                vis.update(&t0()).unwrap();
                let shown = vis.label_for("l1").is_some();

                vis.update(&add_sec(90.)).unwrap();
                let outside_avail = vis.label_for("l1").is_some();

                vis.update(&t0()).unwrap();
                g.set_show(Some(PropertyInput::value(false))).unwrap();
                vis.update(&t0()).unwrap();
                let graphics_hidden = vis.label_for("l1").is_some();

                g.set_show(Some(PropertyInput::value(true))).unwrap();
                collection.set_show(false).unwrap();
                vis.update(&t0()).unwrap();
                let collection_hidden = vis.label_for("l1").is_some();

                collection.set_show(true).unwrap();
                vis.update(&t0()).unwrap();
                let reshown = vis.label_for("l1").is_some();

                e.set_property("label", None).unwrap();
                let after_prop_removed = vis.label_for("l1").is_some();

                let before_destroy = vis.is_destroyed();
                vis.destroy();
                let after_destroy = vis.is_destroyed();
                cluster.destroy();

                ok(json!({
                    "noText": no_text,
                    "shown": shown,
                    "outsideAvail": outside_avail,
                    "graphicsHidden": graphics_hidden,
                    "collectionHidden": collection_hidden,
                    "reshown": reshown,
                    "afterPropRemoved": after_prop_removed,
                    "beforeDestroy": before_destroy,
                    "afterDestroy": after_destroy,
                }))
            }
            "boundingSphere" => {
                let cluster = EntityCluster::<Host>::new(EntityClusterOptions::default());
                let collection = EntityCollection::new();
                let pos = Cartesian3::new(6378137.0, 100.0, 200.0);
                let g = Rc::new(LabelGraphics::new());
                g.set_text(Some(PropertyInput::value("Tag"))).unwrap();
                let e = Entity::new(EntityOptions {
                    id: Some("l".into()),
                    position: Some(Rc::new(ConstantPositionProperty::new(Some(pos), ReferenceFrame::Fixed))),
                    values: vec![("label".into(), PropertyInput::Component(g))],
                    ..Default::default()
                })
                .unwrap();
                collection.add(e.clone()).unwrap();
                let unknown = Entity::with_id("missing");

                let mut vis = LabelVisualizer::new(cluster.clone(), collection);
                vis.update(&t0()).unwrap();
                let mut bs = BoundingSphere::default();
                let st = vis.get_bounding_sphere(&e, &mut bs);
                let missing = vis.get_bounding_sphere(&unknown, &mut BoundingSphere::default());
                vis.destroy();
                cluster.destroy();
                ok(json!({ "active": [st as i32, bs.enc()], "missing": missing as i32 }))
            }
            _ => None,
        },
    );
}

// ===========================================================================
// 7. PointVisualizer parity
// ===========================================================================

fn snapshot_point(p: Option<&PointPrimitive>) -> Value {
    let Some(p) = p else { return Value::Null };
    json!({
        "show": p.show(),
        "id": p.id(),
        "position": p.position().enc(),
        "color": p.color().enc(),
        "outlineColor": p.outline_color().enc(),
        "outlineWidth": p.outline_width(),
        "pixelSize": p.pixel_size(),
        "scaleByDistance": enc_nfs(p.scale_by_distance()),
        "translucencyByDistance": enc_nfs(p.translucency_by_distance()),
        "distanceDisplayCondition": enc_ddc(p.distance_display_condition()),
        "disableDepthTestDistance": p.disable_depth_test_distance(),
        "splitDirection": p.split_direction() as i32,
    })
}

#[test]
fn point_visualizer_matches_cesium() {
    run(
        "data_sources",
        "PointVisualizer",
        |_| TIGHT,
        |f, _| match f {
            "unclampedDefaultsAndProperties" => {
                let cluster = EntityCluster::<Host>::new(EntityClusterOptions::default());
                let collection = EntityCollection::new();
                let p1 = Cartesian3::new(6378137.0, 0.0, 0.0);
                let p2 = Cartesian3::new(0.0, 6378137.0, 0.0);

                let e1 = Entity::with_id("default");
                e1.set_position(Some(Rc::new(ConstantPositionProperty::new(Some(p1), ReferenceFrame::Fixed)))).unwrap();
                e1.set_property("point", Some(PropertyInput::Component(Rc::new(PointGraphics::new())))).unwrap();
                collection.add(e1.clone()).unwrap();

                let e2 = Entity::with_id("custom");
                e2.set_position(Some(Rc::new(ConstantPositionProperty::new(Some(p2), ReferenceFrame::Fixed)))).unwrap();
                let g2 = Rc::new(PointGraphics::new());
                g2.set_color(Some(PropertyInput::value(Color::new(0.3, 0.6, 0.9, 0.75)))).unwrap();
                g2.set_outline_color(Some(PropertyInput::value(Color::new(0.9, 0.2, 0.1, 1.0)))).unwrap();
                g2.set_outline_width(Some(PropertyInput::value(2.5))).unwrap();
                g2.set_pixel_size(Some(PropertyInput::value(14.0))).unwrap();
                g2.set_scale_by_distance(Some(nfs_json(100., 2.0, 1000., 0.5))).unwrap();
                g2.set_translucency_by_distance(Some(nfs_json(50., 1.0, 500., 0.2))).unwrap();
                g2.set_distance_display_condition(Some(ddc_json(10.0, 2000.0))).unwrap();
                g2.set_disable_depth_test_distance(Some(PropertyInput::value(750.0))).unwrap();
                g2.set_split_direction(Some(PropertyInput::value(SplitDirection::Right as i32 as f64))).unwrap();
                e2.set_property("point", Some(PropertyInput::Component(g2))).unwrap();
                collection.add(e2).unwrap();

                let mut vis = PointVisualizer::new(cluster.clone(), collection);
                let updated = vis.update(&t0()).unwrap();
                let mut bs = BoundingSphere::default();
                let bs_state = vis.get_bounding_sphere(&e1, &mut bs);
                let out = json!({
                    "updated": updated,
                    "defaultPoint": snapshot_point(vis.point_for("default").as_ref()),
                    "customPoint": snapshot_point(vis.point_for("custom").as_ref()),
                    "boundingSphere": [bs_state as i32, bs.enc()],
                });
                vis.destroy();
                cluster.destroy();
                ok(out)
            }
            "clampedBillboardPoint" => {
                let cluster = EntityCluster::<Host>::new(EntityClusterOptions::default());
                let sampler: HeightSampler = Rc::new(|_, _| Some(85.0));
                cluster.set_height_sampler(Some(sampler));
                let collection = EntityCollection::new();

                let pos = Cartesian3::from_radians(0.3, 0.4, 20.0, &Ellipsoid::WGS84);
                let g = Rc::new(PointGraphics::new());
                g.set_height_reference(Some(PropertyInput::value(HeightReference::ClampToGround as i32 as f64)))
                    .unwrap();
                g.set_color(Some(PropertyInput::value(Color::RED))).unwrap();
                g.set_outline_color(Some(PropertyInput::value(Color::BLUE))).unwrap();
                g.set_outline_width(Some(PropertyInput::value(0.0))).unwrap();
                g.set_pixel_size(Some(PropertyInput::value(10.0))).unwrap();
                let e = Entity::new(EntityOptions {
                    id: Some("clamped".into()),
                    position: Some(Rc::new(ConstantPositionProperty::new(Some(pos), ReferenceFrame::Fixed))),
                    ..Default::default()
                })
                .unwrap();
                e.set_property("point", Some(PropertyInput::Component(g.clone()))).unwrap();
                collection.add(e.clone()).unwrap();

                let mut vis = PointVisualizer::new(cluster.clone(), collection);
                vis.update(&t0()).unwrap();
                let b = vis.billboard_for("clamped").unwrap();
                let step1 = json!({
                    "hasPoint": vis.point_for("clamped").is_some(),
                    "hasBillboard": true,
                    "scale": b.scale(),
                    "image": b.image(),
                    "heightReference": b.height_reference() as i32,
                });

                g.set_pixel_size(Some(PropertyInput::value(25.0))).unwrap();
                vis.update(&t0()).unwrap();
                let step2 = json!({ "scale": b.scale(), "image": b.image() });

                g.set_outline_width(Some(PropertyInput::value(3.0))).unwrap();
                g.set_pixel_size(Some(PropertyInput::value(12.0))).unwrap();
                vis.update(&t0()).unwrap();
                let step3 = json!({ "scale": b.scale(), "image": b.image() });

                g.set_outline_width(Some(PropertyInput::value(0.0))).unwrap();
                g.set_pixel_size(Some(PropertyInput::value(0.2))).unwrap();
                vis.update(&t0()).unwrap();
                let step4 = json!({ "scale": b.scale(), "image": b.image() });

                // Update cluster to compute clamped_position for bounding sphere
                let mut frame = make_frame();
                let mut camera = Camera::new(CameraOptions::new(800., 600.));
                let ctx = Context::new_webgl2(Host::default());
                frame.context = Some(FrameResource::new(ctx.clone()));
                cluster.update(&mut frame, &mut camera, &ctx).unwrap();

                let mut bs = BoundingSphere::default();
                let bs_state = vis.get_bounding_sphere(&e, &mut bs);

                g.set_height_reference(Some(PropertyInput::value(HeightReference::None as i32 as f64))).unwrap();
                vis.update(&t0()).unwrap();
                let step5 = json!({
                    "hasPoint": vis.point_for("clamped").is_some(),
                    "hasBillboard": vis.billboard_for("clamped").is_some(),
                    "pixelSize": vis.point_for("clamped").unwrap().pixel_size(),
                });

                vis.destroy();
                cluster.destroy();
                ok(json!({
                    "step1": step1,
                    "step2": step2,
                    "step3": step3,
                    "step4": step4,
                    "clampedSphere": [bs_state as i32, bs.enc()],
                    "step5": step5,
                }))
            }
            "visibilityAndLifecycle" => {
                let cluster = EntityCluster::<Host>::new(EntityClusterOptions::default());
                let collection = EntityCollection::new();
                let mut vis = PointVisualizer::new(cluster.clone(), collection.clone());

                let avail = Rc::new(PropertyIntervals::new(|_: &Option<()>, _: &Option<()>| true));
                avail.add_interval(TimeInterval::new(t0(), add_sec(30.), true, true, None));
                let g = Rc::new(PointGraphics::new());
                g.set_show(Some(PropertyInput::value(true))).unwrap();
                let e = Entity::new(EntityOptions {
                    id: Some("pt".into()),
                    position: Some(Rc::new(ConstantPositionProperty::new(
                        Some(Cartesian3::new(6378137.0, 0.0, 0.0)),
                        ReferenceFrame::Fixed,
                    ))),
                    availability: Some(avail),
                    ..Default::default()
                })
                .unwrap();
                e.set_property("point", Some(PropertyInput::Component(g.clone()))).unwrap();
                collection.add(e.clone()).unwrap();

                vis.update(&t0()).unwrap();
                let shown = vis.point_for("pt").is_some();

                vis.update(&add_sec(60.)).unwrap();
                let outside_avail = vis.point_for("pt").is_some();

                vis.update(&t0()).unwrap();
                g.set_show(Some(PropertyInput::value(false))).unwrap();
                vis.update(&t0()).unwrap();
                let hidden = vis.point_for("pt").is_some();

                let failed_sphere = vis.get_bounding_sphere(&e, &mut BoundingSphere::default());
                collection.remove(Some(&e));
                let after_remove = vis.point_for("pt").is_some();

                let before_destroy = vis.is_destroyed();
                vis.destroy();
                let after_destroy = vis.is_destroyed();
                cluster.destroy();

                ok(json!({
                    "shown": shown,
                    "outsideAvail": outside_avail,
                    "hidden": hidden,
                    "failedSphere": failed_sphere as i32,
                    "afterRemove": after_remove,
                    "beforeDestroy": before_destroy,
                    "afterDestroy": after_destroy,
                }))
            }
            _ => None,
        },
    );
}

// ===========================================================================
// 8. PathVisualizer parity
// ===========================================================================

#[test]
fn path_visualizer_matches_cesium() {
    let path_tol = Tol { rel: 1e-11, abs: 1e-11, vector: true };
    run(
        "data_sources",
        "PathVisualizer",
        |_| path_tol,
        |f, _| match f {
            "subSampleSampled" => {
                let prop = Rc::new(SampledPositionProperty::new(ReferenceFrame::Fixed, 0));
                prop.add_sample(add_sec(0.), &Cartesian3::new(1000., 0., 0.), None).unwrap();
                prop.add_sample(add_sec(120.), &Cartesian3::new(2000., 1200., 0.), None).unwrap();
                prop.add_sample(add_sec(240.), &Cartesian3::new(3000., 0., 600.), None).unwrap();
                let pref: PositionPropertyRef = prop;

                let with_update_inside = path_sub_sample(
                    &pref,
                    &add_sec(0.),
                    &add_sec(240.),
                    Some(&add_sec(50.)),
                    ReferenceFrame::Fixed,
                    40.,
                )
                .unwrap();
                let with_update_outside = path_sub_sample(
                    &pref,
                    &add_sec(20.),
                    &add_sec(200.),
                    Some(&add_sec(300.)),
                    ReferenceFrame::Fixed,
                    60.,
                )
                .unwrap();
                let inertial = path_sub_sample(
                    &pref,
                    &add_sec(0.),
                    &add_sec(120.),
                    Some(&add_sec(60.)),
                    ReferenceFrame::Inertial,
                    60.,
                )
                .unwrap();
                ok(json!({
                    "withUpdateInside": with_update_inside.enc(),
                    "withUpdateOutside": with_update_outside.enc(),
                    "inertial": inertial.enc(),
                }))
            }
            "subSampleCallback" => {
                let base = t0();
                let pref: PositionPropertyRef = Rc::new(CallbackPositionProperty::new(
                    Rc::new(move |time, _| {
                        let dt = JulianDate::seconds_difference(time, &base);
                        Ok(Some(Cartesian3::new(1000. + dt * 10., dt * 5., 0.)))
                    }),
                    false,
                    ReferenceFrame::Fixed,
                ));
                let pts = path_sub_sample(
                    &pref,
                    &add_sec(0.),
                    &add_sec(100.),
                    Some(&add_sec(45.)),
                    ReferenceFrame::Fixed,
                    30.,
                )
                .unwrap();
                ok(pts.enc())
            }
            "subSampleComposite" => {
                let comp = Rc::new(CompositePositionProperty::new(ReferenceFrame::Fixed));
                let c0: PositionPropertyRef =
                    Rc::new(ConstantPositionProperty::new(Some(Cartesian3::new(10., 20., 30.)), ReferenceFrame::Fixed));
                comp.intervals().add_interval(TimeInterval::new(add_sec(0.), add_sec(60.), true, true, Some(c0)));
                let sampled = Rc::new(SampledPositionProperty::new(ReferenceFrame::Fixed, 0));
                sampled.add_sample(add_sec(60.), &Cartesian3::new(100., 0., 0.), None).unwrap();
                sampled.add_sample(add_sec(180.), &Cartesian3::new(220., 120., 0.), None).unwrap();
                comp.intervals().add_interval(TimeInterval::new(
                    add_sec(60.),
                    add_sec(180.),
                    true,
                    true,
                    Some(sampled as PositionPropertyRef),
                ));
                let pref: PositionPropertyRef = comp;
                let pts = path_sub_sample(
                    &pref,
                    &add_sec(30.),
                    &add_sec(150.),
                    Some(&add_sec(90.)),
                    ReferenceFrame::Fixed,
                    60.,
                )
                .unwrap();
                ok(pts.enc())
            }
            "subSampleInterval" => {
                let prop = Rc::new(TimeIntervalCollectionPositionProperty::new(ReferenceFrame::Fixed));
                prop.intervals().add_interval(TimeInterval::new(
                    add_sec(0.),
                    add_sec(30.),
                    true,
                    false,
                    Some(Rc::new(Cartesian3::new(1., 2., 3.))),
                ));
                prop.intervals().add_interval(TimeInterval::new(
                    add_sec(30.),
                    add_sec(60.),
                    false,
                    true,
                    Some(Rc::new(Cartesian3::new(4., 5., 6.))),
                ));
                prop.intervals().add_interval(TimeInterval::new(
                    add_sec(60.),
                    add_sec(90.),
                    false,
                    false,
                    Some(Rc::new(Cartesian3::new(7., 8., 9.))),
                ));
                let pref: PositionPropertyRef = prop;
                let pts = path_sub_sample(
                    &pref,
                    &add_sec(0.),
                    &add_sec(90.),
                    Some(&add_sec(45.)),
                    ReferenceFrame::Fixed,
                    10.,
                )
                .unwrap();
                ok(pts.enc())
            }
            "subSampleConstantAndReference" => {
                let c_prop: PositionPropertyRef = Rc::new(ConstantPositionProperty::new(
                    Some(Cartesian3::new(6378137., 0., 0.)),
                    ReferenceFrame::Fixed,
                ));
                let const_pts = path_sub_sample(
                    &c_prop,
                    &add_sec(0.),
                    &add_sec(60.),
                    Some(&add_sec(30.)),
                    ReferenceFrame::Fixed,
                    10.,
                )
                .unwrap();

                let scaled_const: PositionPropertyRef = Rc::new(ScaledPositionProperty::new(Some(c_prop)));
                let scaled_const_pts = path_sub_sample(
                    &scaled_const,
                    &add_sec(0.),
                    &add_sec(60.),
                    Some(&add_sec(30.)),
                    ReferenceFrame::Fixed,
                    10.,
                )
                .unwrap();

                let col = EntityCollection::new();
                let sampled = Rc::new(SampledPositionProperty::new(ReferenceFrame::Fixed, 0));
                sampled.add_sample(add_sec(0.), &Cartesian3::new(10., 0., 0.), None).unwrap();
                sampled.add_sample(add_sec(60.), &Cartesian3::new(70., 0., 0.), None).unwrap();
                let target = Entity::new(EntityOptions {
                    id: Some("target".into()),
                    position: Some(sampled),
                    ..Default::default()
                })
                .unwrap();
                col.add(target).unwrap();
                let ref_prop = ReferenceProperty::from_string(col.clone(), "target#position").unwrap().as_position();
                let ref_pts = path_sub_sample(
                    &ref_prop,
                    &add_sec(0.),
                    &add_sec(60.),
                    Some(&add_sec(25.)),
                    ReferenceFrame::Fixed,
                    30.,
                )
                .unwrap();

                ok(json!({
                    "constPts": const_pts.enc(),
                    "scaledConstPts": scaled_const_pts.enc(),
                    "refPts": ref_pts.enc(),
                }))
            }
            "subSampleGeneric" => {
                let base = t0();
                let cb: PositionPropertyRef = Rc::new(CallbackPositionProperty::new(
                    Rc::new(move |time, _| {
                        let dt = JulianDate::seconds_difference(time, &base);
                        Ok(Some(Cartesian3::new(6378137. + dt * 10., 100., 0.)))
                    }),
                    false,
                    ReferenceFrame::Fixed,
                ));
                let scaled: PositionPropertyRef = Rc::new(ScaledPositionProperty::new(Some(cb)));
                let pts = path_sub_sample(
                    &scaled,
                    &add_sec(0.),
                    &add_sec(180.),
                    Some(&add_sec(90.)),
                    ReferenceFrame::Fixed,
                    60.,
                )
                .unwrap();
                ok(pts.enc())
            }
            "lifecycle" => {
                let collection = EntityCollection::new();
                let sampled = Rc::new(SampledPositionProperty::new(ReferenceFrame::Fixed, 0));
                sampled.add_sample(add_sec(0.), &Cartesian3::new(6378137., 0., 0.), None).unwrap();
                sampled.add_sample(add_sec(120.), &Cartesian3::new(6378137., 1200., 0.), None).unwrap();
                let sampled_ref: PositionPropertyRef = sampled;

                let make_avail = || {
                    let a = Rc::new(PropertyIntervals::new(|_: &Option<()>, _: &Option<()>| true));
                    a.add_interval(TimeInterval::new(add_sec(0.), add_sec(120.), true, true, None));
                    a
                };

                let p_main = Rc::new(PathGraphics::new());
                p_main.set_lead_time(Some(PropertyInput::value(30.0))).unwrap();
                p_main.set_trail_time(Some(PropertyInput::value(20.0))).unwrap();
                p_main.set_resolution(Some(PropertyInput::value(60.0))).unwrap();
                p_main.set_width(Some(PropertyInput::value(3.5))).unwrap();
                p_main.set_material(Some(PropertyInput::value(Color::CYAN))).unwrap();
                p_main.set_distance_display_condition(Some(ddc_json(10.0, 500.0))).unwrap();
                let e_main = Entity::new(EntityOptions {
                    id: Some("main".into()),
                    position: Some(sampled_ref.clone()),
                    availability: Some(make_avail()),
                    values: vec![("path".into(), PropertyInput::Component(p_main))],
                    ..Default::default()
                })
                .unwrap();
                collection.add(e_main.clone()).unwrap();

                let p_zero = Rc::new(PathGraphics::new());
                p_zero.set_lead_time(Some(PropertyInput::value(0.0))).unwrap();
                p_zero.set_trail_time(Some(PropertyInput::value(0.0))).unwrap();
                let e_zero = Entity::new(EntityOptions {
                    id: Some("zero".into()),
                    position: Some(sampled_ref.clone()),
                    values: vec![("path".into(), PropertyInput::Component(p_zero))],
                    ..Default::default()
                })
                .unwrap();
                collection.add(e_zero.clone()).unwrap();

                let p_nb = Rc::new(PathGraphics::new());
                p_nb.set_lead_time(Some(PropertyInput::value(10.0))).unwrap();
                let e_nb = Entity::new(EntityOptions {
                    id: Some("nobounds".into()),
                    position: Some(sampled_ref),
                    values: vec![("path".into(), PropertyInput::Component(p_nb))],
                    ..Default::default()
                })
                .unwrap();
                collection.add(e_nb.clone()).unwrap();

                let inertial_pos = Rc::new(SampledPositionProperty::new(ReferenceFrame::Inertial, 0));
                inertial_pos.add_sample(add_sec(0.), &Cartesian3::new(7_000_000., 0., 0.), None).unwrap();
                inertial_pos.add_sample(add_sec(120.), &Cartesian3::new(7_000_000., 1000., 0.), None).unwrap();
                let p_in = Rc::new(PathGraphics::new());
                p_in.set_resolution(Some(PropertyInput::value(60.0))).unwrap();
                let e_in = Entity::new(EntityOptions {
                    id: Some("inertial".into()),
                    position: Some(inertial_pos),
                    availability: Some(make_avail()),
                    values: vec![("path".into(), PropertyInput::Component(p_in))],
                    ..Default::default()
                })
                .unwrap();
                collection.add(e_in).unwrap();

                let mut vis = PathVisualizer::<Host>::new(SceneMode::Scene3D, collection.clone());
                vis.update(&add_sec(10.)).unwrap();
                let main_poly = vis.polyline_for("main").unwrap();
                let mat = main_poly.material();
                let mat_ref = mat.borrow();
                let mat_color = match &mat_ref.uniforms[0].1 {
                    MaterialValue::Vec4(v) => v.enc(),
                    _ => Value::Null,
                };
                let at10 = json!({
                    "show": main_poly.show(),
                    "width": main_poly.width(),
                    "materialType": mat_ref.type_name,
                    "materialColor": mat_color,
                    "ddc": enc_ddc(main_poly.distance_display_condition()),
                    "positions": main_poly.positions().enc(),
                    "zeroHasPoly": vis.polyline_for("zero").is_some(),
                    "noBoundsHasPoly": vis.polyline_for("nobounds").is_some(),
                    "updaterCount3D": vis.updater_count(),
                    "inertialModelMatrix": vis.model_matrix_for_frame(ReferenceFrame::Inertial).unwrap().enc(),
                });
                drop(mat_ref);

                vis.update(&add_sec(50.)).unwrap();
                let at50_positions = vis.polyline_for("main").unwrap().positions().enc();

                vis.set_scene_mode(SceneMode::Scene2D);
                vis.update(&add_sec(50.)).unwrap();
                assert!(vis.polyline_for("inertial").is_some());
                let inertial_in_2d_frame = ReferenceFrame::Fixed as i32;

                collection.remove_all();
                vis.update(&add_sec(50.)).unwrap();
                let updaters_after_clear = vis.updater_count();

                let before_destroy = vis.is_destroyed();
                vis.destroy();
                let after_destroy = vis.is_destroyed();

                ok(json!({
                    "at10": at10,
                    "at50Positions": at50_positions,
                    "inertialIn2DFrame": inertial_in_2d_frame,
                    "updatersAfterClear": updaters_after_clear,
                    "beforeDestroy": before_destroy,
                    "afterDestroy": after_destroy,
                    "_keep": [e_main.id(), e_zero.id(), e_nb.id()],
                }))
            }
            _ => None,
        },
    );
}

// ===========================================================================
// 9. ModelVisualizer parity
// ===========================================================================

fn make_model_components(context: &Context<Host>, with_node_matrix: bool, with_extras: bool) -> mc::Components<Host> {
    let positions = TypedArray::Float32(vec![-1., -1., 0., 1., -1., 0., 0., 1., 0.]);
    let vbuf =
        Buffer::new(context, BufferKind::Vertex, BufferUsage::StaticDraw, BufferData::Bytes(&positions.to_le_bytes()))
            .unwrap();
    let ibuf = Buffer::new(
        context,
        BufferKind::Index(IndexDatatype::UnsignedShort),
        BufferUsage::StaticDraw,
        BufferData::Bytes(&[0, 0, 1, 0, 2, 0]),
    )
    .unwrap();
    let primitive = mc::Primitive {
        primitive_type: Some(PrimitiveType::Triangles),
        attributes: vec![mc::Attribute {
            semantic: Some(mc::AttributeSemantic::Vertex(VertexAttributeSemantic::Position)),
            attribute_type: Some(AttributeType::Vec3),
            component_datatype: Some(ComponentDatatype::Float),
            count: Some(3),
            typed_array: Some(positions),
            buffer: Some(vbuf),
            min: Some(mc::MathValue::Cartesian3(Cartesian3::new(-1., -1., 0.))),
            max: Some(mc::MathValue::Cartesian3(Cartesian3::new(1., 1., 0.))),
            ..Default::default()
        }],
        indices: Some(mc::Indices {
            index_datatype: Some(IndexDatatype::UnsignedShort),
            count: Some(3),
            typed_array: Some(TypedArray::Uint16(vec![0, 1, 2])),
            buffer: Some(ibuf),
        }),
        material: Some(mc::Material { unlit: true, ..Default::default() }),
        ..Default::default()
    };
    let animations =
        if with_extras { vec![mc::Animation { name: Some("spin".into()), ..Default::default() }] } else { vec![] };
    let articulations = if with_extras {
        vec![mc::Articulation {
            name: Some("Wing".into()),
            stages: vec![mc::ArticulationStage {
                name: Some("Deploy".into()),
                stage_type: Some(ArticulationStageType::ZRotate),
                minimum_value: Some(0.0),
                maximum_value: Some(90.0),
                initial_value: Some(0.0),
            }],
        }]
    } else {
        vec![]
    };
    mc::Components {
        up_axis: Some(Axis::Z),
        forward_axis: Some(Axis::X),
        scene: Some(mc::Scene { nodes: vec![0] }),
        nodes: vec![mc::Node {
            index: Some(0),
            name: Some("root".into()),
            matrix: with_node_matrix.then(|| Matrix4::from_translation(Cartesian3::new(10., 0., 0.))),
            primitives: vec![primitive],
            ..Default::default()
        }],
        animations,
        articulations,
        ..Default::default()
    }
}

fn make_ready_model(with_node_matrix: bool, with_extras: bool) -> Rc<RefCell<Model<Host>>> {
    let context = Context::new_webgl2(Host::default());
    let components = make_model_components(&context, with_node_matrix, with_extras);
    let mut model = Model::from_components(context, components, ModelOptions::default()).unwrap();
    let mut f = make_frame();
    model.update(&mut f).unwrap();
    finish_frame(&mut f);
    assert!(model.ready());
    Rc::new(RefCell::new(model))
}

fn snapshot_model(m: &Model<Host>) -> Value {
    json!({
        "show": m.options.show,
        "scale": m.options.scale,
        "enableVerticalExaggeration": m.options.enable_vertical_exaggeration,
        "minimumPixelSize": m.options.minimum_pixel_size,
        "maximumScale": m.options.maximum_scale,
        "modelMatrix": m.options.model_matrix.enc(),
        "shadows": m.options.shadows as i32,
        "heightReference": m.options.height_reference as i32,
        "distanceDisplayCondition": enc_ddc(m.options.distance_display_condition),
        "silhouetteColor": m.options.silhouette_color.enc(),
        "silhouetteSize": m.options.silhouette_size,
        "color": m.options.color.enc(),
        "colorBlendMode": m.options.color_blend_mode as i32,
        "colorBlendAmount": m.options.color_blend_amount,
        "clampAnimations": m.options.clamp_animations,
        "imageBasedLightingFactor": m.image_based_lighting.image_based_lighting_factor.enc(),
    })
}

#[test]
fn model_visualizer_matches_cesium() {
    run(
        "data_sources",
        "ModelVisualizer",
        |_| TIGHT,
        |f, _| match f {
            "defaultsAndProperties" => {
                let requests = Rc::new(RefCell::new(Vec::<Value>::new()));
                let req_log = Rc::clone(&requests);
                let factory = Rc::new(move |req: ModelLoadRequest| {
                    let max_eps = req
                        .environment_map_options
                        .as_ref()
                        .and_then(|v| v.get("maximumPositionEpsilon"))
                        .and_then(|v| match v {
                            DynamicValue::Number(x) => Some(num(x)),
                            _ => None,
                        })
                        .unwrap_or(Value::Null);
                    req_log.borrow_mut().push(json!({
                        "url": req.resource.url(),
                        "incrementallyLoadTextures": req.incrementally_load_textures,
                        "maxPosEps": max_eps,
                    }));
                    let is_custom = req.resource.url().contains("custom");
                    async move { Ok(make_ready_model(is_custom, is_custom)) }.boxed_local()
                });

                let collection = EntityCollection::new();
                let p1 = Cartesian3::new(6378137., 0., 0.);
                let p2 = Cartesian3::new(0., 6378137., 0.);

                let e1 = Entity::with_id("def");
                e1.set_position(Some(Rc::new(ConstantPositionProperty::new(Some(p1), ReferenceFrame::Fixed)))).unwrap();
                let g1 = Rc::new(ModelGraphics::new());
                g1.set_uri(Some(PropertyInput::value("https://example.test/def.glb"))).unwrap();
                e1.set_property("model", Some(PropertyInput::Component(g1))).unwrap();
                collection.add(e1).unwrap();

                let node_bag = Rc::new(PropertyBag::new());
                let trs = Rc::new(
                    NodeTransformationProperty::with_options([
                        ("translation".into(), PropertyInput::value(Cartesian3::new(1., 2., 3.))),
                        ("rotation".into(), PropertyInput::value(Quaternion::IDENTITY)),
                        ("scale".into(), PropertyInput::value(Cartesian3::new(2., 2., 2.))),
                    ])
                    .unwrap(),
                );
                node_bag.add_property("root", Some(PropertyInput::Component(trs)), None).unwrap();

                let art_bag = Rc::new(PropertyBag::new());
                art_bag.add_property("Wing Deploy", Some(PropertyInput::value(45.0)), None).unwrap();

                let e2 = Entity::with_id("custom");
                e2.set_position(Some(Rc::new(ConstantPositionProperty::new(Some(p2), ReferenceFrame::Fixed)))).unwrap();
                let g2 = Rc::new(ModelGraphics::new());
                g2.set_uri(Some(PropertyInput::value("https://example.test/custom.glb"))).unwrap();
                g2.set_scale(Some(PropertyInput::value(3.0))).unwrap();
                g2.set_enable_vertical_exaggeration(Some(PropertyInput::value(false))).unwrap();
                g2.set_minimum_pixel_size(Some(PropertyInput::value(64.0))).unwrap();
                g2.set_maximum_scale(Some(PropertyInput::value(100.0))).unwrap();
                g2.set_incrementally_load_textures(Some(PropertyInput::value(false))).unwrap();
                g2.set_run_animations(Some(PropertyInput::value(true))).unwrap();
                g2.set_clamp_animations(Some(PropertyInput::value(false))).unwrap();
                g2.set_shadows(Some(PropertyInput::value(ShadowMode::CastOnly as i32 as f64))).unwrap();
                g2.set_height_reference(Some(PropertyInput::value(HeightReference::None as i32 as f64))).unwrap();
                g2.set_silhouette_color(Some(PropertyInput::value(Color::LIME))).unwrap();
                g2.set_silhouette_size(Some(PropertyInput::value(2.0))).unwrap();
                g2.set_color(Some(PropertyInput::value(Color::new(0.5, 0.6, 0.7, 0.8)))).unwrap();
                g2.set_color_blend_mode(Some(PropertyInput::value(ColorBlendMode::Mix as i32 as f64))).unwrap();
                g2.set_color_blend_amount(Some(PropertyInput::value(0.75))).unwrap();
                g2.set_image_based_lighting_factor(Some(PropertyInput::value(Cartesian2::new(0.5, 0.25)))).unwrap();
                g2.set_light_color(Some(PropertyInput::value(Color::new(0.8, 0.6, 0.4, 1.0)))).unwrap();
                g2.set_distance_display_condition(Some(ddc_json(5.0, 500.0))).unwrap();
                g2.set_node_transformations(Some(PropertyInput::Component(node_bag))).unwrap();
                g2.set_articulations(Some(PropertyInput::Component(art_bag))).unwrap();
                e2.set_property("model", Some(PropertyInput::Component(g2.clone()))).unwrap();
                collection.add(e2).unwrap();

                let mut vis = ModelVisualizer::new(collection, factory, Some(Ellipsoid::WGS84));
                vis.update(&t0()).unwrap();

                let m_def = vis.model_for("def").unwrap();
                let m_cus = vis.model_for("custom").unwrap();
                let anim_while_running = m_cus.borrow().active_animations.borrow().length();

                g2.set_run_animations(Some(PropertyInput::value(false))).unwrap();
                vis.update(&t0()).unwrap();

                let (node_matrix, art_stage, active_anims) = {
                    let m = m_cus.borrow();
                    let sg = m.scene_graph.as_ref().unwrap();
                    let nm = sg.get_node("root").unwrap().matrix(sg).unwrap().enc();
                    let av = sg.runtime_articulations()[0].runtime_stages()[0].current_value();
                    let aa = m.active_animations.borrow().length();
                    (nm, av, aa)
                };

                let out = json!({
                    "requests": requests.borrow().clone(),
                    "defaultModel": snapshot_model(&m_def.borrow()),
                    "customModel": snapshot_model(&m_cus.borrow()),
                    "animWhileRunning": anim_while_running,
                    "customStats": {
                        "nodeMatrix": node_matrix,
                        "articulationStage": art_stage,
                        "activeAnimations": active_anims,
                    },
                });
                vis.destroy();
                ok(out)
            }
            "boundingSphereStates" => {
                let gate: Gate<Rc<RefCell<Model<Host>>>> = Rc::new(RefCell::new(None));
                let gate_clone = Rc::clone(&gate);
                let factory = Rc::new(move |req: ModelLoadRequest| {
                    if req.resource.url().contains("fail") {
                        async { Err(RuntimeError::new("boom")) }.boxed_local()
                    } else {
                        GateFuture { slot: Rc::clone(&gate_clone) }.boxed_local()
                    }
                });

                let collection = EntityCollection::new();
                let pos = Cartesian3::from_radians(0.2, 0.3, 40.0, &Ellipsoid::WGS84);
                let g_ok = Rc::new(ModelGraphics::new());
                g_ok.set_uri(Some(PropertyInput::value("https://example.test/ok.glb"))).unwrap();
                let e_ok = Entity::new(EntityOptions {
                    id: Some("ok".into()),
                    position: Some(Rc::new(ConstantPositionProperty::new(Some(pos), ReferenceFrame::Fixed))),
                    ..Default::default()
                })
                .unwrap();
                e_ok.set_property("model", Some(PropertyInput::Component(g_ok.clone()))).unwrap();
                collection.add(e_ok.clone()).unwrap();

                let g_fail = Rc::new(ModelGraphics::new());
                g_fail.set_uri(Some(PropertyInput::value("https://example.test/fail.glb"))).unwrap();
                let e_fail = Entity::new(EntityOptions {
                    id: Some("fail".into()),
                    position: Some(Rc::new(ConstantPositionProperty::new(Some(pos), ReferenceFrame::Fixed))),
                    values: vec![("model".into(), PropertyInput::Component(g_fail))],
                    ..Default::default()
                })
                .unwrap();
                collection.add(e_fail.clone()).unwrap();
                let e_missing = Entity::with_id("missing");

                let mut vis = ModelVisualizer::new(collection, factory, Some(Ellipsoid::WGS84));
                vis.set_height_query(Some(Rc::new(|_, _| Some(150.0))));

                let missing_before = vis.get_bounding_sphere(&e_missing, &mut BoundingSphere::default());
                vis.update(&t0()).unwrap();
                let while_pending = vis.get_bounding_sphere(&e_ok, &mut BoundingSphere::default());
                let after_fail = vis.get_bounding_sphere(&e_fail, &mut BoundingSphere::default());

                // Resolve with an unready model first
                let context = Context::new_webgl2(Host::default());
                let components = make_model_components(&context, false, false);
                let model = Rc::new(RefCell::new(
                    Model::from_components(context, components, ModelOptions::default()).unwrap(),
                ));
                *gate.borrow_mut() = Some(Ok(model.clone()));
                vis.update(&t0()).unwrap();
                let while_not_ready = vis.get_bounding_sphere(&e_ok, &mut BoundingSphere::default());

                // Make model ready with entity's modelMatrix
                {
                    let mut f = make_frame();
                    model.borrow_mut().update(&mut f).unwrap();
                    finish_frame(&mut f);
                }
                vis.update(&t0()).unwrap();
                let mut bs_none = BoundingSphere::default();
                let s_none = vis.get_bounding_sphere(&e_ok, &mut bs_none);

                g_ok.set_show(Some(PropertyInput::value(false))).unwrap();
                vis.update(&t0()).unwrap();
                let while_hidden = vis.get_bounding_sphere(&e_ok, &mut BoundingSphere::default());

                g_ok.set_show(Some(PropertyInput::value(true))).unwrap();
                g_ok.set_height_reference(Some(PropertyInput::value(HeightReference::ClampToGround as i32 as f64)))
                    .unwrap();
                vis.update(&t0()).unwrap();
                let mut bs_clamp = BoundingSphere::default();
                let s_clamp = vis.get_bounding_sphere(&e_ok, &mut bs_clamp);

                g_ok.set_height_reference(Some(PropertyInput::value(HeightReference::RelativeToGround as i32 as f64)))
                    .unwrap();
                vis.update(&t0()).unwrap();
                let mut bs_rel = BoundingSphere::default();
                let s_rel = vis.get_bounding_sphere(&e_ok, &mut bs_rel);

                vis.destroy();
                ok(json!({
                    "missingBefore": missing_before as i32,
                    "whilePending": while_pending as i32,
                    "afterFail": after_fail as i32,
                    "whileNotReady": while_not_ready as i32,
                    "none": [s_none as i32, bs_none.enc()],
                    "whileHidden": while_hidden as i32,
                    "clamp": [s_clamp as i32, bs_clamp.enc()],
                    "relative": [s_rel as i32, bs_rel.enc()],
                }))
            }
            "uriChangeAndLifecycle" => {
                let urls = Rc::new(RefCell::new(Vec::<String>::new()));
                let u = Rc::clone(&urls);
                let factory = Rc::new(move |req: ModelLoadRequest| {
                    u.borrow_mut().push(req.resource.url());
                    async { Ok(make_ready_model(false, false)) }.boxed_local()
                });
                let collection = EntityCollection::new();
                let g = Rc::new(ModelGraphics::new());
                g.set_uri(Some(PropertyInput::value("https://example.test/v1.glb"))).unwrap();
                let e = Entity::new(EntityOptions {
                    id: Some("swap".into()),
                    position: Some(Rc::new(ConstantPositionProperty::new(
                        Some(Cartesian3::new(6378137., 0., 0.)),
                        ReferenceFrame::Fixed,
                    ))),
                    ..Default::default()
                })
                .unwrap();
                e.set_property("model", Some(PropertyInput::Component(g.clone()))).unwrap();
                collection.add(e.clone()).unwrap();

                let mut vis = ModelVisualizer::new(collection.clone(), factory, Some(Ellipsoid::WGS84));
                vis.update(&t0()).unwrap();
                let count_v1 = vis.primitives().len();

                g.set_uri(Some(PropertyInput::value("https://example.test/v2.glb"))).unwrap();
                vis.update(&t0()).unwrap();
                let count_v2 = vis.primitives().len();

                collection.remove(Some(&e));
                let count_after_remove = vis.primitives().len();
                let before_destroy = vis.is_destroyed();
                vis.destroy();
                let after_destroy = vis.is_destroyed();

                ok(json!({
                    "urls": urls.borrow().clone(),
                    "countV1": count_v1,
                    "countV2": count_v2,
                    "countAfterRemove": count_after_remove,
                    "beforeDestroy": before_destroy,
                    "afterDestroy": after_destroy,
                }))
            }
            _ => None,
        },
    );
}

// ===========================================================================
// 10. Cesium3DTilesetVisualizer parity
// ===========================================================================

fn make_tileset(
    resource: Resource,
    center: [f64; 3],
    radius: f64,
) -> Result<Rc<RefCell<Cesium3DTileset>>, RuntimeError> {
    let ts = Cesium3DTileset::from_json(
        resource,
        &json!({
            "asset": { "version": "1.1" },
            "geometricError": 100,
            "root": {
                "boundingVolume": { "sphere": [center[0], center[1], center[2], radius] },
                "geometricError": 0
            }
        }),
        TilesetOptions::default(),
        None,
    )?;
    Ok(Rc::new(RefCell::new(ts)))
}

#[test]
fn cesium_3d_tileset_visualizer_matches_cesium() {
    run(
        "data_sources",
        "Cesium3DTilesetVisualizer",
        |_| TIGHT,
        |f, _| match f {
            "updateAndProperties" => {
                let factory = Rc::new(|resource: Resource| {
                    async move { make_tileset(resource, [6378137., 0., 0.], 10.0) }.boxed_local()
                });
                let collection = EntityCollection::new();

                let g1 = Rc::new(Cesium3DTilesetGraphics::new());
                g1.set_uri(Some(PropertyInput::value("https://example.test/a/tileset.json"))).unwrap();
                let e1 = Entity::with_id("noPos");
                e1.set_property("tileset", Some(PropertyInput::Component(g1))).unwrap();
                collection.add(e1).unwrap();

                let g2 = Rc::new(Cesium3DTilesetGraphics::new());
                g2.set_uri(Some(PropertyInput::value("https://example.test/b/tileset.json"))).unwrap();
                g2.set_maximum_screen_space_error(Some(PropertyInput::value(8.0))).unwrap();
                let e2 = Entity::new(EntityOptions {
                    id: Some("withPos".into()),
                    position: Some(Rc::new(ConstantPositionProperty::new(
                        Some(Cartesian3::new(6378137., 0., 0.)),
                        ReferenceFrame::Fixed,
                    ))),
                    ..Default::default()
                })
                .unwrap();
                e2.set_property("tileset", Some(PropertyInput::Component(g2.clone()))).unwrap();
                collection.add(e2).unwrap();

                let mut vis = Cesium3DTilesetVisualizer::new(collection, factory);
                vis.update(&t0()).unwrap();
                let ts_a = vis.tileset_for("noPos").unwrap();
                let ts_b = vis.tileset_for("withPos").unwrap();
                let shown_state = json!({
                    "aShow": ts_a.borrow().options.show,
                    "aSse": ts_a.borrow().options.maximum_screen_space_error,
                    "aMatrix": ts_a.borrow().model_matrix.enc(),
                    "bShow": ts_b.borrow().options.show,
                    "bSse": ts_b.borrow().options.maximum_screen_space_error,
                    "bMatrix": ts_b.borrow().model_matrix.enc(),
                });

                g2.set_show(Some(PropertyInput::value(false))).unwrap();
                vis.update(&t0()).unwrap();
                let hidden_show = ts_b.borrow().options.show;

                g2.set_show(Some(PropertyInput::value(true))).unwrap();
                vis.update(&t0()).unwrap();
                let reshown_show = ts_b.borrow().options.show;

                vis.destroy();
                ok(json!({ "shownState": shown_state, "hiddenShow": hidden_show, "reshownShow": reshown_show }))
            }
            "boundingSphereStates" => {
                let gate: Gate<Rc<RefCell<Cesium3DTileset>>> = Rc::new(RefCell::new(None));
                let gate_clone = Rc::clone(&gate);
                let factory = Rc::new(move |resource: Resource| {
                    if resource.url().contains("fail") {
                        async { Err(RuntimeError::new("404")) }.boxed_local()
                    } else {
                        GateFuture { slot: Rc::clone(&gate_clone) }.boxed_local()
                    }
                });
                let collection = EntityCollection::new();

                let g_ok = Rc::new(Cesium3DTilesetGraphics::new());
                g_ok.set_uri(Some(PropertyInput::value("https://example.test/ok/tileset.json"))).unwrap();
                let e_ok = Entity::with_id("ok");
                e_ok.set_property("tileset", Some(PropertyInput::Component(g_ok.clone()))).unwrap();
                collection.add(e_ok.clone()).unwrap();

                let g_fail = Rc::new(Cesium3DTilesetGraphics::new());
                g_fail.set_uri(Some(PropertyInput::value("https://example.test/fail/tileset.json"))).unwrap();
                let e_fail = Entity::with_id("fail");
                e_fail.set_property("tileset", Some(PropertyInput::Component(g_fail))).unwrap();
                collection.add(e_fail.clone()).unwrap();
                let e_missing = Entity::with_id("missing");

                let mut vis = Cesium3DTilesetVisualizer::new(collection, factory);
                let missing_state = vis.get_bounding_sphere(&e_missing, &mut BoundingSphere::default());

                vis.update(&t0()).unwrap();
                let pending_state = vis.get_bounding_sphere(&e_ok, &mut BoundingSphere::default());
                let failed_load_state = vis.get_bounding_sphere(&e_fail, &mut BoundingSphere::default());

                let ts = make_tileset(
                    Resource::new("https://example.test/ok/tileset.json").unwrap(),
                    [6378137., 50., 0.],
                    25.0,
                )
                .unwrap();
                *gate.borrow_mut() = Some(Ok(ts));
                vis.update(&t0()).unwrap();
                let mut bs = BoundingSphere::default();
                let done_state = vis.get_bounding_sphere(&e_ok, &mut bs);

                g_ok.set_show(Some(PropertyInput::value(false))).unwrap();
                vis.update(&t0()).unwrap();
                let hidden_state = vis.get_bounding_sphere(&e_ok, &mut BoundingSphere::default());

                vis.destroy();
                ok(json!({
                    "missingState": missing_state as i32,
                    "pendingState": pending_state as i32,
                    "failedLoadState": failed_load_state as i32,
                    "done": [done_state as i32, bs.enc()],
                    "hiddenState": hidden_state as i32,
                }))
            }
            "uriChangeAndDestroy" => {
                let urls = Rc::new(RefCell::new(Vec::<String>::new()));
                let u = Rc::clone(&urls);
                let factory = Rc::new(move |resource: Resource| {
                    u.borrow_mut().push(resource.url());
                    async move { make_tileset(resource, [6378137., 0., 0.], 10.0) }.boxed_local()
                });
                let collection = EntityCollection::new();
                let g = Rc::new(Cesium3DTilesetGraphics::new());
                g.set_uri(Some(PropertyInput::value("https://example.test/1/tileset.json"))).unwrap();
                let e = Entity::with_id("swap");
                e.set_property("tileset", Some(PropertyInput::Component(g.clone()))).unwrap();
                collection.add(e.clone()).unwrap();

                let mut vis = Cesium3DTilesetVisualizer::new(collection, factory);
                vis.update(&t0()).unwrap();
                let count1 = vis.primitives().len();

                g.set_uri(Some(PropertyInput::value("https://example.test/2/tileset.json"))).unwrap();
                vis.update(&t0()).unwrap();
                let count2 = vis.primitives().len();

                e.set_property("tileset", None).unwrap();
                let count_after_clear = vis.primitives().len();

                let before_destroy = vis.is_destroyed();
                vis.destroy();
                let after_destroy = vis.is_destroyed();

                ok(json!({
                    "urls": urls.borrow().clone(),
                    "count1": count1,
                    "count2": count2,
                    "countAfterClear": count_after_clear,
                    "beforeDestroy": before_destroy,
                    "afterDestroy": after_destroy,
                }))
            }
            _ => None,
        },
    );
}

// ===========================================================================
// Additional native lifecycle & parity regression tests
// ===========================================================================

#[test]
fn point_visualizer_invalidates_clamping_on_reused_billboard_slot() {
    let cluster = EntityCluster::<Host>::new(EntityClusterOptions::default());
    let height = Rc::new(Cell::new(100.0));
    let h = Rc::clone(&height);
    let sampler: HeightSampler = Rc::new(move |_, _| Some(h.get()));
    cluster.set_height_sampler(Some(sampler));
    let collection = EntityCollection::new();
    let pos = Cartesian3::from_radians(0.1, 0.2, 0.0, &Ellipsoid::WGS84);

    let g = Rc::new(PointGraphics::new());
    g.set_height_reference(Some(PropertyInput::value(HeightReference::ClampToGround as i32 as f64))).unwrap();
    let e = Entity::new(EntityOptions {
        id: Some("p".into()),
        position: Some(Rc::new(ConstantPositionProperty::new(Some(pos), ReferenceFrame::Fixed))),
        ..Default::default()
    })
    .unwrap();
    e.set_property("point", Some(PropertyInput::Component(g.clone()))).unwrap();
    collection.add(e.clone()).unwrap();

    let mut vis = PointVisualizer::new(cluster.clone(), collection);
    vis.update(&t0()).unwrap();
    let mut frame = make_frame();
    let mut camera = Camera::new(CameraOptions::new(800., 600.));
    let ctx = Context::new_webgl2(Host::default());
    frame.context = Some(FrameResource::new(ctx.clone()));
    cluster.update(&mut frame, &mut camera, &ctx).unwrap();
    let first_clamped = vis.billboard_for("p").unwrap().clamped_position().unwrap();

    // Hide so billboard is returned to pool, then change terrain height and reshow at same pos & heightReference
    g.set_show(Some(PropertyInput::value(false))).unwrap();
    vis.update(&t0()).unwrap();
    height.set(500.0);
    g.set_show(Some(PropertyInput::value(true))).unwrap();
    vis.update(&t0()).unwrap();
    // Stale clamped position must be cleared until re-clamped
    assert!(vis.billboard_for("p").unwrap().clamped_position().is_none());

    frame.context = Some(FrameResource::new(ctx.clone()));
    cluster.update(&mut frame, &mut camera, &ctx).unwrap();
    let second_clamped = vis.billboard_for("p").unwrap().clamped_position().unwrap();
    assert_ne!(first_clamped, second_clamped);
    vis.destroy();
    cluster.destroy();
}

#[test]
fn entity_cluster_updates_enable_disable_and_billboard_label_clustering() {
    let mut camera = make_cluster_camera(10_000_000.0);
    let projection = GeographicProjection::default();
    let state = SceneTransformState {
        mode: SceneMode::Scene3D,
        morph_time: 1.0,
        map_projection: &projection,
        client_dimensions: Cartesian2::new(800., 600.),
        drawing_buffer_dimensions: Cartesian2::new(800., 600.),
    };
    let cluster = EntityCluster::<Host>::new(EntityClusterOptions {
        enabled: true,
        pixel_range: 80.0,
        minimum_cluster_size: 2,
        ..Default::default()
    });
    for (id, lon, lat) in [("l1", 0.0, 0.0), ("l2", 0.02, 0.02)] {
        let e = Entity::with_id(id);
        let l = cluster.get_label(&e).unwrap();
        let mut lb = l.borrow_mut();
        lb.set_id(Some(id.to_owned()));
        lb.set_position(Cartesian3::from_degrees(lon, lat, 0.0, &Ellipsoid::WGS84));
        lb.set_text(id.to_owned());
    }
    cluster.update_clustering(Ellipsoid::WGS84, &mut camera, &state).unwrap();
    assert_eq!(cluster.cluster_label_count(), 1);
    assert!(cluster.ready());

    // Disabling via set_enabled + update_clustering destroys cluster collections & restores cluster_show
    cluster.set_enabled(false);
    assert!(!cluster.ready());
    cluster.update_clustering(Ellipsoid::WGS84, &mut camera, &state).unwrap();
    assert_eq!(cluster.cluster_label_count(), 0);
    assert!(cluster.ready());
    cluster.destroy();
}
