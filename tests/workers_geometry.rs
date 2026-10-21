//! Direct CesiumJS worker fixtures, including wire output and async dispatch.
mod common;
use cesium_rs::{core::*, workers::*};
use common::*;
use serde_json::{json, Value};
use std::{
    future::Future,
    sync::Arc,
    task::{Context, Poll, Wake, Waker},
};

// Same tolerance as Core geometry suites: Float32 values can differ by one ULP.
const GEOMETRY: Tol = Tol { rel: 2e-7, abs: 2e-7, vector: false };
struct ThreadWake(std::thread::Thread);
impl Wake for ThreadWake {
    fn wake(self: Arc<Self>) {
        self.0.unpark();
    }
}
fn block_on<T>(future: impl Future<Output = T>) -> T {
    let waker = Waker::from(Arc::new(ThreadWake(std::thread::current())));
    let mut cx = Context::from_waker(&waker);
    let mut future = std::pin::pin!(future);
    loop {
        match future.as_mut().poll(&mut cx) {
            Poll::Ready(value) => return value,
            Poll::Pending => std::thread::park(),
        }
    }
}
fn replay<T: WorkerGeometry>(a: &[Value], entry: impl Fn(GeometryInput<'_, T>) -> Option<Geometry>) -> Option<Out> {
    let values = nums(&a[0]);
    let geometry = if a[1].is_null() {
        T::unpack_worker(&values, 0).and_then(|description| entry(GeometryInput::Description(&description)))
    } else {
        entry(GeometryInput::Packed { values: &values, offset: u(&a[1]) as usize })
    };
    ok(geometry_enc(geometry))
}
fn task(value: &Value) -> GeometrySubTask {
    let module = format!("create{}", s(&value["name"]));
    let values = nums(&value["packed"]);
    let offset = u(&value["offset"]) as usize;
    let mode = s(&value["mode"]);
    GeometrySubTask {
        geometry: if mode == "ready" {
            GeometrySource::ready(
                GeometryDescription::unpack(&module, &values, offset).and_then(|d| d.create_geometry()),
            )
        } else {
            GeometrySource::Packed { values, offset }
        },
        module_name: (mode == "name").then(|| module.clone()),
        module_path: (mode == "path").then(|| format!("./{module}.js")),
    }
}
fn ready(geometry: Option<Geometry>) -> GeometrySubTask {
    GeometrySubTask { geometry: GeometrySource::ready(geometry), module_name: None, module_path: None }
}
fn wire_geometry(vertex_count: usize) -> Geometry {
    let mut attributes = GeometryAttributes::default();
    for (name, datatype) in [
        ("position", ComponentDatatype::Double),
        ("normal", ComponentDatatype::Float),
        ("i8", ComponentDatatype::Byte),
        ("u8", ComponentDatatype::UnsignedByte),
        ("i16", ComponentDatatype::Short),
        ("u16", ComponentDatatype::UnsignedShort),
        ("i32", ComponentDatatype::Int),
        ("u32", ComponentDatatype::UnsignedInt),
    ] {
        let mut values = TypedArray::zeros(datatype, vertex_count);
        values.set(0, 1.0);
        values.set(vertex_count - 1, 17.0);
        let mut attribute = GeometryAttribute::new(datatype, 1, values);
        attribute.normalize = name == "u8";
        attributes.set(name, attribute);
    }
    let mut g = Geometry::new(
        attributes,
        Some(IndexDatatype::create_typed_array(vertex_count, &[0, 1, vertex_count as u32 - 1])),
        PrimitiveType::Points,
    );
    g.geometry_type = GeometryType::Lines;
    g.bounding_sphere = Some(BoundingSphere::new(Cartesian3::new(1.0, 2.0, 3.0), 5.0));
    g.bounding_sphere_cv = Some(BoundingSphere::new(Cartesian3::new(4.0, 5.0, 6.0), 7.0));
    g.offset_attribute = Some(GeometryOffsetAttribute::Top);
    g
}
fn box_geometry() -> Geometry {
    BoxGeometry::new(Cartesian3::new(-1.0, -2.0, -3.0), Cartesian3::new(3.0, 4.0, 5.0), VertexFormat::ALL, None)
        .unwrap()
        .create_geometry()
        .unwrap()
}
#[test]
fn batch_reference() {
    run(
        "workers",
        "createGeometry",
        |_| GEOMETRY,
        |f, a| {
            let tasks = match f {
                "batch" => a[0].as_array().unwrap().iter().map(task).collect(),
                "undefined" => vec![ready(None), ready(Some(box_geometry())), ready(None)],
                "conflict" => vec![GeometrySubTask {
                    geometry: GeometrySource::Ready(None),
                    module_name: Some("createBoxGeometry".into()),
                    module_path: Some("./createBoxGeometry.js".into()),
                }],
                "wire" => vec![ready(Some(wire_geometry(u(&a[0]) as usize)))],
                _ => return None,
            };
            match block_on(create_geometry(tasks)) {
                Ok(packed) => {
                    let unpacked = packed.unpack().unwrap().into_iter().map(geometry_enc).collect::<Vec<_>>();
                    ok(
                        json!({"stringTable":packed.string_table,"packedData":packed.packed_data.enc(),"transferCount":1,"unpacked":unpacked}),
                    )
                }
                Err(e) => Some(Out::Err(e.message)),
            }
        },
    );
}

#[test]
fn create_box_geometry_reference() {
    run("workers", "createBoxGeometry", |_| GEOMETRY, |_, a| replay::<BoxGeometry>(a, create_box_geometry));
}

#[test]
fn create_box_outline_geometry_reference() {
    run(
        "workers",
        "createBoxOutlineGeometry",
        |_| GEOMETRY,
        |_, a| replay::<BoxOutlineGeometry>(a, create_box_outline_geometry),
    );
}

#[test]
fn create_circle_geometry_reference() {
    run("workers", "createCircleGeometry", |_| GEOMETRY, |_, a| replay::<CircleGeometry>(a, create_circle_geometry));
}

#[test]
fn create_circle_outline_geometry_reference() {
    run(
        "workers",
        "createCircleOutlineGeometry",
        |_| GEOMETRY,
        |_, a| replay::<CircleOutlineGeometry>(a, create_circle_outline_geometry),
    );
}

#[test]
fn create_coplanar_polygon_geometry_reference() {
    run(
        "workers",
        "createCoplanarPolygonGeometry",
        |_| GEOMETRY,
        |_, a| replay::<CoplanarPolygonGeometry>(a, create_coplanar_polygon_geometry),
    );
}

#[test]
fn create_coplanar_polygon_outline_geometry_reference() {
    run(
        "workers",
        "createCoplanarPolygonOutlineGeometry",
        |_| GEOMETRY,
        |_, a| replay::<CoplanarPolygonOutlineGeometry>(a, create_coplanar_polygon_outline_geometry),
    );
}

#[test]
fn create_corridor_geometry_reference() {
    run(
        "workers",
        "createCorridorGeometry",
        |_| GEOMETRY,
        |_, a| replay::<CorridorGeometry>(a, create_corridor_geometry),
    );
}

#[test]
fn create_corridor_outline_geometry_reference() {
    run(
        "workers",
        "createCorridorOutlineGeometry",
        |_| GEOMETRY,
        |_, a| replay::<CorridorOutlineGeometry>(a, create_corridor_outline_geometry),
    );
}

#[test]
fn create_cylinder_geometry_reference() {
    run(
        "workers",
        "createCylinderGeometry",
        |_| GEOMETRY,
        |_, a| replay::<CylinderGeometry>(a, create_cylinder_geometry),
    );
}

#[test]
fn create_cylinder_outline_geometry_reference() {
    run(
        "workers",
        "createCylinderOutlineGeometry",
        |_| GEOMETRY,
        |_, a| replay::<CylinderOutlineGeometry>(a, create_cylinder_outline_geometry),
    );
}

#[test]
fn create_ellipse_geometry_reference() {
    run("workers", "createEllipseGeometry", |_| GEOMETRY, |_, a| replay::<EllipseGeometry>(a, create_ellipse_geometry));
}

#[test]
fn create_ellipse_outline_geometry_reference() {
    run(
        "workers",
        "createEllipseOutlineGeometry",
        |_| GEOMETRY,
        |_, a| replay::<EllipseOutlineGeometry>(a, create_ellipse_outline_geometry),
    );
}

#[test]
fn create_ellipsoid_geometry_reference() {
    run(
        "workers",
        "createEllipsoidGeometry",
        |_| GEOMETRY,
        |_, a| replay::<EllipsoidGeometry>(a, create_ellipsoid_geometry),
    );
}

#[test]
fn create_ellipsoid_outline_geometry_reference() {
    run(
        "workers",
        "createEllipsoidOutlineGeometry",
        |_| GEOMETRY,
        |_, a| replay::<EllipsoidOutlineGeometry>(a, create_ellipsoid_outline_geometry),
    );
}

#[test]
fn create_frustum_geometry_reference() {
    run("workers", "createFrustumGeometry", |_| GEOMETRY, |_, a| replay::<FrustumGeometry>(a, create_frustum_geometry));
}

#[test]
fn create_frustum_outline_geometry_reference() {
    run(
        "workers",
        "createFrustumOutlineGeometry",
        |_| GEOMETRY,
        |_, a| replay::<FrustumOutlineGeometry>(a, create_frustum_outline_geometry),
    );
}

#[test]
fn create_ground_polyline_geometry_reference() {
    run(
        "workers",
        "createGroundPolylineGeometry",
        |_| GEOMETRY,
        |_, a| replay::<GroundPolylineGeometry>(a, create_ground_polyline_geometry),
    );
}

#[test]
fn create_plane_geometry_reference() {
    run("workers", "createPlaneGeometry", |_| GEOMETRY, |_, a| replay::<PlaneGeometry>(a, create_plane_geometry));
}

#[test]
fn create_plane_outline_geometry_reference() {
    run(
        "workers",
        "createPlaneOutlineGeometry",
        |_| GEOMETRY,
        |_, a| replay::<PlaneOutlineGeometry>(a, create_plane_outline_geometry),
    );
}

#[test]
fn create_polygon_geometry_reference() {
    run("workers", "createPolygonGeometry", |_| GEOMETRY, |_, a| replay::<PolygonGeometry>(a, create_polygon_geometry));
}

#[test]
fn create_polygon_outline_geometry_reference() {
    run(
        "workers",
        "createPolygonOutlineGeometry",
        |_| GEOMETRY,
        |_, a| replay::<PolygonOutlineGeometry>(a, create_polygon_outline_geometry),
    );
}

#[test]
fn create_polyline_geometry_reference() {
    run(
        "workers",
        "createPolylineGeometry",
        |_| GEOMETRY,
        |_, a| replay::<PolylineGeometry>(a, create_polyline_geometry),
    );
}

#[test]
fn create_polyline_volume_geometry_reference() {
    run(
        "workers",
        "createPolylineVolumeGeometry",
        |_| GEOMETRY,
        |_, a| replay::<PolylineVolumeGeometry>(a, create_polyline_volume_geometry),
    );
}

#[test]
fn create_polyline_volume_outline_geometry_reference() {
    run(
        "workers",
        "createPolylineVolumeOutlineGeometry",
        |_| GEOMETRY,
        |_, a| replay::<PolylineVolumeOutlineGeometry>(a, create_polyline_volume_outline_geometry),
    );
}

#[test]
fn create_rectangle_geometry_reference() {
    run(
        "workers",
        "createRectangleGeometry",
        |_| GEOMETRY,
        |_, a| replay::<RectangleGeometry>(a, create_rectangle_geometry),
    );
}

#[test]
fn create_rectangle_outline_geometry_reference() {
    run(
        "workers",
        "createRectangleOutlineGeometry",
        |_| GEOMETRY,
        |_, a| replay::<RectangleOutlineGeometry>(a, create_rectangle_outline_geometry),
    );
}

#[test]
fn create_simple_polyline_geometry_reference() {
    run(
        "workers",
        "createSimplePolylineGeometry",
        |_| GEOMETRY,
        |_, a| replay::<SimplePolylineGeometry>(a, create_simple_polyline_geometry),
    );
}

#[test]
fn create_sphere_geometry_reference() {
    run("workers", "createSphereGeometry", |_| GEOMETRY, |_, a| replay::<SphereGeometry>(a, create_sphere_geometry));
}

#[test]
fn create_sphere_outline_geometry_reference() {
    run(
        "workers",
        "createSphereOutlineGeometry",
        |_| GEOMETRY,
        |_, a| replay::<SphereOutlineGeometry>(a, create_sphere_outline_geometry),
    );
}

#[test]
fn create_wall_geometry_reference() {
    run("workers", "createWallGeometry", |_| GEOMETRY, |_, a| replay::<WallGeometry>(a, create_wall_geometry));
}

#[test]
fn create_wall_outline_geometry_reference() {
    run(
        "workers",
        "createWallOutlineGeometry",
        |_| GEOMETRY,
        |_, a| replay::<WallOutlineGeometry>(a, create_wall_outline_geometry),
    );
}

#[test]
fn concurrent_custom_modules_preserve_order_and_reuse_factories() {
    use futures_channel::oneshot;
    use std::{
        cell::{Cell, RefCell},
        rc::Rc,
    };
    let started = Rc::new(Cell::new(0));
    let (tx0, rx0) = oneshot::channel();
    let (tx1, rx1) = oneshot::channel();
    let receivers = Rc::new(RefCell::new(std::collections::VecDeque::from([rx0, rx1])));
    let mut worker = GeometryWorker::default();
    worker.register(
        "custom/async",
        Rc::new({
            let started = started.clone();
            move |_| {
                started.set(started.get() + 1);
                let receiver = receivers.borrow_mut().pop_front();
                Box::pin(async move {
                    if let Some(receiver) = receiver {
                        receiver.await.map_err(|_| RuntimeError::new("cancelled"))
                    } else {
                        Ok(None)
                    }
                })
            }
        }),
    );
    let custom = || GeometrySubTask {
        geometry: GeometrySource::Ready(None),
        module_name: None,
        module_path: Some("custom/async".into()),
    };
    let mut future = Box::pin(worker.create_geometry(vec![custom(), custom()]));
    let waker = futures_util::task::noop_waker();
    let mut cx = Context::from_waker(&waker);
    assert!(future.as_mut().poll(&mut cx).is_pending());
    assert_eq!(started.get(), 2, "all subtasks start before either finishes");
    tx1.send(Some(box_geometry())).unwrap();
    assert!(future.as_mut().poll(&mut cx).is_pending());
    tx0.send(None).unwrap();
    let result = block_on(future).unwrap().unpack().unwrap();
    assert_eq!(result, vec![None, Some(box_geometry())]);
    assert_eq!(block_on(worker.create_geometry(vec![custom()])).unwrap().unpack().unwrap(), vec![None]);
    assert_eq!(started.get(), 3);
}

#[test]
fn failed_or_unknown_modules_reject_the_batch() {
    use std::{cell::Cell, rc::Rc};
    let calls = Rc::new(Cell::new(0));
    let mut worker = GeometryWorker::default();
    worker.register(
        "fail",
        Rc::new({
            let calls = calls.clone();
            move |_| {
                calls.set(calls.get() + 1);
                Box::pin(async { Err(RuntimeError::new("module failed")) })
            }
        }),
    );
    worker.register("pending", Rc::new(|_| Box::pin(std::future::pending())));
    let task = |name: &str| GeometrySubTask {
        geometry: GeometrySource::Ready(None),
        module_name: Some(name.into()),
        module_path: None,
    };
    assert_eq!(
        block_on(worker.create_geometry(vec![task("fail"), task("fail")])).unwrap_err().message,
        "module failed"
    );
    assert_eq!(calls.get(), 2);
    assert_eq!(
        block_on(worker.create_geometry(vec![task("pending"), task("fail")])).unwrap_err().message,
        "module failed"
    );
    assert!(block_on(worker.create_geometry(vec![task("missing")])).unwrap_err().message.contains("missing"));
    assert!(block_on(worker.create_geometry(vec![task("createBoxGeometry")])).is_err());
}

#[test]
fn packed_results_reject_corrupt_lengths_and_preserve_cv_bounds() {
    let geometry = wire_geometry(3);
    let packed = PackedGeometryResults::pack(&[Some(geometry.clone()), None]);
    assert_eq!(packed.unpack().unwrap(), vec![Some(geometry), None]);
    for end in 0..packed.packed_data.len() {
        let broken = PackedGeometryResults {
            string_table: packed.string_table.clone(),
            packed_data: packed.packed_data[..end].to_vec(),
        };
        assert!(broken.unpack().is_err(), "truncation at {end}");
    }
    for number in [f64::NAN, f64::INFINITY, -1.0, 1e30, 0.5] {
        assert!(PackedGeometryResults { string_table: vec![], packed_data: vec![number] }.unpack().is_err());
    }
    for (index, value) in [(2, 999.0), (3, 999.0), (4, 999.0), (16, 900.0), (17, 999.0), (18, 0.0), (20, 1e20)] {
        let mut broken = packed.clone();
        broken.packed_data[index] = value;
        assert!(broken.unpack().is_err(), "corrupt field {index}");
    }
    let mut broken = packed.clone();
    broken.string_table.clear();
    assert!(broken.unpack().is_err());
    let mut broken = packed;
    broken.packed_data.push(0.0);
    assert!(broken.unpack().is_err());
}

#[cfg(feature = "native-host")]
#[test]
fn geometry_batches_run_through_native_task_processor() {
    use std::rc::Rc;
    let worker = cesium_rs::core::native_host::NativeTaskWorker::new(
        |tasks, _| block_on(create_geometry(tasks)),
        |_| Ok(Value::Null),
    );
    let mut processor = TaskProcessor::new(Rc::new(worker), Some(1));
    let description = GeometryDescription::BoxGeometry(
        BoxGeometry::from_dimensions(Cartesian3::new(2.0, 4.0, 6.0), VertexFormat::ALL, None).unwrap(),
    );
    let task = GeometrySubTask {
        module_name: Some(description.module_name().into()),
        module_path: None,
        geometry: GeometrySource::Description(Box::new(description.clone())),
    };
    let result = block_on(processor.schedule_task(vec![task]).unwrap()).unwrap();
    assert_eq!(result.unpack().unwrap(), vec![description.create_geometry()]);
    processor.destroy();
    assert!(processor.schedule_task(vec![]).is_none());
}

#[test]
fn reference_geometry_metadata_indices_and_double_precision() {
    fn numbers(actual: &Value, expected: &Value, tol: Tol) {
        let actual = nums(actual);
        let expected = nums(expected);
        assert_eq!(actual.len(), expected.len());
        for (index, (a, b)) in actual.into_iter().zip(expected).enumerate() {
            assert!(
                (a - b).abs() <= tol.abs.max(tol.rel * a.abs().max(b.abs())) || (a.is_nan() && b.is_nan()),
                "component {index}: {a} != {b}"
            );
        }
    }
    for &module in GeometryDescription::MODULE_NAMES {
        for (index, case) in load("workers", module).iter().enumerate() {
            let values = nums(&case.a[0]);
            let offset = opt(&case.a[1]).unwrap_or(0.0) as usize;
            let actual =
                geometry_enc(GeometryDescription::unpack(module, &values, offset).and_then(|d| d.create_geometry()));
            let expected = &case.r;
            assert_eq!(actual.is_null(), expected.is_null(), "{module} #{index}");
            if actual.is_null() {
                continue;
            }
            for field in ["primitiveType", "geometryType", "offsetAttribute"] {
                assert_eq!(actual[field], expected[field], "{module} #{index} {field}");
            }
            assert_eq!(actual["indices"].is_null(), expected["indices"].is_null());
            if !actual["indices"].is_null() {
                assert_eq!(actual["indices"][0], expected["indices"][0]);
                numbers(&actual["indices"][1], &expected["indices"][1], EXACT);
            }
            let attrs = actual["attributes"].as_object().unwrap();
            assert_eq!(attrs.len(), expected["attributes"].as_object().unwrap().len());
            for (name, attribute) in attrs {
                let reference = &expected["attributes"][name];
                for field in 0..4 {
                    if field < 2 {
                        assert_eq!(n(&attribute[field]), n(&reference[field]));
                    } else {
                        assert_eq!(attribute[field], reference[field]);
                    }
                }
                let tolerance = if s(&attribute[3]) == "Float32Array" {
                    GEOMETRY
                } else if s(&attribute[3]) == "Float64Array" {
                    Tol { rel: 1e-12, abs: 1e-7, vector: false }
                } else {
                    EXACT
                };
                numbers(&attribute[4], &reference[4], tolerance);
            }
            numbers(
                &actual["boundingSphere"],
                &expected["boundingSphere"],
                Tol { rel: 1e-12, abs: 1e-7, vector: false },
            );
        }
    }
}

#[test]
fn conflicting_module_selectors_are_rejected() {
    let task = GeometrySubTask {
        geometry: GeometrySource::Ready(None),
        module_name: Some("createBoxGeometry".into()),
        module_path: Some("./createBoxGeometry.js".into()),
    };
    assert_eq!(block_on(create_geometry(vec![task])).unwrap_err().message, "Must only set moduleName or modulePath");
}
