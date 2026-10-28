//! Reference tests for Scene input, tweening, and morphing.
use cesium_rs::core::*;
use cesium_rs::scene::*;
use cesium_rs::scene::device_orientation_camera_controller::DeviceOrientationCameraController;
use cesium_rs::scene::scene_transitioner::SceneTransitioner;
use cesium_rs::scene::screen_space_camera_controller::ScreenSpaceCameraController;
use serde_json::{json, Value};
use std::{
    cell::{Cell, RefCell},
    collections::BTreeMap,
    rc::Rc,
};
fn fixture() -> Value {
    serde_json::from_str(include_str!("data/scene-input-tweens-morphing.json")).unwrap()
}
fn near(actual: &Value, expected: &Value, path: &str) {
    match (actual, expected) {
        (Value::Number(a), Value::Number(b)) => {
            let (a, b) = (a.as_f64().unwrap(), b.as_f64().unwrap());
            assert!((a - b).abs() <= 2e-8_f64.max(b.abs() * 2e-12), "{path}: {a} != {b}");
        }
        (Value::Array(a), Value::Array(b)) => {
            assert_eq!(a.len(), b.len(), "{path}");
            for (i, (a, b)) in a.iter().zip(b).enumerate() {
                near(a, b, &format!("{path}[{i}]"));
            }
        }
        (Value::Object(a), Value::Object(b)) => {
            for (k, b) in b {
                near(a.get(k).unwrap_or_else(|| panic!("missing {path}.{k}")), b, &format!("{path}.{k}"));
            }
        }
        _ => assert_eq!(actual, expected, "{path}"),
    }
}
fn c2(v: &Value) -> Cartesian2 {
    Cartesian2::new(v[0].as_f64().unwrap(), v[1].as_f64().unwrap())
}
fn modifier(v: &Value) -> Option<KeyboardEventModifier> {
    v.as_u64().and_then(|v| KeyboardEventModifier::from_value(v as u8))
}
fn fire(a: &mut CameraEventAggregator, v: &Value) {
    if v["reset"] == true {
        a.reset();
        return;
    }
    let kind = ScreenSpaceEventType::from_value(v["kind"].as_u64().unwrap() as u8).unwrap();
    let e = if v.get("position").is_some() {
        ScreenSpaceInputEvent::Position(c2(&v["position"]))
    } else if v.get("start").is_some() {
        ScreenSpaceInputEvent::Movement { start_position: c2(&v["start"]), end_position: c2(&v["end"]) }
    } else if let Some(delta) = v["delta"].as_f64() {
        ScreenSpaceInputEvent::Wheel(delta)
    } else if v.get("p1").is_some() {
        ScreenSpaceInputEvent::TwoPositions { position1: c2(&v["p1"]), position2: c2(&v["p2"]) }
    } else if v.get("distance").is_some() {
        ScreenSpaceInputEvent::PinchMovement {
            distance_start: c2(&v["distance"][0]),
            distance_end: c2(&v["distance"][1]),
            angle_height_start: c2(&v["angle"][0]),
            angle_height_end: c2(&v["angle"][1]),
        }
    } else {
        ScreenSpaceInputEvent::End
    };
    a.process_event(kind, modifier(&v["modifier"]), &e, v["time"].as_f64().unwrap()).unwrap();
}
fn movement(m: CameraMovement) -> Value {
    json!({"start":[m.start_position.x,m.start_position.y],"end":[m.end_position.x,m.end_position.y]})
}
fn camera(mode: SceneMode, ortho: bool) -> Camera {
    let mut c = Camera::new(CameraOptions::new(800., 600.));
    if mode == SceneMode::Scene2D {
        c.frustum = CameraFrustum::OrthographicOffCenter(OrthographicOffCenterFrustum {
            left: -4e6,
            right: 4e6,
            top: 3e6,
            bottom: -3e6,
            near: 1.,
            far: 500e6,
        });
        c.position = Cartesian3::new(1e6, 2e6, 2. * Ellipsoid::WGS84.maximum_radius);
        c.direction = Cartesian3::UNIT_Z.negate();
        c.up = Cartesian3::UNIT_Y;
        c.right = Cartesian3::UNIT_X;
    } else {
        c.frustum = if ortho {
            CameraFrustum::Orthographic(OrthographicFrustum::new(8e6, 4. / 3.))
        } else {
            CameraFrustum::Perspective(PerspectiveFrustum::new(std::f64::consts::FRAC_PI_3, 4. / 3.))
        };
        c.position = if mode == SceneMode::Scene3D {
            Ellipsoid::WGS84.cartographic_to_cartesian(&Cartographic::from_degrees(25., 20., 2e6))
        } else {
            Cartesian3::new(1e6, 2e6, 8e6)
        };
        c.direction = if mode == SceneMode::Scene3D {
            c.position.normalize().negate()
        } else {
            Cartesian3::new(0., -1., -1.).normalize()
        };
        c.right = if mode == SceneMode::Scene3D {
            c.direction.cross(Cartesian3::UNIT_Z).normalize()
        } else {
            Cartesian3::UNIT_X
        };
        c.up = c.right.cross(c.direction);
    }
    c.update(mode).unwrap();
    c
}
fn camera_state(c: &mut Camera) -> Value {
    c.position_wc();
    let v = |p: Cartesian3| json!([p.x, p.y, p.z]);
    let frustum = match c.frustum {
        CameraFrustum::Perspective(f) => {
            json!({"kind":"perspective","values":[f.fov,f.aspect_ratio,f.near,f.far,f.x_offset,f.y_offset]})
        }
        CameraFrustum::Orthographic(f) => json!({"kind":"orthographic","values":[f.width,f.aspect_ratio,f.near,f.far]}),
        CameraFrustum::OrthographicOffCenter(f) => {
            json!({"kind":"offCenter","values":[f.left,f.right,f.top,f.bottom,f.near,f.far]})
        }
    };
    json!({"position":v(c.position),"direction":v(c.direction),"up":v(c.up),"right":v(c.right),"frustum":frustum})
}
#[test]
fn pinned_tweens_all_easings_delays_repeats_and_skipped_frames() {
    for case in fixture()["tweens"].as_array().unwrap() {
        let collection = TweenCollection::new();
        let updates = Rc::new(Cell::new(0));
        let count = updates.clone();
        let events = Rc::new(RefCell::new(Vec::new()));
        let completed = events.clone();
        let cancelled = events.clone();
        let t = collection
            .add(TweenOptions {
                start_object: BTreeMap::from([("x".into(), -3.), ("y".into(), 8.), ("untouched".into(), 12.)]),
                stop_object: BTreeMap::from([("x".into(), 17.), ("y".into(), -2.), ("missing".into(), 4.)]),
                duration: case["duration"].as_f64().unwrap(),
                delay: case["delay"].as_f64().unwrap(),
                repeat: case["repeat"].as_f64().unwrap(),
                easing_function: TweenEasing::Builtin(
                    EasingFunction::ALL.into_iter().find(|e| e.name() == case["easing"].as_str().unwrap()).unwrap(),
                ),
                update: Some(Rc::new(move |_| count.set(count.get() + 1))),
                complete: Some(Rc::new(move || completed.borrow_mut().push("complete"))),
                cancel: Some(Rc::new(move || cancelled.borrow_mut().push("cancel"))),
            })
            .unwrap();
        for (time, expected) in case["times"].as_array().unwrap().iter().zip(case["states"].as_array().unwrap()) {
            collection.update(time.as_f64().unwrap());
            let actual = json!({"value":t.value(),"length":collection.length(),"updates":updates.get(),"events":*events.borrow()});
            near(&actual, expected, &format!("tween {} @ {time}", case["easing"]));
        }
        near(&json!(t.start_object()), &case["start"], "start snapshot");
        near(&json!(t.stop_object()), &case["stop"], "stop snapshot");
    }
}
#[test]
fn pinned_aggregation_modifier_handoff_wheel_and_pinch_wrap() {
    let mut a = CameraEventAggregator::new(800.);
    for case in fixture()["aggregation"].as_array().unwrap() {
        fire(&mut a, &case["command"]);
        let expected = &case["state"];
        near(&json!([a.current_mouse_position().x, a.current_mouse_position().y]), &expected["mouse"], "mouse");
        assert_eq!(a.any_button_down(), expected["any"]);
        for s in expected["states"].as_array().unwrap() {
            let t = CameraEventType::from_value(s["type"].as_i64().unwrap() as i32).unwrap();
            let m = modifier(&s["modifier"]);
            let motion = a.get_movement(t, m);
            let start = a.get_start_mouse_position(t, m);
            let actual = json!({"moving":a.is_moving(t,m),"down":a.is_button_down(t,m),"start":[start.x,start.y],"press":a.get_button_press_time(t,m),"release":a.get_button_release_time(t,m),"distance":motion.distance.map(movement),"angle":motion.angle_and_height.map(movement),"last":a.get_last_movement(t,m).map(movement)});
            let mut expected = s.clone();
            expected.as_object_mut().unwrap().remove("type");
            expected.as_object_mut().unwrap().remove("modifier");
            expected.as_object_mut().unwrap().remove("movement");
            near(&actual, &expected, &format!("aggregate {:?} {:?} cmd {}", t, m, case["command"]));
            if t != CameraEventType::Pinch {
                near(&movement(motion.movement), &s["movement"], "movement");
            }
        }
    }
}
#[test]
fn pinned_device_orientation_missing_samples_and_wraparound() {
    let mut c = camera(SceneMode::Scene3D, false);
    let mut controller = DeviceOrientationCameraController::new();
    for case in fixture()["orientation"].as_array().unwrap() {
        let a = &case["angles"];
        controller.orientation(a[0].as_f64(), a[1].as_f64().unwrap(), a[2].as_f64().unwrap()).unwrap();
        controller.update(&mut c).unwrap();
        near(&camera_state(&mut c), &case["state"], "orientation");
    }
    controller.destroy();
    assert!(controller.is_destroyed());
    assert!(controller.update(&mut c).is_err());
}
#[test]
fn pinned_controller_ellipsoid_and_map_gestures() {
    for case in fixture()["controllers"].as_array().unwrap() {
        let mode = SceneMode::from_value(case["mode"].as_i64().unwrap() as i32).unwrap();
        let mut c = camera(mode, case["ortho"].as_bool().unwrap());
        let mut controller = ScreenSpaceCameraController::new(&c);
        controller.inertia_spin = 0.;
        controller.inertia_translate = 0.;
        controller.inertia_zoom = 0.;
        let action = case["action"].as_str().unwrap();
        controller.enable_translate = action == "translate";
        controller.enable_zoom = action == "zoom";
        controller.enable_look = action == "look";
        controller.enable_tilt = action == "tilt";
        controller.enable_rotate = action == "spin";
        if action == "zoom" {
            fire(&mut controller.aggregator, &json!({"kind":16,"time":1.,"delta":120.}));
        } else {
            let modifier = if action == "look" { json!(0) } else { Value::Null };
            fire(
                &mut controller.aggregator,
                &json!({"kind":if action=="tilt"{10}else{0},"time":1.,"position":[400.,300.],"modifier":modifier}),
            );
            fire(
                &mut controller.aggregator,
                &json!({"kind":15,"time":1.01,"start":[400.,300.],"end":[430.,320.],"modifier":modifier}),
            );
        }
        controller.update(&mut c, 1.01).unwrap();
        near(&camera_state(&mut c), &case["state"], &format!("controller {mode:?} {action} ortho {}", case["ortho"]));
    }
}
#[test]
fn pinned_scene_morph_routes() {
    for case in fixture()["morphs"].as_array().unwrap() {
        let from = SceneMode::from_value(case["from"].as_i64().unwrap() as i32).unwrap();
        let to = SceneMode::from_value(case["to"].as_i64().unwrap() as i32).unwrap();
        let duration = case["duration"].as_f64().unwrap();
        let mut c = camera(from, case["ortho"].as_bool().unwrap());
        let mut t = SceneTransitioner::new(&c);
        let events = std::sync::Arc::new(std::sync::Mutex::new(Vec::new()));
        let e = events.clone();
        t.morph_start.add_event_listener(move |v| {
            e.lock().unwrap().push(json!([v.previous_mode.value(), v.mode.value(), v.was_morphing]))
        });
        let e = events.clone();
        t.morph_complete.add_event_listener(move |v| {
            e.lock().unwrap().push(json!([v.previous_mode.value(), v.mode.value(), v.was_morphing]))
        });
        match to {
            SceneMode::Scene2D => t.morph_to_2d(&mut c, duration),
            SceneMode::Scene3D => t.morph_to_3d(&mut c, duration),
            _ => t.morph_to_columbus_view(&mut c, duration),
        }
        .unwrap();
        for expected in case["states"].as_array().unwrap() {
            let time = expected["time"].as_f64().unwrap();
            t.update(&mut c, time).unwrap();
            let actual = json!({"time":time,"mode":t.mode.value(),"morph":t.morph_time,"state":camera_state(&mut c),"events":*events.lock().unwrap()});
            near(
                &actual,
                expected,
                &format!("morph {from:?}->{to:?} duration {duration} ortho {} @{time}", case["ortho"]),
            );
        }
    }
}
#[test]
fn tween_zero_duration_cancellation_handles_and_reentrant_completion() {
    let collection = TweenCollection::new();
    let complete = Rc::new(Cell::new(0));
    let completed = complete.clone();
    let zero = collection
        .add(TweenOptions {
            duration: 0.,
            complete: Some(Rc::new(move || completed.set(completed.get() + 1))),
            ..Default::default()
        })
        .unwrap();
    assert_eq!(complete.get(), 1);
    assert!(!collection.contains(&zero));
    assert!(!zero.cancel_tween());
    assert_eq!(collection.length(), 0);
    assert!(collection.add(TweenOptions { duration: -1., ..Default::default() }).is_err());
    let cancel = Rc::new(Cell::new(0));
    let cancelled = cancel.clone();
    let handle = collection
        .add(TweenOptions { cancel: Some(Rc::new(move || cancelled.set(cancelled.get() + 1))), ..Default::default() })
        .unwrap();
    assert!(handle.needs_start());
    assert!(collection.get(0).unwrap().cancel_tween());
    assert!(!handle.cancel_tween());
    assert_eq!(cancel.get(), 1);
    let pending = collection.clone();
    let completed = complete.clone();
    collection
        .add(TweenOptions {
            duration: 1.,
            complete: Some(Rc::new(move || {
                pending
                    .add(TweenOptions {
                        duration: 2.,
                        complete: Some(Rc::new({
                            let count = completed.clone();
                            move || count.set(count.get() + 1)
                        })),
                        ..Default::default()
                    })
                    .unwrap();
            })),
            ..Default::default()
        })
        .unwrap();
    collection.update(10.);
    collection.update(11.);
    assert_eq!(collection.length(), 1);
    assert!(!collection.get(0).unwrap().needs_start());
    collection.update(13.);
    assert_eq!(collection.length(), 0);
    assert_eq!(complete.get(), 2);
}
#[test]
fn tween_property_material_alpha_and_repeating_offset() {
    let collection = TweenCollection::new();
    let object = Rc::new(RefCell::new(BTreeMap::from([("amount".into(), 42.)])));
    assert!(collection.add_property(object.clone(), "absent", 1., 2., TweenOptions::default()).is_err());
    let t = collection.add_property(object.clone(), "amount", 2., 8., TweenOptions::default()).unwrap();
    assert_eq!(object.borrow()["amount"], 42.);
    collection.update(0.);
    assert_eq!(object.borrow()["amount"], 2.);
    collection.update(1.5);
    assert_eq!(object.borrow()["amount"], 5.);
    collection.update(3.);
    assert_eq!(object.borrow()["amount"], 8.);
    assert!(!collection.contains(&t));
    let material = Rc::new(RefCell::new(Material::from_type("Checkerboard")));
    collection.add_alpha(material.clone(), 0.1, 0.9, TweenOptions { duration: 2., ..Default::default() }).unwrap();
    collection.update(4.);
    collection.update(5.);
    for key in ["lightColor", "darkColor"] {
        let Some(MaterialValue::Vec4(color)) = material.borrow().uniform(key).cloned() else { panic!("color") };
        assert!((color.w - 0.5).abs() < 1e-14);
    }
    let fabric = cesium_rs::scene::Fabric::default()
        .uniform("offset", MaterialValue::Float(2.))
        .source("czm_material czm_getMaterial(czm_materialInput i) { return czm_getDefaultMaterial(i); }");
    let material =
        Rc::new(RefCell::new(Material::new(cesium_rs::scene::MaterialOptions { fabric, ..Default::default() })));
    let handle =
        collection.add_offset_increment(material.clone(), TweenOptions { duration: 1., ..Default::default() }).unwrap();
    collection.update(10.);
    collection.update(10.5);
    assert_eq!(material.borrow().uniform("offset"), Some(&MaterialValue::Float(2.5)));
    collection.update(12.5);
    assert_eq!(material.borrow().uniform("offset"), Some(&MaterialValue::Float(2.5)));
    assert!(handle.cancel_tween());
    collection.remove_all();
    assert!(collection.is_empty());
}
#[test]
fn raw_host_input_aggregation_and_lifecycle() {
    let mut a = CameraEventAggregator::new(800.);
    let mut event = ScreenInput::new(ScreenInputKind::MouseDown, 1000.);
    event.client_position = Cartesian2::new(110., 220.);
    event.element_origin = Cartesian2::new(10., 20.);
    assert!(a.process(&event).unwrap());
    event.kind = ScreenInputKind::MouseMove;
    event.now = 1010.;
    event.client_position = Cartesian2::new(140., 240.);
    a.process(&event).unwrap();
    let m = a.get_movement(CameraEventType::LeftDrag, None).movement;
    assert_eq!(m.start_position, Cartesian2::new(100., 200.));
    assert_eq!(m.end_position, Cartesian2::new(130., 220.));
    assert_eq!(a.get_button_press_time(CameraEventType::LeftDrag, None), Some(1.));
    event.kind = ScreenInputKind::MouseUp;
    event.modifier = Some(KeyboardEventModifier::Ctrl);
    event.now = 1100.;
    a.process(&event).unwrap();
    assert!(!a.any_button_down());
    assert!(!a.is_button_down(CameraEventType::LeftDrag, None));
    a.destroy();
    assert!(a.is_destroyed());
    assert!(a.process(&event).is_err());
}
#[test]
fn morph_completion_interrupt_log_depth_input_and_disabled_handler() {
    let mut c = camera(SceneMode::Scene3D, false);
    let mut t = SceneTransitioner::new(&c);
    t.use_log_depth = true;
    assert!(!t.morph_to_3d(&mut c, 2.).unwrap());
    assert!(t.morph_to_2d(&mut c, 2.).unwrap());
    t.update(&mut c, 0.).unwrap();
    t.update(&mut c, 0.5).unwrap();
    assert!(t.is_morphing());
    assert!(!t.process_input(&mut c, ScreenSpaceEventType::MouseMove, None).unwrap());
    assert!(!t.process_input(&mut c, ScreenSpaceEventType::LeftDown, Some(KeyboardEventModifier::Shift)).unwrap());
    assert!(t.process_input(&mut c, ScreenSpaceEventType::Wheel, None).unwrap());
    assert_eq!(t.mode, SceneMode::Scene2D);
    assert_eq!(t.morph_time, 0.);
    assert!(!t.is_morphing());
    t.morph_to_3d(&mut c, 1.).unwrap();
    assert!(t.complete_morph(&mut c).unwrap());
    assert_eq!(t.mode, SceneMode::Scene3D);
    let CameraFrustum::Perspective(f) = c.frustum else { panic!("perspective") };
    assert_eq!((f.near, f.far), (0.1, 1e10));
    t.complete_morph_on_user_input = false;
    t.morph_to_columbus_view(&mut c, 2.).unwrap();
    assert!(!t.complete_morph(&mut c).unwrap());
    assert!(!t.morph_to_2d(&mut c, 2.).unwrap());
    t.update(&mut c, 0.).unwrap();
    t.update(&mut c, 2.).unwrap();
    t.update(&mut c, 4.).unwrap();
    assert_eq!(t.mode, SceneMode::ColumbusView);
    t.destroy();
    assert!(t.is_destroyed());
    assert!(t.update(&mut c, 9.).is_err());
    assert!(t.morph_to_2d(&mut c, 0.).is_err());
}
struct TestSurface(SceneMode);
impl CameraSurface for TestSurface {
    fn pick_globe(&self, ray: &Ray) -> Option<Cartesian3> {
        if self.0 == SceneMode::Scene3D {
            let interval = intersection_tests::ray_ellipsoid(ray, &Ellipsoid::WGS84)?;
            Some(ray.get_point(if interval.start > 0. { interval.start } else { interval.stop }))
        } else {
            let t = -ray.origin.x / ray.direction.x;
            (t >= 0.).then(|| ray.get_point(t))
        }
    }
    fn pick_depth(&self, _: Cartesian2) -> Option<Cartesian3> {
        None
    }
}
#[test]
fn pinned_controller_sequences_cursor_zoom_inertia_and_surface_queries() {
    for case in fixture()["controllerSequences"].as_array().unwrap() {
        let mode = SceneMode::from_value(case["mode"].as_i64().unwrap() as i32).unwrap();
        let mut c = camera(mode, false);
        let altitude = case["altitude"].as_f64();
        if let Some(altitude) = altitude {
            if mode == SceneMode::Scene3D {
                c.position =
                    Ellipsoid::WGS84.cartographic_to_cartesian(&Cartographic::from_degrees(25., 20., altitude));
                c.direction = c.position.normalize().negate();
                c.right = c.direction.cross(Cartesian3::UNIT_Z).normalize();
                c.up = c.right.cross(c.direction);
            } else {
                c.position.z = altitude;
            }
        }
        let has_surface = case["surface"].as_bool().unwrap();
        if has_surface {
            c.surface = Some(Rc::new(TestSurface(mode)));
        }
        let mut controller = ScreenSpaceCameraController::new(&c);
        let action = case["action"].as_str().unwrap();
        let config = &case["config"];
        controller.enable_translate = config["enableTranslate"].as_bool().unwrap();
        controller.enable_zoom = config["enableZoom"].as_bool().unwrap();
        controller.enable_rotate = config["enableRotate"].as_bool().unwrap();
        controller.enable_tilt = config["enableTilt"].as_bool().unwrap();
        controller.enable_look = config["enableLook"].as_bool().unwrap();
        controller.maximum_movement_ratio = config["maximumMovementRatio"].as_f64().unwrap();
        controller.minimum_zoom_distance = 10.;
        controller.maximum_zoom_distance = 2e7;
        controller.minimum_picking_terrain_distance_with_inertia = 1e12;
        let env = screen_space_camera_controller::ControllerEnvironment {
            has_globe: has_surface,
            globe_height: altitude.map(|_| 0.),
            camera_underground: altitude.is_some_and(|h| h < 0.),
            ..Default::default()
        };
        for command in case["commands"].as_array().unwrap() {
            fire(&mut controller.aggregator, command);
        }
        for (index, expected) in case["frames"].as_array().unwrap().iter().enumerate() {
            let time = expected["time"].as_f64().unwrap();
            if index == 1 {
                if action == "zoom" {
                    fire(&mut controller.aggregator, &json!({"kind":16,"time":1.02,"delta":case["delta"]}));
                } else {
                    fire(
                        &mut controller.aggregator,
                        &json!({"kind":15,"time":1.02,"start":[440.,320.],"end":[460.,330.],"modifier":if action=="look"{json!(0)}else{Value::Null}}),
                    );
                }
            }
            if index == 2 && action != "zoom" {
                fire(
                    &mut controller.aggregator,
                    &json!({"kind":if action=="tilt"||action=="twist"{11}else{1},"time":1.1,"position":[460.,330.],"modifier":if action=="look"{json!(0)}else{Value::Null}}),
                );
            }
            controller.update_with_environment(&mut c, time, env).unwrap();
            near(
                &camera_state(&mut c),
                &expected["state"],
                &format!(
                    "sequence {mode:?} {action} surface {has_surface} altitude {altitude:?} delta {} @{time}",
                    case["delta"]
                ),
            );
        }
    }
}
#[test]
fn morph_handler_is_latched_and_zero_duration_orthographic_wait_matches_source() {
    let mut c = camera(SceneMode::Scene3D, true);
    let mut t = SceneTransitioner::new(&c);
    t.complete_morph_on_user_input = false;
    t.morph_to_2d(&mut c, 0.).unwrap();
    assert_eq!(t.mode, SceneMode::Morphing);
    t.update(&mut c, 100.).unwrap();
    assert!(t.is_morphing());
    assert!(!t.complete_morph(&mut c).unwrap());
    t.destroy();
    let mut c = camera(SceneMode::Scene3D, false);
    let mut t = SceneTransitioner::new(&c);
    t.morph_to_columbus_view(&mut c, 2.).unwrap();
    t.complete_morph_on_user_input = false;
    assert!(t.complete_morph(&mut c).unwrap());
    assert_eq!(t.mode, SceneMode::ColumbusView);
}
