//! Core's small utilities against CesiumJS fixtures: array helpers,
//! containers, typed-array strings, queries and URIs, pixel formats, enums.

mod common;

use cesium_rs::core::array_remove_duplicates::array_remove_duplicates;
use cesium_rs::core::barycentric_coordinates::{
    barycentric_coordinates, barycentric_coordinates_2d, point_inside_triangle,
};
use cesium_rs::core::get_string_from_typed_array::{get_magic, get_string_from_typed_array};
use cesium_rs::core::merge_sort::merge_sort;
use cesium_rs::core::pixel_datatype::PixelArrayType;
use cesium_rs::core::query::{object_to_query, parse_response_headers, query_to_object, QueryValue};
use cesium_rs::core::srgb_to_linear::srgb_to_linear;
use cesium_rs::core::subdivide_array::subdivide_array;
use cesium_rs::core::uri_helpers::*;
use cesium_rs::core::webgl_constant_to_glsl_type::webgl_constant_to_glsl_type;
use cesium_rs::core::*;
use common::*;
use serde_json::{json, Value};

#[test]
fn array_utilities() {
    run(
        "core",
        "ArrayUtilities",
        |_| TIGHT,
        |f, a| match f {
            "arrayRemoveDuplicates" => {
                let mut removed = Vec::new();
                let values = list(&a[0], c3);
                let result = array_remove_duplicates(
                    &values,
                    |x, y, e| x.equals_epsilon(*y, e),
                    a[1].as_bool().unwrap(),
                    Some(&mut removed),
                );
                ok(json!([result.enc(), removed.iter().map(|&i| i as f64).collect::<Vec<_>>().enc()]))
            }
            "barycentricCoordinates" => ok(barycentric_coordinates(c3(&a[0]), c3(&a[1]), c3(&a[2]), c3(&a[3]))),
            "pointInsideTriangle" => ok(point_inside_triangle(c3(&a[0]), c3(&a[1]), c3(&a[2]), c3(&a[3]))),
            "barycentricCoordinates2D" => ok(barycentric_coordinates_2d(c2(&a[0]), c2(&a[1]), c2(&a[2]), c2(&a[3]))),
            "subdivideArray" => ok(subdivide_array(&nums(&a[0]), n(&a[1]) as usize).unwrap()),
            "mergeSort" => {
                let mut values: Vec<Vec<f64>> = list(&a[0], nums);
                merge_sort(&mut values, |x, y| x[0] - y[0]);
                ok(values)
            }
            "srgbToLinear" => ok(srgb_to_linear(n(&a[0]))),
            _ => None,
        },
    );
}

#[test]
fn containers() {
    run(
        "core",
        "Containers",
        |_| EXACT,
        |f, a| match f {
            "queue" => {
                let mut q = Queue::new();
                let mut k = 0.0;
                let out: Vec<Value> = a[0]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|op| {
                        let mut r = None;
                        match s(op) {
                            "enqueue" => {
                                q.enqueue((k * 7.0) % 11.0);
                                k += 1.0;
                            }
                            "dequeue" => r = q.dequeue(),
                            "peek" => r = q.peek().copied(),
                            "sort" => q.sort(|x: &f64, y: &f64| x.total_cmp(y)),
                            _ => q.clear(),
                        }
                        json!([r.enc(), q.len()])
                    })
                    .collect();
                ok(out)
            }
            "doublyLinkedList" => {
                let mut l = DoublyLinkedList::new();
                let mut nodes = Vec::new();
                let out: Vec<Value> = a[0]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|op| {
                        let (x, y) = (n(&op[1]) as usize, n(&op[2]) as usize);
                        let live: Vec<_> = nodes.iter().filter(|(_, alive)| *alive).map(|(id, _)| *id).collect();
                        match s(&op[0]) {
                            "add" => nodes.push((l.add(x as f64), true)),
                            "remove" if !live.is_empty() => {
                                let id = live[x % live.len()];
                                l.remove(id);
                                nodes.iter_mut().find(|(i, _)| *i == id).unwrap().1 = false;
                            }
                            "splice" if !live.is_empty() => l.splice(live[x % live.len()], live[y % live.len()]),
                            _ => {}
                        }
                        json!([l.iter().copied().collect::<Vec<f64>>().enc(), l.len()])
                    })
                    .collect();
                ok(out)
            }
            "associativeArray" => {
                let mut arr = AssociativeArray::new();
                let keys = ["a", "b", "c", "1", "2"];
                let out: Vec<Value> = a[0]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|op| {
                        let key = s(&op[1]);
                        let mut r = Value::Null;
                        match s(&op[0]) {
                            "set" => arr.set(key, n(&op[2])),
                            "remove" => r = json!(arr.remove(key)),
                            _ => arr.remove_all(),
                        }
                        json!([r, (arr.values().to_vec().enc()), (keys.map(|k| arr.get(k).copied()).to_vec().enc())])
                    })
                    .collect();
                ok(out)
            }
            "managedArray" => {
                let mut m = ManagedArray::new(2);
                let out: Vec<Value> = a[0]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|op| {
                        let v = n(&op[1]);
                        let mut r = None;
                        match s(&op[0]) {
                            "push" => m.push(v),
                            "pop" => r = m.pop(),
                            "set" => m.set(v as usize, v * 10.0),
                            "resize" => m.resize(v as usize),
                            _ => r = m.peek().copied(),
                        }
                        let values: Vec<Option<f64>> = (0..m.len()).map(|i| m.get(i).copied()).collect();
                        json!([r.enc(), m.len(), values.enc()])
                    })
                    .collect();
                ok(out)
            }
            _ => None,
        },
    );
}

