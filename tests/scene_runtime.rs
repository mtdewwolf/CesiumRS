//! Frame orchestration and tileset state checked against pinned CesiumJS.
mod common;
use cesium_rs::{
    core::*,
    renderer::{Pass, UniformFrame},
    scene::*,
};
use common::*;
use serde_json::{json, Value};
use std::sync::{Arc, Mutex};
use std::{cell::Cell, rc::Rc};

type Trace = Arc<Mutex<Vec<String>>>;
#[derive(Clone)]
struct Host {
    trace: Trace,
    failure: Arc<Mutex<Option<String>>>,
}
impl Host {
    fn step(&self, name: &str) -> Result<(), RuntimeError> {
        self.trace.lock().unwrap().push(name.into());
        if self.failure.lock().unwrap().as_deref() == Some(name) {
            return Err(RuntimeError::new("injected"));
        }
        Ok(())
    }
}
impl SceneBackend for Host {
    fn capabilities(&self) -> Result<cesium_rs::renderer::ContextCapabilities, RuntimeError> {
        Ok(cesium_rs::renderer::ContextCapabilities {
            webgl2: true,
            fragment_depth: true,
            depth_texture: true,
            color_buffer_float: true,
            ..Default::default()
        })
    }
    fn dimensions(&self) -> Result<[u32; 2], RuntimeError> {
        Ok([800, 600])
    }
    fn begin_frame(&mut self, _: &FrameState) -> Result<(), RuntimeError> {
        self.step("begin")
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
        self.step("end")
    }
    fn destroy(&mut self) -> Result<(), RuntimeError> {
        Ok(())
    }
}
impl ScenePrimitive for Host {
    fn pre_passes_update(&mut self, _: &mut FrameState) -> Result<(), RuntimeError> {
        self.step("primitivePre")
    }
    fn update(&mut self, _: &mut FrameState) -> Result<(), RuntimeError> {
        self.step("primitiveUpdate")
    }
    fn post_passes_update(&mut self, _: &mut FrameState) -> Result<(), RuntimeError> {
        self.step("primitivePost")
    }
    fn update_for_pass(&mut self, _: &mut FrameState, p: Cesium3DTilePass) -> Result<(), RuntimeError> {
        self.step(&format!("pass:{}", p.value()))
    }
    fn destroy(&mut self) -> Result<(), RuntimeError> {
        Ok(())
    }
}
impl SceneComponent for Host {
    fn stage(&mut self, s: SceneStage, _: &mut FrameState) -> Result<(), RuntimeError> {
        let label = match s {
            SceneStage::ResetBudgets => "reset",
            SceneStage::PrePasses => "prePasses",
            SceneStage::MostDetailedPicks => "mostDetailed",
            SceneStage::CreditBegin => "creditBegin",
            SceneStage::CreditEnd => "creditEnd",
            SceneStage::Environment => "environment",
            SceneStage::Resolve => "resolve",
            _ => return Ok(()),
        };
        self.step(label)
    }
    fn destroy(&mut self) -> Result<(), RuntimeError> {
        Ok(())
    }
}
fn host() -> Host {
    Host { trace: Default::default(), failure: Default::default() }
}
fn epoch() -> JulianDate {
    JulianDate::new(2451545., 0., TimeStandard::Tai)
}
fn render_scenario(o: &Value, ops: &Value) -> Value {
    let h = host();
    let options = SceneOptions {
        request_render_mode: o["requestRenderMode"].as_bool().unwrap_or(false),
        maximum_render_time_change: o["maximumRenderTimeChange"].as_f64(),
        ..Default::default()
    };
    let mut scene = Scene::new(h.clone(), CameraOptions::new(800., 600.), options, epoch()).unwrap();
    scene.rethrow_render_errors = o["rethrow"].as_bool().unwrap_or(false);
    scene.frame_state.frame_number = o["frameNumber"].as_u64().unwrap_or(0) as u32;
    scene.primitives.push(Box::new(h.clone()));
    scene.components.push(Box::new(h.clone()));
    let trace = Arc::clone(&h.trace);
    scene.events.add_event_listener(move |event| {
        trace.lock().unwrap().push(
            match event.kind {
                SceneEventKind::PreUpdate => "preUpdate",
                SceneEventKind::PostUpdate => "postUpdate",
                SceneEventKind::PreRender => "preRender",
                SceneEventKind::PostRender => "postRender",
                SceneEventKind::RenderError => "renderError",
            }
            .into(),
        )
    });
    let mut results = vec![];
    for op in ops.as_array().unwrap() {
        match s(&op[0]) {
            "request" => scene.request_render(),
            "camera" => {
                if let CameraFrustum::Perspective(f) = &mut scene.camera.frustum {
                    f.fov += 0.01;
                }
            }
            "hdr" => scene.set_high_dynamic_range(!scene.options.high_dynamic_range),
            "log" => scene.set_logarithmic_depth_buffer(!scene.options.logarithmic_depth_buffer),
            "show" => scene.primitives_show = op[1].as_bool().unwrap(),
            "fail" => *h.failure.lock().unwrap() = op[1].as_str().map(str::to_owned),
            "after" => {
                let trace = Arc::clone(&h.trace);
                let request = op[1].as_bool().unwrap();
                scene.frame_state.after_render.push(Box::new(move || {
                    trace.lock().unwrap().push("after".into());
                    Ok(request)
                }));
            }
            "render" => {
                h.trace.lock().unwrap().clear();
                let error = scene.render(epoch().add_seconds(n(&op[1]))).is_err();
                results.push(json!({"trace":*h.trace.lock().unwrap(),"error":error,"frame":scene.frame_state.frame_number,"newFrame":scene.frame_state.new_frame,"requested":scene.render_requested(),"time":JulianDate::seconds_difference(&scene.frame_state.time,&epoch()),"last":scene.last_render_time().map(|t|JulianDate::seconds_difference(&t,&epoch()))}));
            }
            _ => panic!("unknown operation"),
        }
    }
    json!(results)
}
#[test]
fn scene_render_reference() {
    run(
        "scene",
        "Scene",
        |_| TIGHT,
        |f, a| {
            Some(match f {
                "render" => Out::Ok(render_scenario(&a[0], &a[1])),
                _ => return None,
            })
        },
    );
}

