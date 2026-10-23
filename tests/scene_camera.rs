//! Camera scenarios replayed against the pinned CesiumJS implementation.
mod common;
use cesium_rs::core::*;
use cesium_rs::scene::*;
use common::*;
use serde_json::{json, Value};
use std::{
    rc::Rc,
    sync::{Arc, Mutex},
};

fn value(v: &Value, key: &str, default: f64) -> f64 {
    v.get(key).map_or(default, n)
}
fn frustum(v: &Value) -> CameraFrustum {
    match s(&v["type"]) {
        "perspective" => {
            let mut f =
                PerspectiveFrustum::new(value(v, "fov", std::f64::consts::PI / 3.), value(v, "aspectRatio", 4. / 3.));
            f.near = value(v, "near", 1.);
            f.far = value(v, "far", 500_000_000.);
            f.x_offset = value(v, "xOffset", 0.);
            f.y_offset = value(v, "yOffset", 0.);
            CameraFrustum::Perspective(f)
        }
        "orthographic" => {
            let mut f = OrthographicFrustum::new(n(&v["width"]), value(v, "aspectRatio", 4. / 3.));
            f.near = value(v, "near", 1.);
            f.far = value(v, "far", 500_000_000.);
            CameraFrustum::Orthographic(f)
        }
        _ => CameraFrustum::OrthographicOffCenter(OrthographicOffCenterFrustum::new(
            n(&v["left"]),
            n(&v["right"]),
            n(&v["top"]),
            n(&v["bottom"]),
            value(v, "near", 1.),
            value(v, "far", 500_000_000.),
        )),
    }
}
fn frustum_value(f: CameraFrustum) -> Value {
    match f {
        CameraFrustum::Perspective(f) => {
            json!({"type":"perspective","fov":num(f.fov),"aspectRatio":num(f.aspect_ratio),"near":num(f.near),"far":num(f.far),"xOffset":num(f.x_offset),"yOffset":num(f.y_offset)})
        }
        CameraFrustum::Orthographic(f) => {
            json!({"type":"orthographic","width":num(f.width),"aspectRatio":num(f.aspect_ratio),"near":num(f.near),"far":num(f.far)})
        }
        CameraFrustum::OrthographicOffCenter(f) => {
            json!({"type":"offCenter","left":num(f.left),"right":num(f.right),"top":num(f.top),"bottom":num(f.bottom),"near":num(f.near),"far":num(f.far)})
        }
    }
}
fn snapshot(c: &mut Camera) -> Value {
    let position = c.position_wc().enc();
    let direction = c.direction_wc().enc();
    let up = c.up_wc().enc();
    let right = c.right_wc().enc();
    let cartographic = c.position_cartographic().map_or(
        Value::Null,
        |p| json!({"longitude":num(p.longitude),"latitude":num(p.latitude),"height":num(p.height)}),
    );
    let view = c.view_matrix().enc();
    let inverse_view = c.inverse_view_matrix().enc();
    let hpr = json!({"heading":c.heading().map(num),"pitch":c.pitch().map(num),"roll":c.roll().map(num)});
    let inverse_transform = c.inverse_transform().enc();
    json!({"position":c.position.enc(),"direction":c.direction.enc(),"up":c.up.enc(),"right":c.right.enc(),
        "positionWC":position,"directionWC":direction,"upWC":up,"rightWC":right,"cartographic":cartographic,
        "view":view,"inverseView":inverse_view,"transform":c.transform().enc(),"inverseTransform":inverse_transform,
        "frustum":frustum_value(c.frustum),"hpr":hpr,"magnitude":c.get_magnitude().map(num)})
}
fn pose(c: &mut Camera, p: &Value) {
    c.position = c3(&p[0]);
    c.direction = c3(&p[1]);
    c.up = c3(&p[2]);
    c.right = c3(&p[3]);
}
fn hpr(v: &Value) -> HeadingPitchRoll {
    HeadingPitchRoll::new(
        value(v, "heading", 0.),
        value(v, "pitch", -std::f64::consts::FRAC_PI_2),
        value(v, "roll", 0.),
    )
}
fn sphere(v: &Value) -> BoundingSphere {
    BoundingSphere::new(c3(&v[0]), n(&v[1]))
}
fn range(v: &Value) -> HeadingPitchRange {
    HeadingPitchRange::new(n(&v[0]), n(&v[1]), n(&v[2]))
}
fn offset(v: &Value) -> CameraOffset {
    if v.get("hpr").is_some() {
        CameraOffset::HeadingPitchRange(range(&v["hpr"]))
    } else {
        CameraOffset::Cartesian(c3(&v["cartesian"]))
    }
}
struct Surface {
    globe: Option<Cartesian3>,
    depth: Option<Cartesian3>,
}
#[derive(Default)]
struct Flights {
    callbacks: Arc<Mutex<Vec<Value>>>,
    path: Option<CameraFlightTween>,
    correction: Option<CameraCorrectionTween>,
}
fn view(v: &Value) -> CameraView {
    CameraView {
        destination: v.get("destination").map(|v| {
            if v.is_array() {
                CameraDestination::Cartesian(c3(v))
            } else {
                CameraDestination::Rectangle(rect(&v["rectangle"]))
            }
        }),
        orientation: v.get("orientation").map(|v| {
            if v.get("direction").is_some() {
                CameraOrientation::DirectionUp { direction: c3(&v["direction"]), up: c3(&v["up"]) }
            } else {
                CameraOrientation::HeadingPitchRoll(hpr(v))
            }
        }),
        convert: v["convert"].as_bool().unwrap_or(true),
        end_transform: v.get("endTransform").map(m4),
    }
}
fn flight_options(v: &Value, callbacks: &Arc<Mutex<Vec<Value>>>) -> CameraFlightOptions {
    let callback = |kind: &'static str| -> Option<CameraFlightCallback> {
        if v["noCallbacks"] == true {
            return None;
        }
        let events = Arc::clone(callbacks);
        let label = v["label"].as_str().unwrap_or("").to_owned();
        let destination = v.get("completeDestination").cloned();
        Some(Box::new(move |camera| {
            events.lock().unwrap().push(
                json!({"type":kind,"label":label,"active":camera.flight_in_progress(),"inputs":camera.enable_inputs}),
            );
            if kind == "complete" {
                if let Some(destination) = &destination {
                    camera
                        .fly_to(flight_options(
                            &json!({"destination":destination,"duration":1,"label":"chained"}),
                            &events,
                        ))
                        .unwrap();
                }
            }
        }))
    };
    let easing_function = v.get("easingFunction").map(|v| {
        if s(v) == "CUSTOM_SQUARE" {
            CameraEasing::Custom(Rc::new(|t| t * t))
        } else {
            CameraEasing::Builtin(EasingFunction::ALL.into_iter().find(|f| f.name() == s(v)).unwrap())
        }
    });
    CameraFlightOptions {
        view: view(v),
        duration: v.get("duration").filter(|v| !v.is_null()).map(n),
        maximum_height: v.get("maximumHeight").map(n),
        pitch_adjust_height: v.get("pitchAdjustHeight").map(n),
        fly_over_longitude: v.get("flyOverLongitude").map(n),
        fly_over_longitude_weight: v.get("flyOverLongitudeWeight").map(n),
        easing_function,
        complete: callback("complete"),
        cancel: callback("cancel"),
    }
}
fn descriptor(tween: &CameraFlightTween) -> Value {
    let easing = tween.easing_function.as_ref().map(|f| match f {
        CameraEasing::Builtin(f) => f.name(),
        CameraEasing::Custom(_) => "CUSTOM_SQUARE",
    });
    json!({"duration":num(tween.duration),"easing":easing,
        "start":tween.start_time.map_or(json!({}),|t|json!({"time":num(t)})),"stop":tween.stop_time.map_or(json!({}),|t|json!({"time":num(t)}))})
}
impl CameraSurface for Surface {
    fn pick_globe(&self, _: &Ray) -> Option<Cartesian3> {
        self.globe
    }
    fn pick_depth(&self, _: Cartesian2) -> Option<Cartesian3> {
        self.depth
    }
}
fn operation(
    c: &mut Camera,
    name: &str,
    args: &[Value],
    events: &Arc<Mutex<Vec<f64>>>,
    ellipsoid: &Ellipsoid,
    flights: &mut Flights,
) -> Result<Value, CameraError> {
    let a = args.first().unwrap_or(&Value::Null);
    let amount = || if a.is_null() { None } else { Some(n(a)) };
    match name {
        "flyTo" => c.fly_to(flight_options(a, &flights.callbacks))?,
        "flyToBoundingSphere" => c.fly_to_bounding_sphere(
            &sphere(a),
            args.get(1).and_then(|o| o.get("offset")).filter(|v| !v.is_null()).map(range),
            flight_options(args.get(1).unwrap_or(&json!({})), &flights.callbacks),
        )?,
        "flyHome" => c.fly_home(amount())?,
        "advance" => c.update_flights(n(a))?,
        "cancelFlight" => c.cancel_flight(),
        "completeFlight" => c.complete_flight()?,
        "flightState" => {
            return Ok(
                json!({"active":c.flight_in_progress(),"preload":c.can_preload_flight(),"inputs":c.enable_inputs,"count":c.scheduled_flight_count(),
            "planes":c.preload_flight_culling_volume.as_ref().map(|v|v.planes.iter().map(Enc::enc).collect::<Vec<_>>()),"callbacks":flights.callbacks.lock().unwrap().clone()}),
            )
        }
        "preloadCamera" => c.preload_flight_camera = Some(Box::new(c.clone_camera())),
        "path" => {
            let options = flight_options(a, &flights.callbacks);
            let options = CameraFlightPathOptions {
                destination: c3(&a["destination"]),
                orientation: hpr(a),
                duration: options.duration,
                convert: options.view.convert,
                end_transform: options.view.end_transform,
                maximum_height: options.maximum_height,
                pitch_adjust_height: options.pitch_adjust_height,
                fly_over_longitude: options.fly_over_longitude,
                fly_over_longitude_weight: options.fly_over_longitude_weight,
                easing_function: options.easing_function,
                complete: options.complete,
                cancel: options.cancel,
            };
            flights.path = Some(CameraFlightPath::create_tween(c, options)?);
            return Ok(descriptor(flights.path.as_ref().unwrap()));
        }
        "pathUpdate" => flights.path.as_ref().unwrap().update(c, n(a))?,
        "pathComplete" => flights.path.as_ref().unwrap().complete(c)?,
        "pathCancel" => flights.path.as_ref().unwrap().cancel(c),
        "correction" => {
            flights.correction = c.create_correct_position_tween(n(a));
            return Ok(flights.correction.map_or(Value::Null,|t|json!({"duration":t.duration,"easing":t.easing_function.name(),"start":{"time":0},"stop":{"time":1}})));
        }
        "correctionUpdate" => flights.correction.as_ref().unwrap().update(c, n(a)),
        "setView" => {
            let destination = a.get("destination").map(|v| {
                if v.is_array() {
                    CameraDestination::Cartesian(c3(v))
                } else {
                    CameraDestination::Rectangle(rect(&v["rectangle"]))
                }
            });
            let orientation = a.get("orientation").map(|v| {
                if v.get("direction").is_some() {
                    CameraOrientation::DirectionUp { direction: c3(&v["direction"]), up: c3(&v["up"]) }
                } else {
                    CameraOrientation::HeadingPitchRoll(hpr(v))
                }
            });
            c.set_view(CameraView {
                destination,
                orientation,
                convert: a["convert"].as_bool().unwrap_or(true),
                end_transform: a.get("endTransform").map(m4),
            })?;
        }
        "setTransform" => c.set_transform(m4(a))?,
        "lookAt" => c.look_at(c3(a), offset(&args[1]))?,
        "lookAtTransform" => c.look_at_transform(m4(a), args.get(1).filter(|v| !v.is_null()).map(offset))?,
        "rectangle" => return Ok(c.get_rectangle_camera_coordinates(rect(a)).map_or(Value::Null, |p| p.enc())),
        "pick" => return Ok(c.pick_ellipsoid(c2(a), ellipsoid).map_or(Value::Null, |p| p.enc())),
        "ray" => {
            return Ok(c
                .get_pick_ray(c2(a))
                .map_or(Value::Null, |r| json!({"origin":r.origin.enc(),"direction":r.direction.enc()})))
        }
        "sphere" => {
            let s = sphere(a);
            return Ok(json!([
                num(c.distance_to_bounding_sphere(&s)),
                num(c.get_pixel_size(&s, c.drawing_buffer_width, c.drawing_buffer_height))
            ]));
        }
        "viewBoundingSphere" => c.view_bounding_sphere(&sphere(a), args.get(1).filter(|v| !v.is_null()).map(range))?,
        "computeViewRectangle" => return Ok(c.compute_view_rectangle(ellipsoid)?.map_or(Value::Null, |r| r.enc())),
        "move" => c.move_direction(c3(a), n(&args[1])),
        "look" => c.look(c3(a), args.get(1).filter(|v| !v.is_null()).map(n)),
        "rotate" => c.rotate(c3(a), args.get(1).filter(|v| !v.is_null()).map(n)),
        "worldToCameraCoordinates" => return Ok(c.world_to_camera_coordinates(c4(a)).enc()),
        "cameraToWorldCoordinates" => return Ok(c.camera_to_world_coordinates(c4(a)).enc()),
        "worldToCameraCoordinatesPoint" => return Ok(c.world_to_camera_coordinates_point(c3(a)).enc()),
        "worldToCameraCoordinatesVector" => return Ok(c.world_to_camera_coordinates_vector(c3(a)).enc()),
        "cameraToWorldCoordinatesPoint" => return Ok(c.camera_to_world_coordinates_point(c3(a)).enc()),
        "cameraToWorldCoordinatesVector" => return Ok(c.camera_to_world_coordinates_vector(c3(a)).enc()),
        "state" => return Ok(snapshot(c)),
        "pose" => pose(c, a),
        "update" => c.update(SceneMode::from_value(n(a) as i32).unwrap())?,
        "frustum" => c.frustum = frustum(a),
        "clone" => *c = c.clone_camera(),
        "changed" => {
            c.update_camera_changed(n(a));
            return Ok(
                json!({"delta":num(c.position_wc_delta_magnitude),"last":num(c.position_wc_delta_magnitude_last_frame),"time":num(c.time_since_moved),"events":events.lock().unwrap().iter().copied().map(num).collect::<Vec<_>>()}),
            );
        }
        "moveForward" => c.move_forward(amount()),
        "moveBackward" => c.move_backward(amount()),
        "moveUp" => c.move_up(amount()),
        "moveDown" => c.move_down(amount()),
        "moveRight" => c.move_right(amount()),
        "moveLeft" => c.move_left(amount()),
        "zoomIn" => c.zoom_in(amount()),
        "zoomOut" => c.zoom_out(amount()),
        "lookLeft" => c.look_left(amount()),
        "lookRight" => c.look_right(amount()),
        "lookUp" => c.look_up(amount()),
        "lookDown" => c.look_down(amount()),
        "twistLeft" => c.twist_left(amount()),
        "twistRight" => c.twist_right(amount()),
        "rotateLeft" => c.rotate_left(amount()),
        "rotateRight" => c.rotate_right(amount()),
        "rotateUp" => c.rotate_up(amount()),
        "rotateDown" => c.rotate_down(amount()),
        "switchToOrthographicFrustum" => c.switch_to_orthographic_frustum(),
        "switchToPerspectiveFrustum" => c.switch_to_perspective_frustum(),
        _ => panic!("unknown camera operation: {name}"),
    }
    Ok(Value::Null)
}
fn scenario(o: &Value, ops: &Value) -> Value {
    let ellipsoid = o.get("ellipsoid").map_or(Ellipsoid::WGS84, |e| Ellipsoid::new(n(&e[0]), n(&e[1]), n(&e[2])));
    let mut options = CameraOptions::new(value(o, "width", 800.), value(o, "height", 600.));
    options.ellipsoid = o.get("sceneEllipsoid").map_or(ellipsoid, |e| Ellipsoid::new(n(&e[0]), n(&e[1]), n(&e[2])));
    options.client_width = value(o, "clientWidth", options.drawing_buffer_width);
    options.client_height = value(o, "clientHeight", options.drawing_buffer_height);
    options.pixel_ratio = value(o, "pixelRatio", 1.);
    options.map_mode_2d = MapMode2D::from_value(value(o, "mapMode2D", 1.) as i32).unwrap();
    options.minimum_zoom_distance = value(o, "minZoom", 1.);
    options.maximum_zoom_distance = value(o, "maxZoom", f64::INFINITY);
    options.map_projection = if o["projection"] == "mercator" {
        Rc::new(WebMercatorProjection::new(ellipsoid))
    } else {
        Rc::new(GeographicProjection::new(ellipsoid))
    };
    if o.get("globe").is_some() || o.get("depth").is_some() {
        options.surface = Some(Rc::new(Surface { globe: o.get("globe").map(c3), depth: o.get("depth").map(c3) }));
    }
    let mut camera = Camera::new(options);
    let mut flights = Flights::default();
    let callbacks = Arc::clone(&flights.callbacks);
    camera
        .morph_completion_requested
        .add_event_listener(move |_| callbacks.lock().unwrap().push(json!({"type":"morph"})));
    if let Some(f) = o.get("frustum") {
        camera.frustum = frustum(f);
    }
    if let Some(p) = o.get("pose") {
        pose(&mut camera, p);
    }
    camera.update(SceneMode::from_value(value(o, "mode", 3.) as i32).unwrap()).unwrap();
    if let Some(t) = o.get("transform") {
        camera.set_transform(m4(t)).unwrap();
    }
    camera.constrained_axis = o.get("constrainedAxis").map(c3);
    camera.percentage_changed = value(o, "percentageChanged", 0.5);
    let events = Arc::new(Mutex::new(Vec::new()));
    if o["events"] == true {
        let events = Arc::clone(&events);
        camera.changed.add_event_listener(move |x| events.lock().unwrap().push(*x));
    }
    let results: Vec<_> = ops
        .as_array()
        .unwrap()
        .iter()
        .map(|op| {
            let a = op.as_array().unwrap();
            match operation(&mut camera, s(&a[0]), &a[1..], &events, &ellipsoid, &mut flights) {
                Ok(v) => json!({"ok":v}),
                Err(_) => json!({"throws":"DeveloperError"}),
            }
        })
        .collect();
    json!({"results":results,"camera":snapshot(&mut camera)})
}
#[test]
fn camera_reference_scenarios() {
    run(
        "scene",
        "Camera",
        |_| Tol { rel: 1e-11, abs: 1e-9, vector: false },
        |f, a| match f {
            "scenario" => ok(scenario(&a[0], &a[1])),
            _ => None,
        },
    );
}