#[test]
fn strings() {
    run(
        "core",
        "Strings",
        |_| EXACT,
        |f, a| {
            let bytes: Vec<u8> = list(&a[0], |v| n(v) as u8);
            match f {
                "getStringFromTypedArray" => {
                    ok(get_string_from_typed_array(&bytes, n(&a[1]) as usize, opt(&a[2]).map(|l| l as usize)))
                }
                "getMagic" => ok(get_magic(&bytes, n(&a[1]) as usize)),
                _ => None,
            }
        },
    );
}

fn entries_enc(e: &[(String, QueryValue)]) -> Value {
    Value::Array(
        e.iter()
            .map(|(k, v)| match v {
                QueryValue::Single(s) => json!([k, s]),
                QueryValue::Multiple(m) => json!([k, m]),
            })
            .collect(),
    )
}

#[test]
fn query_and_uri() {
    run(
        "core",
        "QueryAndUri",
        |_| EXACT,
        |f, a| match f {
            "objectToQuery" => {
                let entries: Vec<(String, QueryValue)> = list(&a[0], |e| {
                    let v = if e[1].is_array() {
                        QueryValue::Multiple(list(&e[1], |x| s(x).to_string()))
                    } else {
                        QueryValue::Single(s(&e[1]).to_string())
                    };
                    (s(&e[0]).to_string(), v)
                });
                ok(object_to_query(&entries))
            }
            "queryToObject" => ok(entries_enc(&query_to_object(s(&a[0])).unwrap())),
            "getExtensionFromUri" => ok(get_extension_from_uri(s(&a[0]))),
            "getFilenameFromUri" => ok(get_filename_from_uri(s(&a[0]))),
            "getBaseUri" => ok(get_base_uri(s(&a[0]), a[1].as_bool().unwrap())),
            "getAbsoluteUri" => ok(get_absolute_uri(s(&a[0]), Some(s(&a[1])))),
            _ => None,
        },
    );
}

/// `LeftDoubleClick` → `LEFT_DOUBLE_CLICK`.
fn screaming(name: &str) -> String {
    let mut out = String::new();
    for (i, c) in name.chars().enumerate() {
        if c.is_ascii_uppercase() && i > 0 {
            out.push('_');
        }
        out.push(c.to_ascii_uppercase());
    }
    out
}

fn pixel_format(name: &str) -> PixelFormat {
    *PixelFormat::ALL.iter().find(|p| p.name() == name).unwrap()
}

fn pixel_datatype(name: &str) -> PixelDatatype {
    *PixelDatatype::ALL.iter().find(|p| p.name() == name).unwrap()
}