#[test]
fn atmosphere_reference() {
    run(
        "scene",
        "Atmosphere",
        |_| TIGHT,
        |_, a| {
            let shifts = &a[0];
            let atmosphere = Atmosphere {
                hue_shift: n(&shifts[0]),
                saturation_shift: n(&shifts[1]),
                brightness_shift: n(&shifts[2]),
                ..Default::default()
            };
            Some(Out::Ok(
                json!({"light":atmosphere.light_intensity,"rayleigh":atmosphere.rayleigh_coefficient.enc(),"mie":atmosphere.mie_coefficient.enc(),"rayleighHeight":atmosphere.rayleigh_scale_height,"mieHeight":atmosphere.mie_scale_height,"anisotropy":atmosphere.mie_anisotropy,"dynamic":atmosphere.dynamic_lighting.value(),"correct":atmosphere.requires_color_correct()}),
            ))
        },
    );
}
#[test]
fn fog_reference() {
    run(
        "scene",
        "Fog",
        |_| TIGHT,
        |_, a| {
            let o = &a[0];
            let mut fog = Fog::default();
            if let Some(v) = o["enabled"].as_bool() {
                fog.enabled = v;
            }
            if let Some(v) = o["renderable"].as_bool() {
                fog.renderable = v;
            }
            for (key, field) in [
                ("density", &mut fog.density),
                ("visualDensityScalar", &mut fog.visual_density_scalar),
                ("screenSpaceErrorFactor", &mut fog.screen_space_error_factor),
                ("minimumBrightness", &mut fog.minimum_brightness),
            ] {
                if let Some(v) = o[key].as_f64() {
                    *field = v;
                }
            }
            if let Some(v) = o["heightFalloff"].as_f64() {
                fog.set_height_falloff(v).unwrap();
            }
            let mut state = FrameFog::default();
            let results:Vec<_>=a[1].as_array().unwrap().iter().map(|f|{
        fog.update(&mut state,SceneMode::from_value(u(&f[0]) as i32).unwrap(),f[1].as_f64(),c3(&f[2]),c3(&f[3]));
        json!({"enabled":state.enabled,"renderable":state.renderable,"density":num(state.density),"visualDensityScalar":state.visual_density_scalar,"sse":state.sse,"minimumBrightness":state.minimum_brightness})}).collect();
            Some(Out::Ok(json!(results)))
        },
    );
}

fn tileset_snapshot(a: &[Value]) -> Result<Value, RuntimeError> {
    let resource =
        Resource::new(a.get(2).and_then(Value::as_str).unwrap_or("https://example.com/tiles/tileset.json?key=test"))
            .unwrap();
    let mut tileset = Cesium3DTileset::from_json(resource, &a[0], serde_json::from_value(a[1].clone()).unwrap(), None)?;
    if let Some(matrix) = a.get(3) {
        tileset.model_matrix = m4(matrix);
    }
    let sphere = tileset.bounding_sphere()?.unwrap();
    let origin = tileset.clipping_planes_origin_matrix()?;
    let mut queue = std::collections::VecDeque::from([tileset.root.unwrap()]);
    let mut tiles = vec![];
    while let Some(id) = queue.pop_front() {
        let t = &tileset.tiles[id];
        queue.extend(&t.children);
        tiles.push(json!({"depth":t.depth,"refine":t.refine.value(),"error":num(t.geometric_error*t.computed_transform.get_maximum_scale()),"transform":t.computed_transform.enc(),"sphere":t.bounding_sphere.enc(),"uri":t.content_resource.as_ref().map(Resource::url),"empty":t.has_empty_content()}));
    }
    Ok(
        json!({"url":tileset.url,"basePath":tileset.base_path,"asset":tileset.asset,"extras":tileset.extras,"properties":tileset.properties,"sphere":sphere.enc(),"origin":origin.enc(),"tiles":tiles,"total":tileset.statistics.number_of_tiles_total,"allAdditive":tileset.all_tiles_additive,"skip":tileset.is_skipping_level_of_detail(),"up":tileset.model_up_axis.value(),"forward":tileset.model_forward_axis.value(),"credits":tileset.credits().iter().map(|c|json!([c.html(),c.show_on_screen])).collect::<Vec<_>>(),"geometricError":num(tileset.geometric_error),"maximumScreenSpaceError":tileset.options.maximum_screen_space_error,"memoryAdjusted":tileset.memory_adjusted_screen_space_error(),"progressive":tileset.options.progressive_resolution_height_fraction}),
    )
}
#[test]
fn tileset_json_reference() {
    run(
        "scene",
        "Cesium3DTileset",
        |_| Tol { rel: 1e-11, abs: 1e-7, vector: true },
        |f, a| {
            Some(
                match match f {
                    "load" => tileset_snapshot(a),
                    "lifecycle" => tileset_lifecycle(a),
                    "traversal" => tileset_traversal(a),
                    _ => return None,
                } {
                    Ok(v) => Out::Ok(v),
                    Err(e) => Out::Err(e.to_string()),
                },
            )
        },
    );
}

fn frame() -> FrameState {
    let mut camera = Camera::new(CameraOptions::new(800., 600.));
    let uniform = camera.uniform_camera();
    let volume = camera.frustum.compute_culling_volume(uniform.position, uniform.direction, uniform.up);
    FrameState::new(UniformFrame::new(uniform, epoch()), volume, [800, 600])
}