#[test]
fn camera_flight_reference_scenarios() {
    run(
        "scene",
        "CameraFlights",
        |_| Tol { rel: 1e-11, abs: 1e-8, vector: false },
        |f, a| match f {
            "scenario" => ok(scenario(&a[0], &a[1])),
            _ => None,
        },
    );
    run(
        "scene",
        "CameraFlightPath",
        |_| Tol { rel: 1e-11, abs: 1e-8, vector: false },
        |f, a| match f {
            "scenario" => ok(scenario(&a[0], &a[1])),
            _ => None,
        },
    );
}

#[test]
fn uniform_snapshot_uses_camera_world_frame() {
    let mut c = Camera::new(CameraOptions::new(800., 600.));
    c.look_at(Cartesian3::new(6_378_137., 0., 0.), CameraOffset::Cartesian(Cartesian3::new(1000., 2000., 3000.)))
        .unwrap();
    let snapshot = c.uniform_camera();
    let mut state = cesium_rs::renderer::UniformState::default();
    state.update_camera_snapshot(&snapshot, &Ellipsoid::WGS84).unwrap();
    // At the equator, ENU is (world Y, world Z, world X).
    let expected_eye = Cartesian3::new(6_381_137., 1000., 2000.);
    assert!(state.camera_position.distance(expected_eye) < 1e-8);
    assert!(state.view.multiply_by_point(expected_eye).magnitude() < 1e-8);
    assert_eq!(state.frustum, [1., 500_000_000.]);
    let center = c.get_pick_ray(Cartesian2::new(400., 300.)).unwrap();
    let expected_direction = (Cartesian3::new(6_378_137., 0., 0.) - expected_eye).normalize();
    assert!(center.direction.dot(expected_direction) > 1.0 - 1e-12);
}

