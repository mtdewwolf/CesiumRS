//! Hermite and Catmull-Rom splines, the tridiagonal solver, easing, heaps,
//! polynomial approximations, 2D intersections, Tipsify, wireframe indices,
//! datatypes, Stereographic, and tiling schemes against CesiumJS fixtures.

mod common;

use cesium_rs::core::component_datatype::ComponentDatatype;
use cesium_rs::core::*;
use cesium_rs::core::{
    hermite_polynomial_approximation as hermite_approx, intersections_2d,
    lagrange_polynomial_approximation as lagrange, linear_approximation as linear, tipsify, tridiagonal_system_solver,
    wireframe_index_generator as wireframe,
};
use common::*;
use serde_json::{json, Value};

fn b(v: &Value) -> bool {
    v.as_bool().unwrap()
}

fn evaluate_all<S: Spline>(spline: &S, queries: &Value) -> Vec<Option<S::Value>> {
    list(queries, |t| spline.evaluate(n(t)))
}

#[test]
fn hermite_spline() {
    run(
        "core",
        "HermiteSpline",
        |_| TIGHT_VECTOR,
        |f, a| {
            let t = || nums(&a[0]);
            macro_rules! kinds {
                ($kind:expr, $body:ident) => {
                    match $kind {
                        "scalar" => $body!(n),
                        "cartesian" => $body!(c3),
                        _ => $body!(q),
                    }
                };
            }
            let (name, kind) = f.split_once(':').unwrap_or((f, ""));
            match name {
                "new" => {
                    macro_rules! go {
                        ($d:expr) => {{
                            let s = HermiteSpline::new(t(), list(&a[1], $d), list(&a[2], $d), list(&a[3], $d)).unwrap();
                            ok(evaluate_all(&s, &a[4]))
                        }};
                    }
                    kinds!(kind, go)
                }
                "createC1" => {
                    macro_rules! go {
                        ($d:expr) => {{
                            let s = HermiteSpline::create_c1(t(), list(&a[1], $d), list(&a[2], $d)).unwrap();
                            ok(evaluate_all(&s, &a[3]))
                        }};
                    }
                    kinds!(kind, go)
                }
                "createNaturalCubic" | "createClampedCubic" => {
                    let spline = if name == "createNaturalCubic" {
                        HermiteSpline::create_natural_cubic(t(), list(&a[1], c3))
                    } else {
                        HermiteSpline::create_clamped_cubic(t(), list(&a[1], c3), c3(&a[2]), c3(&a[3]))
                    }
                    .unwrap();
                    let queries = if name == "createNaturalCubic" { &a[2] } else { &a[4] };
                    let evaluated = evaluate_all(&spline, queries);
                    ok(match &spline {
                        CubicSpline::Hermite(h) => json!([
                            "hermite",
                            h.in_tangents().to_vec().enc(),
                            h.out_tangents().to_vec().enc(),
                            evaluated.enc()
                        ]),
                        CubicSpline::Linear(_) => json!(["linear", null, null, evaluated.enc()]),
                    })
                }
                _ => None,
            }
        },
    );
}

#[test]
fn catmull_rom_spline() {
    run(
        "core",
        "CatmullRomSpline",
        |_| TIGHT_VECTOR,
        |f, a| match f {
            "evaluate" => {
                let tangent = |v: &Value| (!v.is_null()).then(|| c3(v));
                let s = CatmullRomSpline::new(nums(&a[0]), list(&a[1], c3), tangent(&a[2]), tangent(&a[3])).unwrap();
                ok(json!([s.first_tangent().enc(), s.last_tangent().enc(), evaluate_all(&s, &a[4]).enc()]))
            }
            _ => None,
        },
    );
}

#[test]
fn tridiagonal_system_solver() {
    run(
        "core",
        "TridiagonalSystemSolver",
        |_| TIGHT_VECTOR,
        |f, a| match f {
            "solve" => ok(tridiagonal_system_solver::solve(&nums(&a[0]), &nums(&a[1]), &nums(&a[2]), &list(&a[3], c3))),
            _ => None,
        },
    );
}

#[test]
fn easing_function() {
    run(
        "core",
        "EasingFunction",
        |_| TIGHT,
        |f, a| match f {
            "evaluate" => {
                let easing = EasingFunction::ALL.into_iter().find(|e| e.name() == s(&a[0])).unwrap();
                ok(list(&a[1], |x| easing.evaluate(n(x))))
            }
            _ => None,
        },
    );
}