struct Content {
    ready: Rc<Cell<bool>>,
    trace: Trace,
}
impl TilesetContent for Content {
    fn metrics(&self) -> TileContentMetrics {
        TileContentMetrics {
            features: 2,
            points: 3,
            triangles: 4,
            geometry_bytes: 100,
            texture_bytes: 20,
            batch_table_bytes: 5,
        }
    }
    fn process(&mut self, _: &FrameState) -> Result<bool, RuntimeError> {
        Ok(self.ready.get())
    }
    fn update(&mut self, _: &mut FrameState) -> Result<(), RuntimeError> {
        Ok(())
    }
    fn pick(&self, _: &Ray, _: &FrameState) -> Result<Option<Cartesian3>, RuntimeError> {
        Ok(None)
    }
    fn destroy(&mut self) -> Result<(), RuntimeError> {
        self.trace.lock().unwrap().push("destroy".into());
        Ok(())
    }
}
fn simple_tileset(options: TilesetOptions, loader: Option<Rc<dyn TilesetContentLoader>>) -> Cesium3DTileset {
    Cesium3DTileset::from_json(Resource::new("https://example.com/tileset.json").unwrap(),&json!({"asset":{"version":"1.1"},"geometricError":100,"root":{"boundingVolume":{"sphere":[0,0,0,10]},"geometricError":0,"content":{"uri":"model.glb"}}}),options,loader).unwrap()
}
fn tileset_lifecycle(a: &[Value]) -> Result<Value, RuntimeError> {
    let mut t = simple_tileset(serde_json::from_value(a[0].clone()).unwrap(), None);
    t.options.dynamic_screen_space_error = false;
    let trace = Arc::new(Mutex::new(Vec::<Value>::new()));
    let events = Arc::clone(&trace);
    t.events.borrow_mut().add_event_listener(move |event| {
        events.lock().unwrap().push(match event {
            TilesetEvent::TileLoad(_) => json!("tileLoad"),
            TilesetEvent::TileUnload(_) => json!("tileUnload"),
            TilesetEvent::AllTilesLoaded => json!("allTilesLoaded"),
            TilesetEvent::InitialTilesLoaded => json!("initialTilesLoaded"),
            TilesetEvent::LoadProgress { pending, processing } => json!(["progress", pending, processing]),
            _ => json!("failed"),
        })
    });
    let destroy_trace: Trace = Default::default();
    let ready = Rc::new(Cell::new(false));
    let mut frame = frame();
    let mut results = vec![];
    for op in a[1].as_array().unwrap() {
        match s(&op[0]) {
            "install" => {
                t.install_content(0, Box::new(Content { ready: Rc::clone(&ready), trace: Arc::clone(&destroy_trace) }))?
            }
            "ready" => ready.set(op[1].as_bool().unwrap()),
            "pre" => {
                frame.new_frame = op[1].as_bool().unwrap();
                frame.frame_number += 1;
                frame.time = frame.time.add_seconds(1.);
                t.pre_passes_update(&mut frame)?;
            }
            "post" => {
                t.post_passes_update(&mut frame)?;
                for message in destroy_trace.lock().unwrap().drain(..) {
                    trace.lock().unwrap().push(json!(message));
                }
            }
            "touch" => t.cache.touch(0),
            "trim" => t.trim_loaded_tiles(),
            "after" => {
                let mut requested = vec![];
                for callback in std::mem::take(&mut frame.after_render) {
                    requested.push(callback()?);
                }
                trace.lock().unwrap().push(json!(["callbacks", requested]));
            }
            "snapshot" => {
                results.push(json!({"trace":*trace.lock().unwrap(),"state":t.tiles[0].state.value(),"ready":t.statistics.number_of_tiles_with_content_ready,"processing":t.statistics.number_of_tiles_processing,"loaded":t.statistics.number_of_loaded_tiles_total,"memory":t.statistics.total_memory_usage_in_bytes(),"tilesLoaded":t.tiles_loaded(),"time":t.time_since_load,"adjusted":t.memory_adjusted_screen_space_error()}));
                trace.lock().unwrap().clear();
            }
            _ => panic!("unknown operation"),
        }
    }
    Ok(json!(results))
}
fn tileset_traversal(a: &[Value]) -> Result<Value, RuntimeError> {
    let leaf = |x: f64, id: u8| json!({"boundingVolume":{"sphere":[6378137.+x,0,0,5]},"geometricError":0,"content":{"uri":format!("{id}.glb")}});
    let mut root = leaf(0., 0);
    root["geometricError"] = json!(100);
    root["refine"] = a[0]["refine"].clone();
    root["children"] = json!([leaf(-10., 1), leaf(10., 2)]);
    let mut t = Cesium3DTileset::from_json(
        Resource::new("https://example.com/tileset.json").unwrap(),
        &json!({"asset":{"version":"1.1"},"geometricError":1000,"root":root}),
        TilesetOptions {
            dynamic_screen_space_error: false,
            foveated_screen_space_error: false,
            cull_with_children_bounds: false,
            cull_requests_while_moving: false,
            ..Default::default()
        },
        None,
    )?;
    for id in a[1].as_array().unwrap() {
        t.install_content(
            u(id) as usize,
            Box::new(Content { ready: Rc::new(Cell::new(true)), trace: Default::default() }),
        )?;
    }
    let mut camera = Camera::new(CameraOptions::new(800., 600.));
    camera.position = Cartesian3::new(6379137., 0., 0.);
    camera.direction = Cartesian3::new(-1., 0., 0.);
    camera.up = Cartesian3::UNIT_Z;
    camera.right = Cartesian3::UNIT_Y;
    let snapshot = camera.uniform_camera();
    let volume = camera.frustum.compute_culling_volume(snapshot.position, snapshot.direction, snapshot.up);
    let mut f = FrameState::new(UniformFrame::new(snapshot, epoch()), volume, [800, 600]);
    f.frame_number = 1;
    f.new_frame = true;
    t.pre_passes_update(&mut f)?;
    t.update_for_pass(&mut f, &mut TilesetPassState::new(Cesium3DTilePass::from_value(u(&a[2]) as i32).unwrap()))?;
    let mut requested = t.requested_tiles.clone();
    requested.sort_unstable();
    Ok(json!({"selected":t.selected_tiles,"requested":requested,"visited":t.statistics.visited}))
}