#[test]
fn pixel_format_and_datatype() {
    run(
        "core",
        "PixelFormat",
        |_| EXACT,
        |f, a| match f {
            "formats" => ok(Value::Array(PixelFormat::ALL.iter().map(|p| json!([p.name(), *p as u32])).collect())),
            "datatypes" => ok(Value::Array(PixelDatatype::ALL.iter().map(|p| json!([p.name(), *p as u32])).collect())),
            "predicates" => {
                let p = pixel_format(s(&a[0]));
                ok(json!([
                    p.components_length(),
                    PixelFormat::validate(p as u32),
                    p.is_color_format(),
                    p.is_depth_format(),
                    p.is_compressed_format(),
                    p.is_dxt_format(),
                    p.is_pvrtc_format(),
                    p.is_astc_format(),
                    p.is_etc1_format(),
                    p.is_etc2_format(),
                    p.is_bc7_format(),
                    ([1.0, 3.0, 4.0, 7.0, 16.0, 33.0]
                        .map(|w| p.compressed_texture_size_in_bytes(w, w + 1.0))
                        .to_vec()
                        .enc())
                ]))
            }
            "sizes" => {
                let (p, d) = (pixel_format(s(&a[0])), pixel_datatype(s(&a[1])));
                ok(json!([
                    num(p.texture_size_in_bytes(d, 5.0, 3.0)),
                    num(p.texture_3d_size_in_bytes(d, 5.0, 3.0, 2.0)),
                    p.alignment_in_bytes(d, 5.0),
                    p.alignment_in_bytes(d, 6.0),
                    p.to_internal_format(d, true),
                    p.to_internal_format(d, false)
                ]))
            }
            "datatype" => {
                let d = pixel_datatype(s(&a[0]));
                let name = match d.get_typed_array_type() {
                    PixelArrayType::Uint8 => "Uint8Array",
                    PixelArrayType::Uint16 => "Uint16Array",
                    PixelArrayType::Uint32 => "Uint32Array",
                    PixelArrayType::Float32 => "Float32Array",
                };
                ok(json!([
                    d.to_webgl_constant(true),
                    d.to_webgl_constant(false),
                    d.is_packed(),
                    d.size_in_bytes(),
                    name
                ]))
            }
            "flipY" => {
                let (p, d) = (pixel_format(s(&a[1])), pixel_datatype(s(&a[2])));
                let input = TypedArray::from_values(
                    if d == PixelDatatype::Float { ComponentDatatype::Float } else { ComponentDatatype::UnsignedByte },
                    &nums(&a[0]),
                );
                ok(p.flip_y(&input, d, n(&a[3]) as usize, n(&a[4]) as usize).to_f64_vec())
            }
            _ => None,
        },
    );
}

fn enum_entries(name: &str) -> Value {
    macro_rules! numeric {
        ($t:ty) => {
            Value::Array(<$t>::ALL.iter().map(|v| json!([screaming(&format!("{v:?}")), *v as u32])).collect())
        };
    }
    macro_rules! strings {
        ($t:ty) => {
            Value::Array(<$t>::ALL.iter().map(|v| json!([v.as_str().to_uppercase(), v.as_str()])).collect())
        };
    }
    match name {
        "WindingOrder" => numeric!(WindingOrder),
        "TrackingReferenceFrame" => numeric!(TrackingReferenceFrame),
        "ExtrapolationType" => numeric!(ExtrapolationType),
        "InterpolationType" => numeric!(InterpolationType),
        "HeightmapEncoding" => numeric!(HeightmapEncoding),
        "KeyboardEventModifier" => numeric!(KeyboardEventModifier),
        "RequestState" => numeric!(RequestState),
        "RequestType" => numeric!(RequestType),
        "GeocodeType" => numeric!(GeocodeType),
        "ScreenSpaceEventType" => numeric!(ScreenSpaceEventType),
        "TileEdge" => numeric!(TileEdge),
        "TerrainQuantization" => numeric!(TerrainQuantization),
        "IonGeocodeProviderType" => strings!(IonGeocodeProviderType),
        _ => strings!(ArticulationStageType),
    }
}