fn heap_value(v: Option<f64>) -> Value {
    v.enc()
}

#[test]
fn heap() {
    run(
        "core",
        "Heap",
        |_| EXACT,
        |f, a| match f {
            "ops" => {
                let mut h = Heap::new(|x: &f64, y: &f64| x.partial_cmp(y).unwrap());
                let mut out = Vec::new();
                for op in a[0].as_array().unwrap() {
                    let v = n(&op[1]);
                    match s(&op[0]) {
                        "insert" => out.push(heap_value(h.insert(v))),
                        "pop" => out.push(heap_value(h.pop(0))),
                        "popMax" => out.push(heap_value(if h.len() > 2 { h.pop(2) } else { None })),
                        "max" => {
                            if v >= 0.0 {
                                h.set_maximum_length(Some(v as usize));
                            }
                        }
                        _ => {
                            if !h.is_empty() {
                                let last = h.len() - 1;
                                h.internal_array_mut()[last] = v;
                                h.resort();
                            }
                        }
                    }
                    out.push(h.internal_array().to_vec().enc());
                }
                ok(out)
            }
            _ => None,
        },
    );
}

#[test]
fn double_ended_priority_queue() {
    run(
        "core",
        "DoubleEndedPriorityQueue",
        |_| EXACT,
        |f, a| match f {
            "ops" => {
                let max = n(&a[1]);
                let mut q =
                    DoubleEndedPriorityQueue::new(|x: &f64, y: &f64| x - y, (max >= 0.0).then_some(max as usize));
                let mut out = Vec::new();
                for op in a[0].as_array().unwrap() {
                    let v = n(&op[1]);
                    match s(&op[0]) {
                        "insert" => out.push(heap_value(q.insert(v))),
                        "pop" => out.push(heap_value(q.remove_minimum())),
                        "popMax" => out.push(heap_value(q.remove_maximum())),
                        "max" => q.set_maximum_length((v >= 0.0).then_some(v as usize)),
                        _ => {
                            if !q.is_empty() {
                                let last = q.len() - 1;
                                q.internal_array_mut()[last] = v;
                                q.resort();
                            }
                        }
                    }
                    out.push(json!([
                        q.internal_array().to_vec().enc(),
                        q.get_minimum().copied().enc(),
                        q.get_maximum().copied().enc()
                    ]));
                }
                ok(out)
            }
            _ => None,
        },
    );
}

#[test]
fn polynomial_approximation() {
    run(
        "core",
        "PolynomialApproximation",
        |_| TIGHT,
        |f, a| {
            let x = || n(&a[0]);
            let stride = || n(&a[3]) as usize;
            match f {
                "lagrange" => ok(lagrange::interpolate_order_zero(x(), &nums(&a[1]), &nums(&a[2]), stride())),
                "hermiteOrderZero" => {
                    ok(hermite_approx::interpolate_order_zero(x(), &nums(&a[1]), &nums(&a[2]), stride()))
                }
                "linear" => ok(linear::interpolate_order_zero(x(), &nums(&a[1]), &nums(&a[2]), stride())),
                "hermite" => ok(hermite_approx::interpolate(
                    x(),
                    &nums(&a[1]),
                    &nums(&a[2]),
                    stride(),
                    n(&a[4]) as usize,
                    n(&a[5]) as usize,
                )),
                "requiredLagrange" => ok(lagrange::get_required_data_points(n(&a[0]))),
                "requiredHermite" => ok(hermite_approx::get_required_data_points(n(&a[0]), n(&a[1]))),
                _ => None,
            }
        },
    );
}

#[test]
fn intersections_2d() {
    run(
        "core",
        "Intersections2D",
        |_| TIGHT,
        |f, a| {
            let x = |i: usize| n(&a[i]);
            match f {
                "clipTriangleAtAxisAlignedThreshold" => {
                    ok(intersections_2d::clip_triangle_at_axis_aligned_threshold(x(0), b(&a[1]), x(2), x(3), x(4)))
                }
                "computeBarycentricCoordinates" => ok(intersections_2d::compute_barycentric_coordinates(
                    x(0),
                    x(1),
                    x(2),
                    x(3),
                    x(4),
                    x(5),
                    x(6),
                    x(7),
                )),
                "computeLineSegmentLineSegmentIntersection" => {
                    ok(intersections_2d::compute_line_segment_line_segment_intersection(
                        x(0),
                        x(1),
                        x(2),
                        x(3),
                        x(4),
                        x(5),
                        x(6),
                        x(7),
                    ))
                }
                _ => None,
            }
        },
    );
}