#[test]
fn concurrent_camera_flights_keep_their_own_destinations() {
    let mut first = Camera::new(CameraOptions::new(800., 600.));
    let mut second = Camera::new(CameraOptions::new(800., 600.));
    let a = Cartesian3::from_degrees(10., 25., 1000., &Ellipsoid::WGS84);
    let b = Cartesian3::from_degrees(-120., -35., 2000., &Ellipsoid::WGS84);
    first.fly_to(CameraFlightOptions::new(CameraDestination::Cartesian(a))).unwrap();
    second.fly_to(CameraFlightOptions::new(CameraDestination::Cartesian(b))).unwrap();
    first.update_flights(0.).unwrap();
    second.update_flights(0.).unwrap();
    first.update_flights(1.).unwrap();
    second.update_flights(1.).unwrap();
    first.complete_flight().unwrap();
    second.complete_flight().unwrap();
    assert!(first.position_wc().distance(a) < 1e-7);
    assert!(second.position_wc().distance(b) < 1e-7);
    assert!(first.enable_inputs && second.enable_inputs);
    assert_eq!(first.scheduled_flight_count() + second.scheduled_flight_count(), 0);
}

#[test]
fn cancelling_releases_captured_callback_resources() {
    let mut camera = Camera::new(CameraOptions::new(800., 600.));
    let resource = Rc::new(());
    let weak = Rc::downgrade(&resource);
    let mut options = CameraFlightOptions::new(CameraDestination::Cartesian(Cartesian3::new(7e6, 1e6, 2e6)));
    options.complete = Some(Box::new(move |_| {
        let _keep_alive = &resource;
    }));
    camera.fly_to(options).unwrap();
    assert!(weak.upgrade().is_some());
    camera.cancel_flight();
    assert!(weak.upgrade().is_none());
    camera.update_flights(100.).unwrap();
    assert!(!camera.flight_in_progress());
}