type ContentSender = futures_channel::oneshot::Sender<Result<Box<dyn TilesetContent>, RuntimeError>>;
#[derive(Default)]
struct Loader {
    senders: std::cell::RefCell<Vec<ContentSender>>,
    requests: std::cell::RefCell<Vec<TileContentRequest>>,
}
impl TilesetContentLoader for Loader {
    fn request(&self, request: TileContentRequest) -> Option<TileContentFuture> {
        self.requests.borrow_mut().push(request);
        let (send, receive) = futures_channel::oneshot::channel();
        self.senders.borrow_mut().push(send);
        Some(Box::pin(async move { receive.await.map_err(|_| RuntimeError::new("request dropped"))? }))
    }
}
fn unrestricted_frame() -> FrameState {
    let mut f = frame();
    f.culling_volume = CullingVolume::default();
    f.uniform.camera.position = Cartesian3::new(100., 0., 0.);
    f.frame_number = 1;
    f.new_frame = true;
    f
}
#[test]
fn asynchronous_content_wakes_scene_and_obsolete_requests_are_cancelled() {
    let loader = Rc::new(Loader::default());
    let mut t = simple_tileset(TilesetOptions::default(), Some(loader.clone()));
    let mut scene = Scene::new(
        host(),
        CameraOptions::new(800., 600.),
        SceneOptions { request_render_mode: true, maximum_render_time_change: None, ..Default::default() },
        epoch(),
    )
    .unwrap();
    scene.render(epoch()).unwrap();
    assert!(!scene.render_requested());
    let mut f = unrestricted_frame();
    f.render_request = Some(scene.request_handle());
    t.pre_passes_update(&mut f).unwrap();
    t.update_for_pass(&mut f, &mut TilesetPassState::new(Cesium3DTilePass::Render)).unwrap();
    assert_eq!(t.statistics.number_of_pending_requests, 1);
    assert_eq!(loader.requests.borrow()[0].resource.url(), "https://example.com/model.glb");
    t.pre_passes_update(&mut f).unwrap(); // register the future's Scene waker
    let ready = Rc::new(Cell::new(true));
    let destroyed: Trace = Default::default();
    assert!(loader
        .senders
        .borrow_mut()
        .remove(0)
        .send(Ok(Box::new(Content { ready, trace: destroyed.clone() })))
        .is_ok());
    assert!(scene.render_requested());
    t.pre_passes_update(&mut f).unwrap();
    assert_eq!(t.statistics.number_of_pending_requests, 0);
    assert_eq!(t.statistics.number_of_tiles_with_content_ready, 1);
    assert_eq!(t.statistics.total_memory_usage_in_bytes(), 125);
    assert!(t
        .install_content(0, Box::new(Content { ready: Rc::new(Cell::new(true)), trace: destroyed.clone() }))
        .is_err());
    t.unload_tile(0).unwrap();
    assert_eq!(&*destroyed.lock().unwrap(), &["destroy"]);
    f.frame_number += 1;
    t.update_for_pass(&mut f, &mut TilesetPassState::new(Cesium3DTilePass::Render)).unwrap();
    let cancelled = loader.requests.borrow()[1].cancelled.clone();
    assert!(!cancelled.get());
    f.frame_number += 1;
    t.post_passes_update(&mut f).unwrap();
    assert!(cancelled.get());
    assert_eq!(t.tiles[0].state, Cesium3DTileContentState::Unloaded);
    assert_eq!(t.statistics.number_of_pending_requests, 0);
    assert!(loader.senders.borrow_mut().remove(0).send(Err(RuntimeError::new("cancelled"))).is_err());
}
#[test]
fn destroy_cancels_requests_and_suppresses_deferred_events() {
    let loader = Rc::new(Loader::default());
    let mut t = simple_tileset(TilesetOptions::default(), Some(loader.clone()));
    let mut f = unrestricted_frame();
    let count = Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let listener = Arc::clone(&count);
    t.events.borrow_mut().add_event_listener(move |_| {
        listener.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    });
    t.update_for_pass(&mut f, &mut TilesetPassState::new(Cesium3DTilePass::Render)).unwrap();
    t.post_passes_update(&mut f).unwrap();
    assert_eq!(f.after_render.len(), 1);
    t.destroy().unwrap();
    t.destroy().unwrap();
    assert!(loader.requests.borrow()[0].cancelled.get());
    for callback in std::mem::take(&mut f.after_render) {
        callback().unwrap();
    }
    assert_eq!(count.load(std::sync::atomic::Ordering::Relaxed), 0);
    assert!(t.pre_passes_update(&mut f).is_err());
}