fn indices(v: &Value) -> Vec<u32> {
    list(v, |x| n(x) as u32)
}

#[test]
fn tipsify() {
    run(
        "core",
        "Tipsify",
        |_| TIGHT,
        |f, a| {
            let max = opt(&a[1]).map(|m| m as u32);
            let cache = n(&a[2]) as usize;
            match f {
                "tipsify" => ok(tipsify::tipsify(&indices(&a[0]), max, cache)
                    .unwrap()
                    .iter()
                    .map(|&i| f64::from(i))
                    .collect::<Vec<_>>()),
                "calculateACMR" => ok(tipsify::calculate_acmr(&indices(&a[0]), max, cache).unwrap()),
                _ => None,
            }
        },
    );
}

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

fn primitive(name: &str) -> PrimitiveType {
    match name {
        "TRIANGLES" => PrimitiveType::Triangles,
        "TRIANGLE_STRIP" => PrimitiveType::TriangleStrip,
        "TRIANGLE_FAN" => PrimitiveType::TriangleFan,
        _ => PrimitiveType::Lines,
    }
}

#[test]
fn wireframe_index_generator() {
    run(
        "core",
        "WireframeIndexGenerator",
        |_| EXACT,
        |f, a| {
            let p = primitive(s(&a[0]));
            let count = n(&a[1]) as usize;
            match f {
                "createWireframeIndices" => {
                    let original = (!a[2].is_null()).then(|| indices(&a[2]));
                    let r = wireframe::create_wireframe_indices(p, count, original.as_deref());
                    ok(r.map(|r| {
                        let mut values = r.to_f64_vec();
                        if original.is_none() {
                            values.truncate(40);
                        }
                        json!([typed_array_name(&r), r.len(), values.enc()])
                    }))
                }
                "getWireframeIndicesCount" => ok(wireframe::get_wireframe_indices_count(p, count) as f64),
                _ => None,
            }
        },
    );
}

#[test]
fn datatypes() {
    run(
        "core",
        "Datatypes",
        |_| EXACT,
        |f, a| match f {
            "createTypedArray" => {
                let d = ComponentDatatype::from_name(s(&a[0])).unwrap();
                let arr = d.create_typed_array(&nums(&a[1]));
                ok(json!([d as u32, d.get_size_in_bytes(), typed_array_name(&arr), arr.to_f64_vec().enc()]))
            }
            "createArrayBufferView" => {
                let d = ComponentDatatype::from_name(s(&a[0])).unwrap();
                let bytes: Vec<u8> = list(&a[1], |v| n(v) as u8);
                let size = d.get_size_in_bytes();
                let off = (n(&a[2]) as usize / size) * size;
                let view = d.create_array_buffer_view(&bytes, off, opt(&a[3]).map(|l| l as usize)).unwrap();
                ok(json!([off, view.to_f64_vec().enc()]))
            }
            "indexCreateTypedArray" => {
                let arr = IndexDatatype::create_typed_array(n(&a[0]) as usize, &indices(&a[1]));
                ok(json!([typed_array_name(&arr), arr.to_f64_vec().enc()]))
            }
            _ => None,
        },
    );
}

#[test]
fn stereographic() {
    run(
        "core",
        "Stereographic",
        |_| TIGHT,
        |f, a| match f {
            "fromCartesian" => {
                let st = Stereographic::from_cartesian(c3(&a[0])).unwrap();
                ok(json!([
                    st.position.enc(),
                    num(st.conformal_latitude()),
                    num(st.longitude()),
                    st.get_latitude(&Ellipsoid::WGS84).enc(),
                    num(st.tangent_plane.plane.normal.z)
                ]))
            }
            _ => None,
        },
    );
}

