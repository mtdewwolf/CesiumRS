mod common;
use cesium_rs::{core::*, scene::*};
use common::*;
use serde_json::{json, Value};
use std::{
    cell::RefCell,
    rc::Rc,
    sync::{Arc, Mutex},
};
fn stencil_snapshot(rs: &cesium_rs::renderer::RenderState) -> Value {
    let t = &rs.stencil_test;
    json!({"stencilMask":rs.stencil_mask,"reference":t.reference,"mask":t.mask,"frontFunction":t.front_function,"backFunction":t.back_function,"frontOperation":[t.front_operation.fail,t.front_operation.z_fail,t.front_operation.z_pass],"backOperation":[t.back_operation.fail,t.back_operation.z_fail,t.back_operation.z_pass],"cull":[rs.cull.enabled,rs.cull.face],"colorMask":[rs.color_mask.red,rs.color_mask.green,rs.color_mask.blue,rs.color_mask.alpha],"polygonOffset":[rs.polygon_offset.enabled,rs.polygon_offset.factor,rs.polygon_offset.units],"depthMask":rs.depth_mask})
}
#[test]
fn stencil_reference() {
    run(
        "scene",
        "TilesetStencil",
        |_| EXACT,
        |_, a| {
            let mut state =
                cesium_rs::renderer::RenderState { depth_mask: a[2].as_bool().unwrap(), ..Default::default() };
            state.cull.enabled = a[1].as_bool().unwrap();
            Some(Out::Ok(json!([
                stencil_snapshot(&TilesetDrawKind::Regular.render_state(&state)),
                stencil_snapshot(&TilesetDrawKind::Backface.render_state(&state)),
                stencil_snapshot(&TilesetDrawKind::Stencil { selection_depth: u(&a[0]) as usize }.render_state(&state))
            ])))
        },
    );
}
struct DrawContent(usize);
impl TilesetContent for DrawContent {
    fn metrics(&self) -> TileContentMetrics {
        Default::default()
    }
    fn process(&mut self, _: &FrameState) -> Result<bool, RuntimeError> {
        Ok(true)
    }
    fn update(&mut self, f: &mut FrameState) -> Result<(), RuntimeError> {
        let mut command =
            SceneCommand::new(self.0.to_string(), cesium_rs::renderer::Pass::Cesium3DTile, Rc::new(|_| Ok(())));
        command.metadata.owner = Some(self.0.to_string());
        f.command_list.push(command);
        Ok(())
    }
    fn pick(&self, _: &Ray, _: &FrameState) -> Result<Option<Cartesian3>, RuntimeError> {
        Ok(None)
    }
    fn destroy(&mut self) -> Result<(), RuntimeError> {
        Ok(())
    }
}
#[test]
fn mixed_draw_reference() {
    run(
        "scene",
        "TilesetMixedDraw",
        |_| EXACT,
        |_, a| {
            let tile = |radius: f64, error: f64, children: Option<Vec<Value>>| {
                let mut v = json!({"boundingVolume":{"sphere":[6378137,0,0,radius]},"geometricError":error,"refine":"REPLACE","content":{"uri":format!("{radius}.glb")}});
                if let Some(children) = children {
                    v["children"] = json!(children);
                }
                v
            };
            let json = json!({"asset":{"version":"1.1"},"geometricError":1000,"root":tile(300.,500.,Some(vec![tile(100.,100.,Some(vec![tile(30.,0.,None),tile(20.,0.,None)]))]))});
            let mut t = Cesium3DTileset::from_json(
                Resource::new("https://example.com/tileset.json").unwrap(),
                &json,
                TilesetOptions {
                    skip_level_of_detail: a[0].as_bool().unwrap(),
                    dynamic_screen_space_error: false,
                    foveated_screen_space_error: false,
                    cull_with_children_bounds: false,
                    cull_requests_while_moving: false,
                    ..Default::default()
                },
                None,
            )
            .unwrap();
            for id in [0, 2] {
                t.install_content(id, Box::new(DrawContent(id))).unwrap();
            }
            let mut camera = Camera::new(CameraOptions::new(800., 600.));
            camera.position = Cartesian3::new(6379137., 0., 0.);
            camera.direction = Cartesian3::new(-1., 0., 0.);
            camera.up = Cartesian3::UNIT_Z;
            camera.right = Cartesian3::UNIT_Y;
            if let CameraFrustum::Perspective(f) = &mut camera.frustum {
                f.fov = std::f64::consts::PI / 3.;
                f.near = 1.;
                f.far = 1e8;
            }
            let snapshot = camera.uniform_camera();
            let culling = camera.frustum.compute_culling_volume(snapshot.position, snapshot.direction, snapshot.up);
            let mut f = FrameState::new(
                cesium_rs::renderer::UniformFrame::new(snapshot, JulianDate::new(2451545., 0., TimeStandard::Tai)),
                culling,
                [800, 600],
            );
            f.frame_number = 5;
            f.stencil_buffer = a[1].as_bool().unwrap();
            t.pre_passes_update(&mut f).unwrap();
            t.update_for_pass(&mut f, &mut TilesetPassState::new(Cesium3DTilePass::Render)).unwrap();
            let commands=f.command_list.iter().map(|c|{let mut rs=c.tileset_draw.map_or_else(Default::default,|d|d.render_state(&cesium_rs::renderer::RenderState::default()));if let Some(mask)=c.stencil_clear{rs.stencil_mask=mask as i64;}json!({"kind":if c.stencil_clear.is_some(){"clear"}else if c.tileset_draw==Some(TilesetDrawKind::Backface){"backface"}else{"front"},"tile":c.metadata.owner.as_ref().map(|id|id.parse::<usize>().unwrap()),"reference":rs.stencil_test.reference,"mask":rs.stencil_mask})}).collect::<Vec<_>>();
            Some(Out::Ok(
                json!({"mixed":t.has_mixed_content,"selected":t.selected_tiles,"commands":commands,"count":t.statistics.number_of_commands}),
            ))
        },
    );
}
fn plane(v: &Value) -> ClippingPlaneHandle {
    ClippingPlane::handle(Cartesian3::new(n(&v[0]), n(&v[1]), n(&v[2])), n(&v[3]))
}
fn values(p: &ClippingPlane) -> Vec<f64> {
    let normal = p.normal();
    vec![normal.x, normal.y, normal.z, p.distance()]
}
#[test]
fn plane_reference() {
    run(
        "scene",
        "ClippingPlane",
        |_| EXACT,
        |_, a| {
            let mut p = plane(&a[0]).borrow().clone();
            p.index = 3;
            let changes = Rc::new(RefCell::new(Vec::new()));
            let events = changes.clone();
            p.on_change_callback = Some(Rc::new(RefCell::new(move |index: isize, p: &ClippingPlane| {
                events.borrow_mut().push(json!([index, p.normal().enc(), p.distance()]))
            })));
            let mut result = Vec::new();
            for op in a[1].as_array().unwrap() {
                match s(&op[0]) {
                    "distance" => p.set_distance(n(&op[1])),
                    "normal" => p.set_normal(Cartesian3::new(n(&op[1]), n(&op[2]), n(&op[3]))),
                    "component" => p.set_normal_component(
                        match s(&op[1]) {
                            "x" => 0,
                            "y" => 1,
                            "z" => 2,
                            _ => unreachable!(),
                        },
                        n(&op[2]),
                    ),
                    "cloneInto" => plane(&op[1]).borrow().clone_into(&mut p),
                    _ => unreachable!(),
                }
                let cloned = p.clone();
                let mut clone = values(&cloned);
                clone.push(cloned.index as f64);
                result.push(json!({"plane":values(&p),"clone":clone,"changes":*changes.borrow()}));
            }
            Some(Out::Ok(json!(result)))
        },
    );
}
#[test]
fn collection_reference() {
    run(
        "scene",
        "ClippingPlaneCollection",
        |_| TIGHT,
        |_, a| {
            let mut collection = ClippingPlaneCollection::new(a[0].as_array().unwrap().iter().map(plane));
            collection.model_matrix = m4(&a[2]);
            let transform = m4(&a[3]);
            let events = Arc::new(Mutex::new(Vec::new()));
            for added in [true, false] {
                let events = events.clone();
                let callback = move |e: &ClippingPlaneEvent| {
                    let p = e.plane.borrow();
                    let v = values(&p);
                    events.lock().unwrap().push(json!([
                        if added { "add" } else { "remove" },
                        e.index,
                        v[0],
                        v[1],
                        v[2],
                        v[3],
                        p.index
                    ]));
                };
                if added {
                    collection.plane_added.add_event_listener(callback);
                } else {
                    collection.plane_removed.add_event_listener(callback);
                }
            }
            let mut result = Vec::new();
            for op in a[1].as_array().unwrap() {
                let mut returned = Value::Null;
                match s(&op[0]) {
                    "union" => collection.union_clipping_regions = op[1].as_bool().unwrap(),
                    "enabled" => collection.enabled = op[1].as_bool().unwrap(),
                    "distance" => collection.get(u(&op[1]) as usize).unwrap().borrow_mut().set_distance(n(&op[2])),
                    "add" => collection.add(plane(&op[1])),
                    "remove" => returned = json!(collection.remove(&plane(&op[1]))),
                    "removeAt" => {
                        let p = collection.get(u(&op[1]) as usize).unwrap().clone();
                        returned = json!(collection.remove(&p));
                    }
                    "clear" => collection.remove_all(),
                    _ => unreachable!(),
                }
                let intersections = [
                    [-10., 0., 0., 1.],
                    [0., 0., 0., 1.],
                    [10., 0., 0., 1.],
                    [0., -10., 0., 1.],
                    [0., 10., 0., 1.],
                    [0., 0., 10., 5.],
                ]
                .map(|v| {
                    collection
                        .compute_intersection(
                            &BoundingSphere::new(Cartesian3::new(v[0], v[1], v[2]), v[3]),
                            Some(&transform),
                        )
                        .unwrap() as i32
                });
                let planes = (0..collection.len())
                    .map(|i| {
                        let p = collection.get(i).unwrap().borrow();
                        let mut result = values(&p);
                        result.push(p.index as f64);
                        result
                    })
                    .collect::<Vec<_>>();
                result.push(json!({"returned":returned,"state":collection.state(),"enabled":collection.enabled,"planes":planes,"intersections":intersections,"events":*events.lock().unwrap()}));
            }
            Some(Out::Ok(json!(result)))
        },
    );
}
#[test]
fn clipping_collection_has_one_owner_and_replacement_destroys_it() {
    let document = json!({"asset":{"version":"1.1"},"geometricError":0,"root":{"boundingVolume":{"sphere":[0,0,0,1]},"geometricError":0}});
    let make = || {
        Cesium3DTileset::from_json(
            Resource::new("https://example.com/tileset.json").unwrap(),
            &document,
            TilesetOptions::default(),
            None,
        )
        .unwrap()
    };
    let mut first = make();
    let mut second = make();
    let planes = Rc::new(RefCell::new(ClippingPlaneCollection::new([ClippingPlane::handle(Cartesian3::UNIT_X, 0.)])));
    first.set_clipping_planes(Some(planes.clone())).unwrap();
    first.set_clipping_planes(Some(planes.clone())).unwrap();
    assert!(!planes.borrow().is_destroyed());
    assert!(second.set_clipping_planes(Some(planes.clone())).is_err());
    assert!(second.clipping_planes().is_none());
    first.set_clipping_planes(None).unwrap();
    assert!(planes.borrow().is_destroyed());
    assert!(first.clipping_planes().is_none());
}
#[derive(Clone)]
struct PrepassHost {
    uploads: Rc<std::cell::Cell<usize>>,
    draws: Rc<std::cell::Cell<usize>>,
}
impl SceneBackend for PrepassHost {
    fn dimensions(&self) -> Result<[u32; 2], RuntimeError> {
        Ok([8, 8])
    }
    fn update_clipping_planes(&mut self, p: &mut ClippingPlaneCollection) -> Result<(), RuntimeError> {
        assert_eq!(p.len(), 1);
        self.uploads.set(self.uploads.get() + 1);
        Ok(())
    }
    fn begin_frame(&mut self, _: &FrameState) -> Result<(), RuntimeError> {
        self.draws.set(self.draws.get() + 1);
        Ok(())
    }
    fn clear(&mut self, _: Color, _: Option<f64>, _: Option<i32>) -> Result<(), RuntimeError> {
        Ok(())
    }
    fn set_frustum(&mut self, _: &FrameState, _: f64, _: f64) -> Result<(), RuntimeError> {
        Ok(())
    }
    fn execute(&mut self, c: &SceneCommand, s: SceneSubmission) -> Result<(), RuntimeError> {
        (c.execute)(s)
    }
    fn end_frame(&mut self) -> Result<(), RuntimeError> {
        Ok(())
    }
    fn destroy(&mut self) -> Result<(), RuntimeError> {
        Ok(())
    }
}
#[test]
fn clipping_uploads_run_for_hidden_tilesets_when_request_render_mode_skips_a_draw() {
    let document = json!({"asset":{"version":"1.1"},"geometricError":0,"root":{"boundingVolume":{"sphere":[0,0,0,1]},"geometricError":0}});
    let mut t = Cesium3DTileset::from_json(
        Resource::new("https://example.com/tileset.json").unwrap(),
        &document,
        TilesetOptions { show: false, ..Default::default() },
        None,
    )
    .unwrap();
    let planes = Rc::new(RefCell::new(ClippingPlaneCollection::new([ClippingPlane::handle(Cartesian3::UNIT_X, 0.)])));
    t.set_clipping_planes(Some(planes.clone())).unwrap();
    let host = PrepassHost { uploads: Rc::new(std::cell::Cell::new(0)), draws: Rc::new(std::cell::Cell::new(0)) };
    let time = JulianDate::new(2451545., 0., TimeStandard::Tai);
    let mut scene = Scene::new(
        host.clone(),
        CameraOptions::new(8., 8.),
        SceneOptions { request_render_mode: true, maximum_render_time_change: None, ..Default::default() },
        time,
    )
    .unwrap();
    scene.primitives.push(Box::new(t));
    assert!(scene.render(time).unwrap());
    planes.borrow().get(0).unwrap().borrow_mut().set_distance(2.);
    assert!(!scene.render(time).unwrap());
    assert_eq!(host.uploads.get(), 2);
    assert_eq!(host.draws.get(), 1);
    scene.destroy().unwrap();
    assert!(planes.borrow().is_destroyed());
}