struct Commands(Vec<SceneCommand>);
impl ScenePrimitive for Commands {
    fn pre_passes_update(&mut self, _: &mut FrameState) -> Result<(), RuntimeError> {
        Ok(())
    }
    fn post_passes_update(&mut self, _: &mut FrameState) -> Result<(), RuntimeError> {
        Ok(())
    }
    fn update_for_pass(&mut self, _: &mut FrameState, _: Cesium3DTilePass) -> Result<(), RuntimeError> {
        Ok(())
    }
    fn update(&mut self, f: &mut FrameState) -> Result<(), RuntimeError> {
        f.command_list.extend(self.0.clone());
        Ok(())
    }
    fn destroy(&mut self) -> Result<(), RuntimeError> {
        Ok(())
    }
}
#[test]
fn scene_orders_translucency_and_submits_closest_frustum_once() {
    let mut scene = Scene::new(
        host(),
        CameraOptions::new(800., 600.),
        SceneOptions { logarithmic_depth_buffer: false, far_to_near_ratio: 10., ..Default::default() },
        epoch(),
    )
    .unwrap();
    scene.camera.position = Cartesian3::ZERO;
    scene.camera.direction = Cartesian3::UNIT_X;
    scene.camera.up = Cartesian3::UNIT_Z;
    scene.camera.right = Cartesian3::UNIT_Y;
    if let CameraFrustum::Perspective(f) = &mut scene.camera.frustum {
        f.near = 1.;
        f.far = 100.;
    }
    let trace: Trace = Default::default();
    let commands = [
        ("near", 10., 1., Pass::Translucent, false),
        ("large", 20., 19., Pass::Translucent, false),
        ("closest", 10., 2., Pass::Opaque, true),
        ("overlay", 0., 0., Pass::Overlay, false),
        ("compute", 0., 0., Pass::Compute, false),
    ]
    .into_iter()
    .map(|(name, x, r, pass, closest)| {
        let trace = trace.clone();
        let mut c = SceneCommand::new(
            name,
            pass,
            Rc::new(move |s| {
                trace.lock().unwrap().push(format!("{name}:{}", s.frustum));
                Ok(())
            }),
        );
        c.metadata.cull = false;
        c.metadata.bounding_volume = Some(BoundingSphere::new(Cartesian3::new(x, 0., 0.), r));
        c.metadata.execute_in_closest_frustum = closest;
        c
    })
    .collect();
    scene.primitives.push(Box::new(Commands(commands)));
    scene.render(epoch()).unwrap();
    let log = trace.lock().unwrap();
    assert_eq!(log.iter().filter(|s| s.starts_with("closest")).count(), 1);
    assert_eq!(log.first().unwrap(), &format!("compute:{}", usize::MAX));
    assert_eq!(log.last().unwrap(), &format!("overlay:{}", usize::MAX));
    let translucent: Vec<_> = log
        .iter()
        .filter(|s| s.starts_with("near:") || s.starts_with("large:"))
        .map(|s| s.split(':').next().unwrap())
        .collect();
    assert_eq!(translucent, vec!["near", "large", "near", "large"]);
}

#[derive(Clone, Default)]
struct Gpu {
    next: Rc<Cell<u32>>,
    calls: Trace,
}
impl cesium_rs::renderer::Gl for Gpu {
    type Object = u32;
    fn drawing_buffer_size(&self) -> Result<[u32; 2], RuntimeError> {
        Ok([800, 600])
    }
    fn invoke(
        &self,
        name: &str,
        args: &[cesium_rs::renderer::GlValue<u32>],
    ) -> Result<cesium_rs::renderer::GlValue<u32>, RuntimeError> {
        use cesium_rs::renderer::GlValue;
        self.calls.lock().unwrap().push(name.into());
        if name.starts_with("create") {
            let id = self.next.get() + 1;
            self.next.set(id);
            return Ok(GlValue::Object(id));
        }
        Ok(match name {
            "getParameter" if matches!(args.first(),Some(GlValue::Number(p)) if [cesium_rs::core::webgl_constants::ALIASED_LINE_WIDTH_RANGE,cesium_rs::core::webgl_constants::ALIASED_POINT_SIZE_RANGE,cesium_rs::core::webgl_constants::MAX_VIEWPORT_DIMS].contains(&(*p as u32))) => {
                GlValue::Numbers(vec![1., 16384.])
            }
            "getParameter" => GlValue::Number(16384.),
            "getExtension" => GlValue::Object(0),
            "getShaderParameter" | "getProgramParameter" => GlValue::Bool(true),
            "checkFramebufferStatus" => GlValue::Number(cesium_rs::core::webgl_constants::FRAMEBUFFER_COMPLETE as f64),
            _ => GlValue::Null,
        })
    }
}
#[test]
fn scene_executes_owned_draw_resources_through_renderer_backend() {
    use cesium_rs::renderer::{Context, DrawMetadata, RenderState, ShaderProgram, ViewportQuad};
    let gpu = Gpu::default();
    let context = Context::new_webgl2(gpu.clone());
    let quad = ViewportQuad::new(&context).unwrap();
    let program = Rc::new(
        ShaderProgram::from_sources(
            &context,
            &cesium_rs::renderer::ShaderProgramOptions {
                vertex_shader_source: cesium_rs::renderer::ShaderSource::new("void main(){gl_Position=vec4(0.0);}"),
                fragment_shader_source: cesium_rs::renderer::ShaderSource::new("void main(){out_FragColor=vec4(1.0);}"),
                attribute_locations: Default::default(),
            },
        )
        .unwrap(),
    );
    let command = SceneCommand::draw(
        "quad",
        context.clone(),
        quad.vertex_array().clone(),
        program,
        Pass::Opaque,
        RenderState::default(),
        DrawMetadata { execute_in_closest_frustum: true, ..Default::default() },
    );
    let mut scene = Scene::new(
        RendererSceneBackend::new(context.clone()),
        CameraOptions::new(800., 600.),
        SceneOptions::default(),
        epoch(),
    )
    .unwrap();
    scene.rethrow_render_errors = true;
    scene.primitives.push(Box::new(Commands(vec![command])));
    scene.render(epoch()).unwrap();
    let draw_count = gpu.calls.lock().unwrap().iter().filter(|s| s.as_str() == "drawElements").count();
    // The owned primitive draw is accompanied by View depth-copy and resolve
    // draws, which are internal renderer work rather than Scene commands.
    assert!(draw_count > 1);
    assert_eq!(scene.frame_state.executed_commands.len(), 1);
    assert_eq!(context.uniform_state().entire_frustum, [0.1, 1e10]);
    scene.destroy().unwrap();
    assert!(context.is_destroyed());
    assert!(scene.render(epoch()).is_err());
}