fn scheme(name: &str) -> Box<dyn TilingScheme> {
    match name {
        "geographic" => Box::new(GeographicTilingScheme::default()),
        "geographicCustom" => Box::new(GeographicTilingScheme::new(
            Ellipsoid::WGS84,
            Rectangle::from_degrees(170.0, -40.0, -150.0, 20.0),
            3,
            2,
        )),
        "webMercator" => Box::new(WebMercatorTilingScheme::default()),
        _ => Box::new(WebMercatorTilingScheme::new(
            Ellipsoid::WGS84,
            2,
            1,
            Some((Cartesian2::new(-5e6, -3e6), Cartesian2::new(7e6, 4e6))),
        )),
    }
}

#[test]
fn tiling_scheme() {
    run(
        "core",
        "TilingScheme",
        |_| TIGHT,
        |f, a| {
            let t = scheme(s(&a[0]));
            match f {
                "rectangle" => ok(json!([
                    t.rectangle().enc(),
                    t.get_number_of_x_tiles_at_level(3),
                    t.get_number_of_y_tiles_at_level(3)
                ])),
                "tileXYToRectangle" => {
                    let (x, y, l) = (n(&a[1]), n(&a[2]), n(&a[3]) as u32);
                    ok(vec![t.tile_xy_to_rectangle(x, y, l), t.tile_xy_to_native_rectangle(x, y, l)])
                }
                "positionToTileXY" => ok(t.position_to_tile_xy(&carto(&a[1]), n(&a[2]) as u32)),
                "rectangleToNativeRectangle" => ok(t.rectangle_to_native_rectangle(&rect(&a[1]))),
                _ => None,
            }
        },
    );
}

#[test]
fn display_conditions_match_cesium_defaults_and_packing() {
    // Cesium: new NearFarScalar() is (0, 0, 1, 0); new DistanceDisplayCondition() is (0, Number.MAX_VALUE).
    assert_eq!(NearFarScalar::default(), NearFarScalar::new(0.0, 0.0, 1.0, 0.0));
    assert_eq!(DistanceDisplayCondition::default(), DistanceDisplayCondition::new(0.0, f64::MAX));
    let mut array = [9.0; 7];
    NearFarScalar::new(1.0, 2.0, 3.0, 4.0).pack(&mut array, 2);
    assert_eq!(array, [9.0, 9.0, 1.0, 2.0, 3.0, 4.0, 9.0]);
    assert_eq!(NearFarScalar::unpack(&array, 2), NearFarScalar::new(1.0, 2.0, 3.0, 4.0));
    DistanceDisplayCondition::new(5.0, 6.0).pack(&mut array, 5);
    assert_eq!(DistanceDisplayCondition::unpack(&array, 5), DistanceDisplayCondition::new(5.0, 6.0));
}

#[test]
fn double_ended_priority_queue_keeps_the_maximum_reachable() {
    // The case from the PR #4 review: CesiumJS reports 23 here, with 28 still queued.
    let mut q = DoubleEndedPriorityQueue::new(|a: &f64, b: &f64| a - b, None);
    for v in [10.0, 23.0, 29.0, 23.0, 23.0, 28.0] {
        q.insert(v);
    }
    assert_eq!(q.remove_minimum(), Some(10.0));
    assert_eq!(q.remove_maximum(), Some(29.0));
    assert_eq!(q.get_maximum(), Some(&28.0));

    // Against a sorted list, over many random operations.
    let mut seed = 12345u64;
    let mut next = || {
        seed = seed.wrapping_mul(6364136223846793005).wrapping_add(1442695040888963407);
        (seed >> 33) as u32
    };
    for _ in 0..200 {
        let mut q = DoubleEndedPriorityQueue::new(|a: &f64, b: &f64| a - b, None);
        let mut reference: Vec<f64> = Vec::new();
        for _ in 0..60 {
            match next() % 4 {
                0 | 1 => {
                    let v = f64::from(next() % 50);
                    q.insert(v);
                    reference.push(v);
                }
                2 => {
                    reference.sort_by(f64::total_cmp);
                    let expected = (!reference.is_empty()).then(|| reference.remove(0));
                    assert_eq!(q.remove_minimum(), expected);
                }
                _ => {
                    reference.sort_by(f64::total_cmp);
                    assert_eq!(q.remove_maximum(), reference.pop());
                }
            }
            reference.sort_by(f64::total_cmp);
            assert_eq!(q.get_minimum(), reference.first());
            assert_eq!(q.get_maximum(), reference.last());
        }
    }
}
