//! Deferred Core ports checked against the pinned JavaScript engine.
mod common;
use cesium_rs::core::compressed_texture_buffer::CompressedTextureBufferOptions;
use cesium_rs::core::decode_google_earth_enterprise_data::decode_google_earth_enterprise_data;
use cesium_rs::core::decode_vector_polyline_positions::{
    decode_vector_polyline_positions, DecodeVectorPolylineOptions,
};
use cesium_rs::core::google_earth_enterprise_tile_information::GoogleEarthEnterpriseTileInformationOptions;
use cesium_rs::core::proxy::Proxy;
use cesium_rs::core::query::QueryValue;
use cesium_rs::core::resource::{DerivedResourceOptions, QueryParameters, ResourceOptions};
use cesium_rs::core::*;
use common::*;
use serde_json::{json, Value};
use std::sync::Arc;
fn bytes(v: &Value) -> Vec<u8> {
    v.as_array().unwrap().iter().map(|v| u(v) as u8).collect()
}
fn qp(v: Value) -> QueryParameters {
    v.as_object()
        .unwrap()
        .iter()
        .map(|(k, v)| {
            (
                k.clone(),
                Some(if let Some(a) = v.as_array() {
                    QueryValue::Multiple(a.iter().map(|v| s(v).into()).collect())
                } else {
                    QueryValue::Single(s(v).into())
                }),
            )
        })
        .collect()
}
fn qjson(q: &QueryParameters) -> Value {
    q.iter()
        .map(|(k, v)| {
            (
                k.clone(),
                match v {
                    None => Value::Null,
                    Some(QueryValue::Single(s)) => json!(s),
                    Some(QueryValue::Multiple(v)) => json!(v),
                },
            )
        })
        .collect()
}
#[test]
fn proxy() {
    run("core", "DefaultProxy", |_| EXACT, |_, a| ok(DefaultProxy::new(s(&a[0])).get_url(s(&a[1]))));
}
#[test]
fn trusted() {
    run(
        "core",
        "TrustedServers",
        |_| EXACT,
        |_, a| {
            let mut t = TrustedServers::new();
            t.add(s(&a[0]), 443);
            let urls = [
                "https://example.test/a",
                "https://user:pw@EXAMPLE.test:443/a",
                "http://example.test/a",
                "//example.test:443/a",
                "https://bücher.test/a",
                "https://[::1]:443/a",
                "/relative",
            ];
            let before: Vec<_> = urls.iter().map(|u| t.contains(u, "https:")).collect();
            t.remove(s(&a[0]), 443);
            ok(json!([before, urls.map(|u| t.contains(u, "https:"))]))
        },
    );
}
#[test]
fn texture_buffer() {
    run(
        "core",
        "CompressedTextureBuffer",
        |_| EXACT,
        |_, a| {
            let v = CompressedTextureBuffer::new(CompressedTextureBufferOptions {
                internal_format: u(&a[0]),
                pixel_datatype: opt(&a[1]).map(|v| v as u32),
                width: u(&a[2]),
                height: u(&a[3]),
                buffer: bytes(&a[4]).into(),
            });
            let c = v.clone();
            ok(json!([
                c.internal_format,
                c.pixel_datatype,
                c.width,
                c.height,
                c.buffer_view(),
                Arc::ptr_eq(c.shared_buffer(), v.shared_buffer())
            ]))
        },
    );
}
#[test]
fn credits() {
    run(
        "core",
        "Credit",
        |_| EXACT,
        |_, a| {
            let c = Credit::new(s(&a[0]), a[1].as_bool().unwrap());
            let b = c.clone();
            let d = Credit::new(s(&a[0]), !c.show_on_screen);
            ok(json!([
                c.html(),
                c.show_on_screen,
                c.id() == b.id(),
                c == b,
                c == d,
                c.is_ion(),
                Credit::get_ion_credit(&Attribution { html: c.html().into(), collapsible: Some(false) }).show_on_screen
            ]))
        },
    );
}
#[test]
fn request_errors() {
    run(
        "core",
        "RequestErrorEvent",
        |_| EXACT,
        |_, a| {
            let e = RequestErrorEvent::from_header_string(opt(&a[0]).map(|v| v as u16), Some("body"), a[1].as_str());
            let headers = e
                .response_headers
                .as_ref()
                .map(|h| h.iter().map(|(k, v)| (k.clone(), json!(v))).collect::<serde_json::Map<_, _>>());
            ok(json!([e.status_code, e.response, headers, e.to_string()]))
        },
    );
}
#[test]
fn tile_information() {
    run(
        "core",
        "GoogleEarthEnterpriseTileInformation",
        |_| EXACT,
        |_, a| {
            let mut t = GoogleEarthEnterpriseTileInformation::new(GoogleEarthEnterpriseTileInformationOptions {
                bits: u(&a[0]) as u8,
                cnode_version: 2,
                imagery_version: 3,
                terrain_version: 4,
                imagery_provider: 5,
                terrain_provider: 6,
            });
            let p = GoogleEarthEnterpriseTileInformation::new(GoogleEarthEnterpriseTileInformationOptions {
                bits: 128,
                ..Default::default()
            });
            t.set_parent(&p);
            let b = t.clone();
            ok(json!([
                b.has_subtree(),
                b.has_imagery(),
                b.has_terrain(),
                b.has_children(),
                ([0, 1, 2, 3].map(|i| b.has_child(i))),
                b.get_child_bitmask(),
                b.ancestor_has_terrain,
                b.cnode_version,
                b.imagery_version,
                b.terrain_version,
                b.imagery_provider,
                b.terrain_provider
            ]))
        },
    );
}
#[test]
fn texture_packing() {
    run(
        "core",
        "TexturePacker",
        |_| EXACT,
        |_, a| {
            let mut p = TexturePacker::new(n(&a[0]), n(&a[0]), n(&a[1]));
            ok(json!(a[2]
                .as_array()
                .unwrap()
                .iter()
                .enumerate()
                .map(|(i, b)| p.pack(i, n(&b[0]), n(&b[1])).map(|node| node.rectangle.enc()))
                .collect::<Vec<_>>()))
        },
    );
}
#[test]
fn collisions() {
    run(
        "core",
        "RectangleCollisionChecker",
        |_| EXACT,
        |_, a| {
            let mut c = RectangleCollisionChecker::new();
            ok(json!(a[0]
                .as_array()
                .unwrap()
                .iter()
                .map(|o| {
                    let r = rect(&o[2]);
                    match s(&o[0]) {
                        "insert" => {
                            c.insert(s(&o[1]), r);
                            Value::Null
                        }
                        "remove" => {
                            c.remove(s(&o[1]), &r);
                            Value::Null
                        }
                        _ => json!(c.collides(&r)),
                    }
                })
                .collect::<Vec<_>>()))
        },
    );
}
#[test]
fn gee_decoding() {
    run(
        "core",
        "decodeGoogleEarthEnterpriseData",
        |_| EXACT,
        |_, a| {
            let mut b = bytes(&a[1]);
            match decode_google_earth_enterprise_data(&bytes(&a[0]), &mut b) {
                Ok(()) => ok(json!(b)),
                Err(e) => Some(Out::Err(e.to_string())),
            }
        },
    );
}
#[test]
fn vector_decoding() {
    run(
        "core",
        "decodeVectorPolylinePositions",
        |_| TIGHT,
        |_, a| {
            let mut b: Vec<_> = a[0].as_array().unwrap().iter().map(|v| u(v) as u16).collect();
            let e = if s(&a[4]) == "WGS84" { Ellipsoid::WGS84 } else { Ellipsoid::UNIT_SPHERE };
            let v = decode_vector_polyline_positions(
                &mut b,
                DecodeVectorPolylineOptions {
                    rectangle: &rect(&a[1]),
                    minimum_height: n(&a[2]),
                    maximum_height: n(&a[3]),
                    ellipsoid: &e,
                },
            )?;
            ok(json!([v.enc(), b]))
        },
    );
}
#[test]
fn resource_urls() {
    run(
        "core",
        "Resource",
        |_| EXACT,
        |_, a| {
            let r = Resource::from_options(ResourceOptions {
                url: s(&a[0]).into(),
                query_parameters: qp(json!({"a":"base","z":["3","4"]})),
                template_values: vec![("z".into(), "a/b".into()), ("x".into(), "é".into())],
                headers: vec![("Accept".into(), "x".into())],
                ..Default::default()
            })?;
            r.append_query_parameters(&qp(json!({"a":["appended"],"new":"v"})));
            let b = r.get_derived_resource(DerivedResourceOptions {
                url: Some("../other/{x}?a=derived&u=1".into()),
                preserve_query_parameters: a[1].as_bool().unwrap(),
                query_parameters: qp(json!({"q":"a b"})),
                headers: vec![("X".into(), "y".into())],
                ..Default::default()
            })?;
            let encode = |r: &Resource| {
                let o = r.options();
                let tv: serde_json::Map<_, _> = o.template_values.iter().map(|(k, v)| (k.clone(), json!(v))).collect();
                let h: serde_json::Map<_, _> = o.headers.iter().map(|(k, v)| (k.clone(), json!(v))).collect();
                json!([
                    r.get_url_component(true, true),
                    r.get_url_component(false, false),
                    qjson(&o.query_parameters),
                    tv,
                    h,
                    r.extension(),
                    r.get_base_uri(true),
                    r.is_data_uri(),
                    r.is_blob_uri()
                ])
            };
            ok(json!([encode(&r), encode(&b), encode(&r.clone())]))
        },
    );
}
#[test]
fn coordinates() {
    run(
        "core",
        "CartographicGeocoderService",
        |_| TIGHT,
        |_, a| match CartographicGeocoderService.geocode_coordinates(s(&a[0])) {
            Ok(r) => ok(json!(r
                .iter()
                .map(|r| {
                    let d = match r.destination {
                        GeocoderDestination::Point(c) => c.enc(),
                        GeocoderDestination::Rectangle(c) => c.enc(),
                    };
                    json!([r.display_name, d])
                })
                .collect::<Vec<_>>())),
            Err(e) => Some(Out::Err(e.to_string())),
        },
    );
}