struct FlightProbe(Rc<std::cell::RefCell<Vec<Cartesian3>>>);
impl ScenePrimitive for FlightProbe {
    fn pre_passes_update(&mut self, _: &mut FrameState) -> Result<(), RuntimeError> {
        Ok(())
    }
    fn update(&mut self, _: &mut FrameState) -> Result<(), RuntimeError> {
        Ok(())
    }
    fn post_passes_update(&mut self, _: &mut FrameState) -> Result<(), RuntimeError> {
        Ok(())
    }
    fn update_for_pass(&mut self, f: &mut FrameState, p: Cesium3DTilePass) -> Result<(), RuntimeError> {
        if p == Cesium3DTilePass::PreloadFlight {
            self.0.borrow_mut().push(f.uniform.camera.position);
            return Err(RuntimeError::new("preload failed"));
        }
        Ok(())
    }
    fn destroy(&mut self) -> Result<(), RuntimeError> {
        Ok(())
    }
}
#[test]
fn flight_preload_uses_destination_camera_and_restores_current_camera_on_error() {
    let mut scene = Scene::new(host(), CameraOptions::new(800., 600.), SceneOptions::default(), epoch()).unwrap();
    let positions = Rc::new(std::cell::RefCell::new(vec![]));
    scene.primitives.push(Box::new(FlightProbe(positions.clone())));
    scene.render(epoch()).unwrap();
    assert!(positions.borrow().is_empty());
    let destination = Cartesian3::from_degrees(20., 40., 1000., &Ellipsoid::WGS84);
    let mut flight = CameraFlightOptions::new(CameraDestination::Cartesian(destination));
    flight.duration = Some(3.);
    scene.camera.fly_to(flight).unwrap();
    assert!(scene.camera.can_preload_flight());
    let current = scene.camera.uniform_camera().position;
    scene.render(epoch()).unwrap();
    assert_eq!(positions.borrow().len(), 1);
    assert!(positions.borrow()[0].distance(destination) < 1e-5);
    assert_eq!(scene.frame_state.uniform.camera.position, current);
    let mut f = unrestricted_frame();
    let mut tileset = simple_tileset(TilesetOptions::default(), None);
    let saved = f.uniform.camera.position;
    let saved_volume = f.culling_volume.clone();
    let mut state = TilesetPassState::new(Cesium3DTilePass::MostDetailedPick);
    let mut camera = f.uniform.camera.clone();
    camera.position = destination;
    state.camera = Some(camera);
    state.culling_volume = Some(CullingVolume::default());
    tileset.update_for_pass(&mut f, &mut state).unwrap();
    assert_eq!(f.uniform.camera.position, saved);
    assert_eq!(f.culling_volume, saved_volume);
}

struct JsonTransport {
    requests: std::cell::RefCell<Vec<String>>,
}
impl cesium_rs::core::resource::ResourceTransport for JsonTransport {
    fn execute(
        &self,
        r: cesium_rs::core::resource::HttpRequest,
    ) -> RequestFuture<cesium_rs::core::resource::HttpResponse> {
        self.requests.borrow_mut().push(r.url.clone());
        let value = if r.url.contains("schema.json") {
            json!({"id":"external-schema","classes":{}})
        } else {
            json!({"asset":{"version":"1.1"},"geometricError":0,"schemaUri":"../metadata/schema.json","root":{"boundingVolume":{"sphere":[0,0,0,10]},"geometricError":0}})
        };
        Box::pin(std::future::ready(Ok(cesium_rs::core::resource::HttpResponse {
            status: 200,
            body: serde_json::to_vec(&value).unwrap(),
            headers: vec![],
        })))
    }
}
#[test]
fn tileset_fetches_relative_metadata_and_preserves_nested_resource_bases() {
    use std::{
        future::Future,
        task::{Context, Poll, Waker},
    };
    let transport = Rc::new(JsonTransport { requests: Default::default() });
    let client = ResourceClient::new(transport.clone());
    let mut future = std::pin::pin!(Cesium3DTileset::from_url(
        Resource::new("https://example.com/tiles/tileset.json?token=test").unwrap(),
        &client,
        TilesetOptions::default(),
        None
    ));
    let Poll::Ready(Ok(mut tileset)) = future.as_mut().poll(&mut Context::from_waker(Waker::noop())) else {
        panic!("ready transport stalled")
    };
    assert_eq!(tileset.metadata_extension.as_ref().unwrap().schema().id.as_deref(), Some("external-schema"));
    assert_eq!(
        &*transport.requests.borrow(),
        &["https://example.com/tiles/tileset.json?token=test", "https://example.com/metadata/schema.json?token=test"]
    );
    let nested = json!({"asset":{"version":"1.1","tilesetVersion":"nested-v2"},"root":{"boundingVolume":{"sphere":[0,0,0,5]},"geometricError":0,"content":{"uri":"models/child.glb"}}});
    let root = tileset
        .load_tileset(Resource::new("https://example.com/nested/other.json?key=ok").unwrap(), &nested, tileset.root)
        .unwrap();
    assert_eq!(tileset.tiles[root].depth, 1);
    assert_eq!(
        tileset.tiles[root].content_resource.as_ref().unwrap().url(),
        "https://example.com/nested/models/child.glb?v=nested-v2&key=ok"
    );
    assert_eq!(tileset.resource.url(), "https://example.com/tiles/tileset.json?token=test");
}

