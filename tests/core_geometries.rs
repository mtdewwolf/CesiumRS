//! VertexFormat and the box, plane, ellipsoid, sphere, cylinder, and frustum
//! geometries against CesiumJS fixtures.

mod common;

use cesium_rs::core::*;
use common::*;
use serde_json::{json, Map, Value};

/// Float32 attributes can land one f32 step apart when the f64 inputs differ
/// in the last bit; 1e-7 relative covers that and nothing larger.
const GEOMETRY: Tol = Tol { rel: 1e-7, abs: 1e-9, vector: false };

fn typed_array_name(a: &TypedArray) -> &'static str {
    match a {
        TypedArray::Int8(_) => "Int8Array",
        TypedArray::Uint8(_) => "Uint8Array",
        TypedArray::Int16(_) => "Int16Array",
        TypedArray::Uint16(_) => "Uint16Array",
        TypedArray::Int32(_) => "Int32Array",
        TypedArray::Uint32(_) => "Uint32Array",
        TypedArray::Float32(_) => "Float32Array",
        TypedArray::Float64(_) => "Float64Array",
    }
}

pub fn geometry_enc(g: Option<Geometry>) -> Value {
    let Some(g) = g else { return Value::Null };
    let mut attributes = Map::new();
    for (name, a) in g.attributes.iter() {
        attributes.insert(
            name.to_string(),
            json!([
                a.component_datatype as u32,
                a.components_per_attribute,
                a.normalize,
                typed_array_name(&a.values),
                a.values.to_f64_vec().enc()
            ]),
        );
    }
    json!({
        "attributes": attributes,
        "indices": g.indices.as_ref().map(|i| json!([typed_array_name(i), i.to_f64_vec().enc()])),
        "primitiveType": g.primitive_type as u32,
        "boundingSphere": g.bounding_sphere.enc(),
        "geometryType": g.geometry_type as u32,
        "offsetAttribute": g.offset_attribute.map(|o| o as u32),
    })
}

fn format(v: &Value) -> VertexFormat {
    match s(v) {
        "POSITION_ONLY" => VertexFormat::POSITION_ONLY,
        "POSITION_AND_NORMAL" => VertexFormat::POSITION_AND_NORMAL,
        "POSITION_NORMAL_AND_ST" => VertexFormat::POSITION_NORMAL_AND_ST,
        "POSITION_AND_ST" => VertexFormat::POSITION_AND_ST,
        "ALL" => VertexFormat::ALL,
        "NONE" => VertexFormat::default(),
        "TANGENT_ONLY" => VertexFormat { position: true, tangent: true, ..Default::default() },
        _ => VertexFormat { position: true, bitangent: true, ..Default::default() },
    }
}

fn offset(v: &Value) -> Option<GeometryOffsetAttribute> {
    opt(v).and_then(GeometryOffsetAttribute::unpack)
}

/// `[geometry, packed]`, packing into a zeroed array as Cesium's fixtures do.
fn with_pack(geometry: Option<Geometry>, length: usize, pack: impl Fn(&mut [f64])) -> Option<Out> {
    let mut array = vec![0.0; length];
    pack(&mut array);
    ok(json!([geometry_enc(geometry), array.enc()]))
}

#[test]
fn box_and_plane_geometry() {
    run(
        "core",
        "BoxPlaneGeometry",
        |_| GEOMETRY,
        |f, a| match f {
            "box" => {
                let b = BoxGeometry::new(c3(&a[0]), c3(&a[1]), format(&a[2]), offset(&a[3])).unwrap();
                with_pack(b.create_geometry(), BoxGeometry::PACKED_LENGTH, |arr| b.pack(arr, 0))
            }
            "boxOutline" => {
                let b = BoxOutlineGeometry::new(c3(&a[0]), c3(&a[1]), offset(&a[2])).unwrap();
                with_pack(b.create_geometry(), BoxOutlineGeometry::PACKED_LENGTH, |arr| b.pack(arr, 0))
            }
            "boxFromDimensions" => {
                let b = BoxGeometry::from_dimensions(c3(&a[0]), format(&a[1]), None).unwrap();
                ok(geometry_enc(b.create_geometry()))
            }
            "plane" => {
                let p = PlaneGeometry::new(format(&a[0]));
                with_pack(Some(p.create_geometry()), PlaneGeometry::PACKED_LENGTH, |arr| p.pack(arr, 0))
            }
            "planeOutline" => ok(geometry_enc(Some(PlaneOutlineGeometry.create_geometry()))),
            _ => None,
        },
    );
}