#[test]
fn misc() {
    run(
        "core",
        "Misc",
        |_| TIGHT,
        |f, a| match f {
            "webGLConstantToGlslType" => ok(webgl_constant_to_glsl_type(n(&a[0]) as u32).map(str::to_string)),
            "verticalExaggeration" => ok(cesium_rs::core::vertical_exaggeration::get_position(
                c3(&a[0]),
                &Ellipsoid::WGS84,
                n(&a[1]),
                n(&a[2]),
            )),
            "verticalExaggerationHeight" => {
                ok(cesium_rs::core::vertical_exaggeration::get_height(n(&a[0]), n(&a[1]), n(&a[2])))
            }
            "enum" => ok(enum_entries(s(&a[0]))),
            _ => None,
        },
    );
}

#[test]
fn guids_events_headers_and_origins() {
    let guid = cesium_rs::core::create_guid::create_guid();
    assert_eq!(guid.len(), 36);
    assert_eq!(&guid[14..15], "4");
    assert!("89ab".contains(&guid[19..20]));
    assert_ne!(guid, cesium_rs::core::create_guid::create_guid());

    use std::sync::atomic::{AtomicI32, Ordering};
    use std::sync::{Arc, Mutex};
    let event = Arc::new(Mutex::new(Event::<i32>::new()));
    let hits = Arc::new(AtomicI32::new(0));
    let mut helper = EventHelper::new();
    let h = hits.clone();
    let remove_one = helper.add(&event, move |_| {
        h.fetch_add(1, Ordering::SeqCst);
    });
    let h = hits.clone();
    let _keep = helper.add(&event, move |_| {
        h.fetch_add(10, Ordering::SeqCst);
    });
    event.lock().unwrap().raise_event(&0);
    assert_eq!(hits.load(Ordering::SeqCst), 11);
    remove_one();
    event.lock().unwrap().raise_event(&0);
    assert_eq!(hits.load(Ordering::SeqCst), 21);
    helper.remove_all();
    event.lock().unwrap().raise_event(&0);
    assert_eq!(hits.load(Ordering::SeqCst), 21);
    assert_eq!(event.lock().unwrap().number_of_listeners(), 0);

    let headers =
        parse_response_headers("Content-Type: text/plain\r\nX-1: a\r\nbad line\r\nContent-Type: json\r\n2: b");
    assert_eq!(
        headers,
        vec![("2".into(), "b".into()), ("Content-Type".into(), "json".into()), ("X-1".into(), "a".into())]
    );

    assert!(!is_cross_origin_url("/a/b.json", "http://example.com/app/"));
    assert!(is_cross_origin_url("https://example.com/x", "http://example.com/app/"));
    assert!(!is_cross_origin_url("http://EXAMPLE.com:80/x", "http://example.com/app/"));
    assert!(is_data_uri("DATA:image/png;base64,") && !is_data_uri("http://x"));
    assert!(is_blob_uri("blob:http://x/1"));
    assert_eq!(append_forward_slash("a"), "a/");
    assert_eq!(append_forward_slash("a/"), "a/");
}

#[test]
fn js_math_matches_v8_bit_for_bit() {
    use cesium_rs::core::js_math::JsMath;
    run(
        "core",
        "JsMath",
        |_| EXACT,
        |f, a| {
            let xs = nums(&a[0]);
            let map = |op: fn(f64) -> f64| ok(xs.iter().map(|&x| op(x)).collect::<Vec<_>>());
            match f {
                "sin" => map(f64::js_sin),
                "cos" => map(f64::js_cos),
                "tan" => map(f64::js_tan),
                "atan" => map(f64::js_atan),
                "exp" => map(f64::js_exp),
                "log" => map(f64::js_ln),
                "asin" => map(f64::js_asin),
                "acos" => map(f64::js_acos),
                "cosh" => map(f64::js_cosh),
                "atan2" => {
                    let x = nums(&a[1]);
                    ok(xs.iter().zip(&x).map(|(&y, &x)| y.js_atan2(x)).collect::<Vec<_>>())
                }
                "special" => ok(xs
                    .iter()
                    .flat_map(|&x| [x.js_sin(), x.js_cos(), x.abs().js_ln(), x.js_atan()])
                    .collect::<Vec<_>>()),
                _ => None,
            }
        },
    );
}