struct StructuralTransport {
    requests: std::cell::RefCell<Vec<String>>,
}
impl resource::ResourceTransport for StructuralTransport {
    fn execute(&self, r: resource::HttpRequest) -> RequestFuture<resource::HttpResponse> {
        self.requests.borrow_mut().push(r.url.clone());
        let value = if r.url.ends_with("main.json") {
            json!({"asset":{"version":"1.1"},"geometricError":100,"root":{"boundingVolume":{"sphere":[0,0,0,100]},"geometricError":0,"content":{"uri":"nested/tileset.json"}}})
        } else if r.url.ends_with("nested/tileset.json") {
            json!({"asset":{"version":"1.1"},"geometricError":100,"root":{"boundingVolume":{"box":[0,0,0,10,0,0,0,10,0,0,0,10]},"geometricError":0,"content":{"uri":"model/{level}/{x}/{y}.glb"},"implicitTiling":{"subdivisionScheme":"QUADTREE","subtreeLevels":1,"availableLevels":1,"subtrees":{"uri":"subtree/{level}/{x}/{y}.json"}}}})
        } else if r.url.ends_with("subtree/0/0/0.json") {
            json!({"tileAvailability":{"constant":1},"contentAvailability":{"constant":1},"childSubtreeAvailability":{"constant":0}})
        } else {
            json!({"asset":{"version":"2.0"}})
        };
        Box::pin(std::future::ready(Ok(resource::HttpResponse {
            status: 200,
            body: serde_json::to_vec(&value).unwrap(),
            headers: vec![],
        })))
    }
}
#[test]
fn default_loader_expands_external_and_implicit_resources_and_reports_missing_decoders() {
    use futures_util::FutureExt;
    let transport = Rc::new(StructuralTransport { requests: Default::default() });
    let client = ResourceClient::new(transport.clone());
    let options = TilesetOptions {
        foveated_screen_space_error: false,
        dynamic_screen_space_error: false,
        cull_requests_while_moving: false,
        ..Default::default()
    };
    let mut t =
        Cesium3DTileset::from_url(Resource::new("https://example.com/main.json").unwrap(), &client, options, None)
            .now_or_never()
            .unwrap()
            .unwrap();
    let errors = Arc::new(Mutex::new(Vec::new()));
    let events = errors.clone();
    t.events.borrow_mut().add_event_listener(move |e| {
        if let TilesetEvent::TileFailed { message, .. } = e {
            events.lock().unwrap().push(message.clone());
        }
    });
    let mut f = unrestricted_frame();
    for i in 1..=5 {
        f.frame_number = i;
        t.pre_passes_update(&mut f).unwrap();
        t.update_for_pass(&mut f, &mut TilesetPassState::new(Cesium3DTilePass::Render)).unwrap();
    }
    assert_eq!(
        &*transport.requests.borrow(),
        &[
            "https://example.com/main.json",
            "https://example.com/nested/tileset.json",
            "https://example.com/nested/subtree/0/0/0.json",
            "https://example.com/nested/model/0/0/0.glb"
        ]
    );
    assert_eq!(t.statistics.number_of_tiles_total, 3);
    assert_eq!(t.statistics.number_of_tiles_with_content_ready, 0);
    assert_eq!(t.total_memory_usage_in_bytes(), 0);
    assert_eq!(&*errors.lock().unwrap(), &["No renderable content factory for gltf"]);
}

struct SharedContent {
    trace: Trace,
}
impl TilesetContent for SharedContent {
    fn metrics(&self) -> TileContentMetrics {
        TileContentMetrics { features: 2, geometry_bytes: 50, texture_bytes: 110, ..Default::default() }
    }
    fn texture_allocations(&self) -> Vec<(Option<String>, u64)> {
        vec![(Some("texture-1".into()), 100), (None, 10)]
    }
    fn process(&mut self, _: &FrameState) -> Result<bool, RuntimeError> {
        Ok(true)
    }
    fn update(&mut self, _: &mut FrameState) -> Result<(), RuntimeError> {
        Ok(())
    }
    fn pick(&self, _: &Ray, _: &FrameState) -> Result<Option<Cartesian3>, RuntimeError> {
        Ok(None)
    }
    fn destroy(&mut self) -> Result<(), RuntimeError> {
        self.trace.lock().unwrap().push("destroy".into());
        Ok(())
    }
}
#[test]
fn multiple_requests_poll_concurrently_preserve_partial_success_and_count_shared_textures() {
    let loader = Rc::new(Loader::default());
    let trace: Trace = Default::default();
    let mut t=Cesium3DTileset::from_json(Resource::new("https://example.com/tileset.json").unwrap(),&json!({"asset":{"version":"1.1"},"geometricError":100,"root":{"boundingVolume":{"sphere":[0,0,0,10]},"geometricError":0,"contents":[{"uri":"a.glb"},{"uri":"b.glb"},{"uri":"c.glb"}]}}),TilesetOptions{foveated_screen_space_error:false,..Default::default()},Some(loader.clone())).unwrap();
    let errors = Arc::new(Mutex::new(Vec::new()));
    let events = errors.clone();
    t.events.borrow_mut().add_event_listener(move |e| {
        if let TilesetEvent::TileFailed { url, message, .. } = e {
            events.lock().unwrap().push((url.clone(), message.clone()));
        }
    });
    let mut f = unrestricted_frame();
    t.pre_passes_update(&mut f).unwrap();
    t.update_for_pass(&mut f, &mut TilesetPassState::new(Cesium3DTilePass::Render)).unwrap();
    assert_eq!(t.statistics.number_of_pending_requests, 3);
    // Resolve the last request before the first: join_all must poll every one.
    assert!(loader.senders.borrow_mut().remove(2).send(Ok(Box::new(SharedContent { trace: trace.clone() }))).is_ok());
    t.pre_passes_update(&mut f).unwrap();
    assert_eq!(t.statistics.number_of_pending_requests, 2);
    assert!(loader.senders.borrow_mut().remove(1).send(Err(RuntimeError::new("bad content"))).is_ok());
    assert!(loader.senders.borrow_mut().remove(0).send(Ok(Box::new(SharedContent { trace: trace.clone() }))).is_ok());
    t.pre_passes_update(&mut f).unwrap();
    assert_eq!(t.statistics.number_of_pending_requests, 0);
    assert_eq!(t.statistics.number_of_tiles_with_content_ready, 1);
    assert_eq!(t.statistics.geometry_byte_length, 100);
    assert_eq!(t.statistics.textures_byte_length, 120);
    assert_eq!(t.statistics.textures_reference_counter_by_id["texture-1"], 2);
    assert_eq!(&*errors.lock().unwrap(), &[(Some("https://example.com/b.glb".into()), "bad content".into())]);
    t.unload_tile(0).unwrap();
    assert_eq!(t.total_memory_usage_in_bytes(), 0);
    assert!(t.statistics.textures_reference_counter_by_id.is_empty());
    assert_eq!(trace.lock().unwrap().len(), 2);
}
#[test]
fn expired_content_remains_live_until_the_replacement_is_ready() {
    let loader = Rc::new(Loader::default());
    let old: Trace = Default::default();
    let new: Trace = Default::default();
    let ready = Rc::new(Cell::new(false));
    let mut t = simple_tileset(
        TilesetOptions { foveated_screen_space_error: false, ..Default::default() },
        Some(loader.clone()),
    );
    t.tiles[0].expire_duration = Some(1.);
    t.install_content(0, Box::new(Content { ready: Rc::new(Cell::new(true)), trace: old.clone() })).unwrap();
    let mut f = unrestricted_frame();
    t.pre_passes_update(&mut f).unwrap();
    assert_eq!(t.statistics.number_of_tiles_with_content_ready, 1);
    f.time = epoch().add_seconds(2.);
    f.frame_number = 2;
    t.pre_passes_update(&mut f).unwrap();
    t.update_for_pass(&mut f, &mut TilesetPassState::new(Cesium3DTilePass::Render)).unwrap();
    assert_eq!(t.tiles[0].state, Cesium3DTileContentState::Loading);
    assert!(t.tiles[0].expired_content.is_some());
    assert_eq!(t.total_memory_usage_in_bytes(), 0);
    assert!(old.lock().unwrap().is_empty());
    assert!(loader.requests.borrow()[0].resource.url().contains("expired="));
    assert_eq!(t.tiles[0].content_resource.as_ref().unwrap().url(), "https://example.com/model.glb");
    assert!(loader
        .senders
        .borrow_mut()
        .remove(0)
        .send(Ok(Box::new(Content { ready: ready.clone(), trace: new.clone() })))
        .is_ok());
    t.pre_passes_update(&mut f).unwrap();
    assert!(t.tiles[0].expire_date.is_none());
    assert_eq!(t.tiles[0].state, Cesium3DTileContentState::Processing);
    t.update_for_pass(&mut f, &mut TilesetPassState::new(Cesium3DTilePass::Render)).unwrap();
    assert!(old.lock().unwrap().is_empty());
    ready.set(true);
    t.pre_passes_update(&mut f).unwrap();
    t.update_for_pass(&mut f, &mut TilesetPassState::new(Cesium3DTilePass::Render)).unwrap();
    assert_eq!(&*old.lock().unwrap(), &["destroy"]);
    assert_eq!(t.statistics.number_of_tiles_with_content_ready, 1);
    assert_eq!(t.total_memory_usage_in_bytes(), 125);
    assert_eq!(t.tiles[0].expire_date, Some(f.time.add_seconds(1.)));
    t.destroy().unwrap();
    assert_eq!(&*new.lock().unwrap(), &["destroy"]);
}