#[test]
fn ellipsoid_geometry() {
    run(
        "core",
        "EllipsoidGeometry",
        |_| GEOMETRY,
        |f, a| {
            let inner = || (!a[1].is_null()).then(|| c3(&a[1]));
            let x = |i: usize| n(&a[i]);
            match f {
                "ellipsoid" => {
                    let e = EllipsoidGeometry::new(
                        c3(&a[0]),
                        inner(),
                        x(2),
                        x(3),
                        x(4),
                        x(5),
                        x(6),
                        x(7),
                        format(&a[8]),
                        offset(&a[9]),
                    )
                    .unwrap();
                    with_pack(e.create_geometry(), EllipsoidGeometry::PACKED_LENGTH, |arr| e.pack(arr, 0))
                }
                "ellipsoidOutline" => {
                    let e = EllipsoidOutlineGeometry::new(
                        c3(&a[0]),
                        inner(),
                        x(2),
                        x(3),
                        x(4),
                        x(5),
                        x(6),
                        x(7),
                        x(8),
                        offset(&a[9]),
                    )
                    .unwrap();
                    with_pack(e.create_geometry(), EllipsoidOutlineGeometry::PACKED_LENGTH, |arr| e.pack(arr, 0))
                }
                "sphere" => {
                    let sp = SphereGeometry::new(x(0), x(1), x(2), format(&a[3])).unwrap();
                    with_pack(sp.create_geometry(), SphereGeometry::PACKED_LENGTH, |arr| sp.pack(arr, 0))
                }
                "sphereOutline" => {
                    let sp = SphereOutlineGeometry::new(x(0), x(1), x(2), x(3)).unwrap();
                    with_pack(sp.create_geometry(), SphereOutlineGeometry::PACKED_LENGTH, |arr| sp.pack(arr, 0))
                }
                _ => None,
            }
        },
    );
}

#[test]
fn cylinder_geometry() {
    run(
        "core",
        "CylinderGeometry",
        |_| GEOMETRY,
        |f, a| {
            let x = |i: usize| n(&a[i]);
            match f {
                "cylinder" => {
                    let c =
                        CylinderGeometry::new(x(0), x(1), x(2), x(3) as usize, format(&a[4]), offset(&a[5])).unwrap();
                    with_pack(c.create_geometry(), CylinderGeometry::PACKED_LENGTH, |arr| c.pack(arr, 0))
                }
                "cylinderOutline" => {
                    let c = CylinderOutlineGeometry::new(x(0), x(1), x(2), x(3) as usize, x(4), offset(&a[5])).unwrap();
                    with_pack(c.create_geometry(), CylinderOutlineGeometry::PACKED_LENGTH, |arr| c.pack(arr, 0))
                }
                _ => None,
            }
        },
    );
}

fn frustum(perspective: bool, p: &[f64]) -> Frustum {
    if perspective {
        let mut f = PerspectiveFrustum::new(p[0], p[1]);
        f.near = p[2];
        f.far = p[3];
        f.x_offset = p[4];
        f.y_offset = p[5];
        Frustum::Perspective(f)
    } else {
        let mut f = OrthographicFrustum::new(p[0], p[1]);
        f.near = p[2];
        f.far = p[3];
        Frustum::Orthographic(f)
    }
}

#[test]
fn frustum_geometry() {
    run(
        "core",
        "FrustumGeometry",
        |_| GEOMETRY,
        |f, a| {
            let fr = frustum(a[0].as_bool().unwrap(), &nums(&a[1]));
            let draw_near_plane = a[4].as_bool().unwrap();
            match f {
                "frustum" => {
                    let mut g = FrustumGeometry::new(fr, c3(&a[2]), q(&a[3]), VertexFormat::POSITION_ONLY);
                    g.draw_near_plane = draw_near_plane;
                    with_pack(g.create_geometry(), g.packed_length(), |arr| g.pack(arr, 0))
                }
                "frustumOutline" => {
                    let mut g = FrustumOutlineGeometry::new(fr, c3(&a[2]), q(&a[3]));
                    g.draw_near_plane = draw_near_plane;
                    with_pack(g.create_geometry(), g.packed_length(), |arr| g.pack(arr, 0))
                }
                _ => None,
            }
        },
    );
}

#[test]
fn vertex_format() {
    run(
        "core",
        "VertexFormat",
        |_| EXACT,
        |f, a| match f {
            "pack" => {
                let mut array = vec![0.0; 6];
                format(&a[0]).pack(&mut array, 0);
                ok(array)
            }
            _ => None,
        },
    );
}

