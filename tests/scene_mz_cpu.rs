use cesium_rs::scene::{scene::ScenePrimitive, spatial_node::RenderableKeyframe};
use cesium_rs::{core::*, renderer::UniformFrame, scene::*};
use serde_json::{json, Value};
use std::{
    cell::RefCell,
    rc::Rc,
    sync::{Arc, Mutex},
    task::{Context, Poll},
};
fn fixture() -> Value {
    let root: Value = serde_json::from_str(include_str!("fixtures/scene/MzCpu.json")).unwrap();
    Value::Object(
        root["cases"]
            .as_array()
            .unwrap()
            .iter()
            .map(|c| (c["name"].as_str().unwrap().to_string(), c["value"].clone()))
            .collect(),
    )
}
fn v(j: &Value) -> Cartesian3 {
    Cartesian3::new(j[0].as_f64().unwrap(), j[1].as_f64().unwrap(), j[2].as_f64().unwrap())
}
fn close(actual: f64, expected: f64) {
    assert!((actual - expected).abs() <= 1e-12 * expected.abs().max(1.0), "{actual} != {expected}");
}
fn check_vec(a: Cartesian3, b: &Value) {
    for (a, b) in a.to_array().iter().zip(b.as_array().unwrap()) {
        close(*a, b.as_f64().unwrap());
    }
}
fn check_obb(a: OrientedBoundingBox, b: &Value) {
    let mut packed = [0.; 12];
    a.pack(&mut packed, 0);
    for (a, b) in packed.iter().zip(b.as_array().unwrap()) {
        close(*a, b.as_f64().unwrap());
    }
}
fn shape() -> VoxelBoxShape {
    let mut s = VoxelBoxShape::default();
    assert!(s.update(Matrix4::IDENTITY, Cartesian3::new(-1., -1., -1.), Cartesian3::new(1., 1., 1.), None, None));
    s
}
fn frame() -> FrameState {
    let mut c = Camera::new(CameraOptions::new(16., 16.));
    let u = c.uniform_camera();
    let volume = c.frustum.compute_culling_volume(u.position, u.direction, u.up);
    FrameState::new(UniformFrame::new(u, JulianDate::new(2451545., 0., TimeStandard::Tai)), volume, [16, 16])
}
#[test]
fn voxel_bounds_and_transformed_boxes_match_reference() {
    let f = fixture();
    for c in f["bounds"].as_array().unwrap() {
        let t = VoxelShapeType::from_str_value(c["type"].as_str().unwrap()).unwrap();
        check_vec(t.min_bounds(), &c["min"]);
        check_vec(t.max_bounds(), &c["max"]);
    }
    for c in f["boxes"].as_array().unwrap() {
        let mut s = VoxelBoxShape::default();
        let m = Matrix4::from_array(
            &c["model"].as_array().unwrap().iter().map(|v| v.as_f64().unwrap()).collect::<Vec<_>>(),
            0,
        );
        assert!(s.update(m, v(&c["min"]), v(&c["max"]), None, None));
        let level = c["level"].as_u64().unwrap() as u32;
        check_obb(s.oriented_bounding_box, &c["box"]);
        check_obb(s.tile_obb(level, [0, 0, 0]), &c["tile"]);
        check_obb(s.sample_obb(level, [0, 0, 0], v(&c["dims"]), v(&c["uv"])), &c["sample"]);
        check_vec(s.convert_local_to_shape_uv_space(Cartesian3::new(0.2, 0.1, -0.1)), &c["uvResult"]);
        s.update_view_transforms(Matrix3::IDENTITY).unwrap();
    }
}
#[test]
fn clipped_degenerate_and_singular_boxes() {
    let mut s = shape();
    let lo = Cartesian3::new(-1., -1., -1.);
    let hi = Cartesian3::new(1., 1., 1.);
    assert!(s.update(Matrix4::IDENTITY, lo, hi, Some(Cartesian3::ZERO), None));
    assert_eq!(s.oriented_bounding_box.center, Cartesian3::new(0.5, 0.5, 0.5));
    assert!(s.update(Matrix4::IDENTITY, Cartesian3::new(0., -1., -1.), Cartesian3::new(0., 1., 1.), None, None));
    assert!(!s.update(Matrix4::IDENTITY, Cartesian3::ZERO, Cartesian3::new(0., 0., 1.), None, None));
    assert!(!s.update(Matrix4::from_scale(Cartesian3::new(1., 0., 1.)), lo, hi, None, None));
}
#[test]
fn spatial_selection_and_error_match_reference() {
    let s = shape();
    let dims = Cartesian3::new(2., 2., 2.);
    let mut parent = SpatialNode::new(0, [0, 0, 0], &s, dims);
    parent.renderable_keyframes =
        [0., 2., 4.].into_iter().map(|keyframe| RenderableKeyframe { level: 0, coords: [0, 0, 0], keyframe }).collect();
    for c in fixture()["temporal"].as_array().unwrap() {
        let mut node = SpatialNode::new(1, [1, 0, 1], &s, dims);
        node.renderable_keyframes =
            [1., 3.].into_iter().map(|keyframe| RenderableKeyframe { level: 1, coords: [1, 0, 1], keyframe }).collect();
        node.compute_surrounding_renderable_keyframe_nodes(c["location"].as_f64().unwrap(), &[&parent]);
        for (actual, key) in [(node.previous.as_ref().unwrap(), "prev"), (node.next.as_ref().unwrap(), "next")] {
            assert_eq!(actual.level, c[key][0].as_u64().unwrap() as u32);
            close(actual.keyframe, c[key][1].as_f64().unwrap());
        }
        close(node.lerp, c["lerp"].as_f64().unwrap());
        node.compute_screen_space_error(Cartesian3::new(3., 4., 5.), 2.);
        close(node.screen_space_error, c["error"].as_f64().unwrap());
    }
    let children = parent.construct_child_nodes(&s);
    assert_eq!(children.len(), 8);
    assert_eq!(children[7].coords, [1, 1, 1]);
    parent.create_keyframe_node(4.);
    parent.create_keyframe_node(1.);
    parent.create_keyframe_node(4.);
    assert_eq!(parent.keyframes, vec![1., 4.]);
    parent.compute_surrounding_renderable_keyframe_nodes(1., &[]);
    parent.visited_frame_number = 3;
    assert!(parent.is_renderable(3));
    assert!(!parent.is_renderable(2));
    parent.compute_screen_space_error(Cartesian3::ZERO, 1.);
    assert!(parent.screen_space_error.is_finite());
}
#[test]
fn padded_voxel_cells_keep_typed_metadata_and_bounds() {
    let s = shape();
    let node = SpatialNode::new(0, [0, 0, 0], &s, Cartesian3::new(4., 4., 4.));
    let content = VoxelContent::from_metadata_array(vec![
        TypedArray::Uint16((0..64).map(|i| i * 3).collect()),
        TypedArray::Float32((0..192).map(|i| i as f32 * 0.25).collect()),
    ]);
    for c in fixture()["cells"].as_array().unwrap() {
        let sample = c["sample"].as_u64().unwrap() as usize;
        let cell = VoxelCell::from_node(
            99,
            7,
            sample,
            &node,
            &content,
            &[("density".into(), MetadataType::Scalar), ("velocity".into(), MetadataType::Vec3)],
            &s,
            Cartesian3::new(2., 2., 2.),
            Cartesian3::new(1., 1., 1.),
        )
        .unwrap();
        assert_eq!(cell.get_names(), vec!["density", "velocity"]);
        assert!(cell.has_property("density"));
        assert!(!cell.has_property("missing"));
        for name in ["density", "velocity"] {
            let got = cell.get_property(name).unwrap().to_f64_vec();
            let want: Vec<f64> = c[name].as_array().unwrap().iter().map(|x| x.as_f64().unwrap()).collect();
            assert_eq!(got, want);
        }
        assert!(matches!(cell.get_property("density"), Some(TypedArray::Uint16(_))));
        check_obb(cell.oriented_bounding_box(), &c["box"]);
    }
}
#[test]
fn voxel_content_requests_one_more_frame_and_rejects_use_after_destroy() {
    let mut c = VoxelContent::from_metadata_array(vec![]);
    let mut f = frame();
    let mut values = vec![json!(c.ready())];
    c.update(&mut f).unwrap();
    values.extend([json!(c.ready()), json!(f.after_render.len())]);
    c.update(&mut f).unwrap();
    values.push(json!(f.after_render.len()));
    assert!(f.after_render.pop().unwrap()().unwrap());
    c.destroy();
    values.push(json!(c.is_destroyed()));
    assert_eq!(json!(values), fixture()["content"]);
    assert!(c.update(&mut f).is_err());
}
#[test]
fn performance_sampling_matches_reference() {
    let mut d = PerformanceDisplay::new(0.);
    for c in fixture()["performance"].as_array().unwrap() {
        d.update(c["time"].as_f64().unwrap(), c["rendered"].as_bool().unwrap());
        assert_eq!(d.fps_text, c["fps"].as_str().unwrap());
        assert_eq!(d.ms_text, c["ms"].as_str().unwrap());
    }
    d.set_throttled(true);
    assert_eq!(d.throttled_text(), "(throttled)");
    d.set_throttled(false);
    assert_eq!(d.throttled_text(), "");
}
#[test]
fn terrain_polls_pending_success_and_error_once() {
    let mut cx = Context::from_waker(futures_util::task::noop_waker_ref());
    let events = Arc::new(Mutex::new(vec![json!(false)]));
    let capture = events.clone();
    let mut terrain = Terrain::new(async { Ok(42) });
    terrain.ready_event.add_event_listener(move |p| capture.lock().unwrap().push(json!(p)));
    assert!(!terrain.ready());
    assert_eq!(terrain.poll(&mut cx), Poll::Ready(()));
    events.lock().unwrap().push(json!(terrain.ready()));
    assert_eq!(json!(*events.lock().unwrap()), fixture()["terrain"]);
    assert_eq!(terrain.provider(), Some(&42));
    assert_eq!(terrain.poll(&mut cx), Poll::Ready(()));
    let (tx, rx) = futures_channel::oneshot::channel();
    let mut terrain = Terrain::<u32>::new(async { rx.await.unwrap() });
    assert_eq!(terrain.poll(&mut cx), Poll::Pending);
    let errors = Arc::new(Mutex::new(0));
    let capture = errors.clone();
    terrain.error_event.add_event_listener(move |_| *capture.lock().unwrap() += 1);
    tx.send(Err(RuntimeError::new("bad terrain"))).unwrap();
    assert_eq!(terrain.poll(&mut cx), Poll::Ready(()));
    assert_eq!(terrain.poll(&mut cx), Poll::Ready(()));
    assert!(!terrain.ready());
    assert_eq!(*errors.lock().unwrap(), 1);
}
struct Mock {
    id: i32,
    events: Rc<RefCell<Vec<i32>>>,
}
impl ScenePrimitive for Mock {
    fn pre_passes_update(&mut self, _: &mut FrameState) -> Result<(), RuntimeError> {
        Ok(())
    }
    fn post_passes_update(&mut self, _: &mut FrameState) -> Result<(), RuntimeError> {
        Ok(())
    }
    fn update_for_pass(&mut self, f: &mut FrameState, _: Cesium3DTilePass) -> Result<(), RuntimeError> {
        self.update(f)
    }
    fn update(&mut self, _: &mut FrameState) -> Result<(), RuntimeError> {
        self.events.borrow_mut().push(self.id);
        Ok(())
    }
    fn destroy(&mut self) -> Result<(), RuntimeError> {
        self.events.borrow_mut().push(-self.id);
        Ok(())
    }
}
#[test]
fn ground_order_updates_and_destruction_match_reference() {
    let mut c = OrderedGroundPrimitiveCollection::default();
    let events = Rc::new(RefCell::new(vec![]));
    let mut f = frame();
    let add = |c: &mut OrderedGroundPrimitiveCollection, id, z| {
        c.add(Box::new(Mock { id, events: events.clone() }), z).unwrap()
    };
    let a = add(&mut c, 1, 5.);
    let b = add(&mut c, 2, -1.);
    let _ = add(&mut c, 3, 5.);
    c.update(&mut f).unwrap();
    c.set(a, -1.).unwrap();
    c.update(&mut f).unwrap();
    c.remove(b, false).unwrap();
    c.show = false;
    c.update(&mut f).unwrap();
    c.remove_all().unwrap();
    assert_eq!(json!(*events.borrow()), fixture()["ordered"]);
    assert!(c.is_empty());
    assert!(!c.contains(a));
    assert!(!c.set(100, 0.).unwrap());
    assert!(c.add(Box::new(Mock { id: 4, events: events.clone() }), f64::NAN).is_err());
    let id = add(&mut c, 5, 0.);
    let owned = c.remove(id, true).unwrap().unwrap();
    assert!(!events.borrow().contains(&-5));
    drop(owned);
    c.destroy().unwrap();
    assert!(c.update(&mut f).is_err());
}
#[test]
fn structural_tileset_content_loads_before_ready_and_propagates_failure() {
    let mut loaded = 0;
    let resource = Resource::new("https://example.test/tiles.json?q=1").unwrap();
    let mut c = Tileset3DTileContent::from_json(7, resource.clone(), &json!({}), |_, _, tile| {
        assert_eq!(tile, 7);
        loaded += 1;
        Ok(())
    })
    .unwrap();
    assert_eq!(
        json!({"loaded":loaded,"ready":c.ready(),"url":c.url(),"features":c.metrics().features,"has":c.has_property(0,"x")}),
        fixture()["tileset"]
    );
    assert_eq!(c.metrics().memory(), 0);
    c.destroy().unwrap();
    assert!(c.process(&frame()).is_err());
    assert!(Tileset3DTileContent::from_json(7, resource, &json!({}), |_, _, _| Err(RuntimeError::new("invalid root")))
        .is_err());
}
#[test]
fn vector_batch_matches_reference() {
    let b = Vector3DTileBatch::new(3, 12, Color::new(0.1, 0.2, 0.3, 0.4), vec![2, 8]);
    assert_eq!(
        json!({"offset":b.offset,"count":b.count,"color":[b.color.red,b.color.green,b.color.blue,b.color.alpha],"ids":b.batch_ids}),
        fixture()["batch"]
    );
}