struct SphereContent(BoundingSphere);
impl TilesetContent for SphereContent {
    fn metrics(&self) -> TileContentMetrics {
        Default::default()
    }
    fn process(&mut self, _: &FrameState) -> Result<bool, RuntimeError> {
        Ok(true)
    }
    fn update(&mut self, _: &mut FrameState) -> Result<(), RuntimeError> {
        Ok(())
    }
    fn pick(&self, ray: &Ray, _: &FrameState) -> Result<Option<Cartesian3>, RuntimeError> {
        Ok(intersection_tests::ray_sphere(ray, &self.0).map(|i| ray.get_point(i.start)))
    }
    fn destroy(&mut self) -> Result<(), RuntimeError> {
        Ok(())
    }
}
#[test]
fn cpu_height_queries_and_cancelled_height_callbacks() {
    let sphere = BoundingSphere::new(Cartesian3::new(6378137., 0., 0.), 10.);
    let mut t=Cesium3DTileset::from_json(Resource::new("https://example.com/tileset.json").unwrap(),&json!({"asset":{"version":"1.1"},"geometricError":1000,"root":{"boundingVolume":{"sphere":[6378137,0,0,10]},"geometricError":0,"content":{"uri":"a.glb"}}}),TilesetOptions{enable_collision:true,enable_pick:true,foveated_screen_space_error:false,..Default::default()},None).unwrap();
    let mut f = unrestricted_frame();
    f.uniform.camera.position = Cartesian3::new(6378237., 0., 0.);
    f.frame_number = 5;
    t.install_content(0, Box::new(SphereContent(sphere))).unwrap();
    let invocations = Rc::new(Cell::new(0));
    let callback = invocations.clone();
    let subscription = t.update_height(Cartographic::new(0., 0., 0.), move || callback.set(callback.get() + 1), None);
    t.pre_passes_update(&mut f).unwrap();
    t.update_for_pass(&mut f, &mut TilesetPassState::new(Cesium3DTilePass::Render)).unwrap();
    assert_eq!(t.selected_tiles, vec![0]);
    assert!((t.get_height(Cartographic::new(0., 0., 0.), &f, None).unwrap().unwrap() - 10.).abs() < 1e-6);
    t.tiles[0].expire_date = Some(epoch().add_seconds(-1.));
    f.frame_number += 1;
    t.update_for_pass(&mut f, &mut TilesetPassState::new(Cesium3DTilePass::Render)).unwrap();
    assert!(t.tiles[0].expired_content.is_some());
    assert!((t.get_height(Cartographic::new(0., 0., 0.), &f, None).unwrap().unwrap() - 10.).abs() < 1e-6);
    subscription.cancel();
    for callback in std::mem::take(&mut f.after_render) {
        callback().unwrap();
    }
    assert_eq!(invocations.get(), 0);
    t.options.enable_pick = false;
    assert!(t.get_height(Cartographic::new(0., 0., 0.), &f, None).unwrap().is_none());
    f.context_capabilities.webgl2 = true;
    assert!(t.get_height(Cartographic::new(0., 0., 0.), &f, None).unwrap().is_some());
    let callback = invocations.clone();
    let subscription = t.update_height(Cartographic::new(0., 0., 0.), move || callback.set(callback.get() + 1), None);
    f.frame_number += 2;
    t.pre_passes_update(&mut f).unwrap();
    t.update_for_pass(&mut f, &mut TilesetPassState::new(Cesium3DTilePass::Render)).unwrap();
    for callback in std::mem::take(&mut f.after_render) {
        callback().unwrap();
    }
    assert_eq!(invocations.get(), 1);
    t.destroy().unwrap();
    assert!(!subscription.is_active());
}