#[test]
fn unpack_round_trips() {
    let b = BoxGeometry::new(
        Cartesian3::new(-1.0, -2.0, -3.0),
        Cartesian3::new(4.0, 5.0, 6.0),
        VertexFormat::ALL,
        Some(GeometryOffsetAttribute::All),
    )
    .unwrap();
    let mut array = vec![0.0; BoxGeometry::PACKED_LENGTH + 2];
    b.pack(&mut array, 2);
    assert_eq!(BoxGeometry::unpack(&array, 2), b);
    let e = EllipsoidGeometry::default();
    let mut array = vec![0.0; EllipsoidGeometry::PACKED_LENGTH];
    e.pack(&mut array, 0);
    assert_eq!(EllipsoidGeometry::unpack(&array, 0), Some(e));
    let c = CylinderOutlineGeometry::new(3.0, 1.0, 2.0, 12, 5.0, None).unwrap();
    let mut array = vec![0.0; CylinderOutlineGeometry::PACKED_LENGTH];
    c.pack(&mut array, 0);
    assert_eq!(CylinderOutlineGeometry::unpack(&array, 0), Some(c));
    let fg = FrustumGeometry::new(
        frustum(true, &[1.0, 1.5, 1.0, 100.0, 0.0, 0.0]),
        Cartesian3::new(1.0, 2.0, 3.0),
        Quaternion::IDENTITY,
        VertexFormat::ALL,
    );
    let mut array = vec![0.0; fg.packed_length()];
    fg.pack(&mut array, 0);
    assert_eq!(FrustumGeometry::unpack(&array, 0), fg);
}

#[test]
fn ellipse_outline_geometry() {
    use cesium_rs::core::ellipse_geometry_library::{compute_ellipse_positions, EllipseOptions};
    run(
        "core",
        "EllipseOutlineGeometry",
        |_| TIGHT_VECTOR,
        |f, a| {
            let x = |i: usize| n(&a[i]);
            match f {
                "ellipseOutline" => {
                    let mut options = EllipseOutlineOptions::new(c3(&a[0]), x(1), x(2));
                    options.rotation = x(3);
                    options.granularity = x(4);
                    options.height = x(5);
                    options.extruded_height = opt(&a[6]);
                    options.number_of_vertical_lines = x(7);
                    options.offset_attribute = offset(&a[8]);
                    let e = EllipseOutlineGeometry::new(options).unwrap();
                    if x(1) == 0.0 {
                        // The degenerate case's fixture has no packing.
                        return ok(json!([geometry_enc(e.create_geometry()), null]));
                    }
                    with_pack(e.create_geometry(), EllipseOutlineGeometry::PACKED_LENGTH, |arr| e.pack(arr, 0))
                }
                "circleOutline" => {
                    let mut options = EllipseOutlineOptions::new(c3(&a[0]), 0.0, 0.0);
                    options.granularity = x(2);
                    options.height = x(3);
                    options.extruded_height = opt(&a[4]);
                    options.number_of_vertical_lines = x(5);
                    let c = CircleOutlineGeometry::new(x(1), options).unwrap();
                    with_pack(c.create_geometry(), CircleOutlineGeometry::PACKED_LENGTH, |arr| c.pack(arr, 0))
                }
                "computeEllipsePositions" => {
                    let p = compute_ellipse_positions(
                        &EllipseOptions {
                            center: c3(&a[0]),
                            semi_major_axis: x(1),
                            semi_minor_axis: x(2),
                            rotation: x(3),
                            granularity: x(4),
                        },
                        true,
                        true,
                    );
                    ok(json!([p.positions.enc(), p.num_pts.map(|v| v as f64).enc(), p.outer_positions.enc()]))
                }
                _ => None,
            }
        },
    );
}

#[test]
fn large_ellipsoids_use_32_bit_indices() {
    // 254 partitions: 66,049 vertices but an index count of 65,025. CesiumJS
    // picks 16-bit indices from the latter and wraps them.
    let e = EllipsoidGeometry::new(
        Cartesian3::new(1.0, 1.0, 1.0),
        None,
        0.0,
        std::f64::consts::TAU,
        0.0,
        std::f64::consts::PI,
        254.0,
        254.0,
        VertexFormat::POSITION_ONLY,
        None,
    )
    .unwrap();
    let g = e.create_geometry().unwrap();
    let vertices = g.attributes.position.as_ref().unwrap().values.len() / 3;
    assert_eq!(vertices, 66_049);
    let indices = g.indices.unwrap();
    assert!(matches!(indices, TypedArray::Uint32(_)));
    assert!(indices.to_f64_vec().iter().any(|&i| i >= 65_536.0));
    assert!(indices.to_f64_vec().iter().all(|&i| (i as usize) < vertices));
}

#[test]
fn cylinder_outlines_need_three_slices() {
    assert!(CylinderOutlineGeometry::new(1.0, 1.0, 1.0, 2, 16.0, None).is_none());
    assert!(CylinderOutlineGeometry::new(1.0, 1.0, 1.0, 0, 16.0, None).is_none());
    assert!(CylinderOutlineGeometry::new(1.0, 1.0, 1.0, 3, 16.0, None).is_some());
}