fn pack_contract<T: Packable>(values: &[f64], offset: usize) -> Value {
    let t = T::unpack(values, 0);
    let mut out = vec![-999.; T::PACKED_LENGTH + offset + 2];
    t.pack(&mut out, offset);
    let clone = T::unpack(&out, offset);
    let mut copy = vec![0.; T::PACKED_LENGTH];
    clone.pack(&mut copy, 0);
    json!([T::PACKED_LENGTH, out, copy])
}
#[test]
fn packing_contract() {
    run(
        "core",
        "Packable",
        |_| EXACT,
        |_, a| {
            let values = nums(&a[1]);
            let offset = u(&a[2]) as usize;
            macro_rules! pack{($($name:ident),*)=>{match s(&a[0]){$(stringify!($name)=>pack_contract::<$name>(&values,offset),)*_=>panic!("unknown packable")}}}
            ok(pack!(
                Cartesian2,
                Cartesian3,
                Cartesian4,
                Matrix2,
                Matrix3,
                Matrix4,
                Quaternion,
                Color,
                BoundingRectangle,
                BoundingSphere,
                Rectangle,
                Ellipsoid,
                NearFarScalar,
                DistanceDisplayCondition,
                VertexFormat
            ))
        },
    );
}
#[test]
fn factory_contract() {
    run(
        "core",
        "GeometryFactory",
        |_| TIGHT,
        |_, a| {
            let d: Box<dyn GeometryFactory> = match s(&a[0]) {
                "Box" => {
                    Box::new(BoxGeometry::from_dimensions(Cartesian3::new(2., 3., 4.), VertexFormat::DEFAULT, None)?)
                }
                "Plane" => Box::new(PlaneGeometry::new(VertexFormat::DEFAULT)),
                "Sphere" => Box::new(SphereGeometry::new(2., 4., 5., VertexFormat::DEFAULT)?),
                _ => Box::new(CylinderGeometry::new(3., 1., 2., 8, VertexFormat::DEFAULT, None)?),
            };
            let g = d.create_geometry()?;
            ok(json!([
                g.primitive_type as u8,
                g.indices.as_ref()?.len(),
                g.attributes.position.as_ref()?.values.len(),
                g.bounding_sphere?.radius
            ]))
        },
    );
}
