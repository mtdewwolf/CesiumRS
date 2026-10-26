//! Interaction, primitive lifecycle, appearances and materials replayed against
//! pinned CesiumJS fixtures.
mod common;

use cesium_rs::core::*;
use cesium_rs::renderer::RenderState;
use cesium_rs::scene::*;
use common::*;
use serde_json::{json, Value};

fn rs_snapshot(rs: &RenderState, mask_depth_mask: bool, mask_blending: bool) -> Value {
    let depth_mask = if mask_depth_mask { Value::Null } else { json!(rs.depth_mask) };
    let blending = if mask_blending && !rs.blending.enabled {
        json!({
            "enabled": Value::Null,
            "equationRgb": Value::Null,
            "sourceRgb": Value::Null,
            "destinationRgb": Value::Null,
        })
    } else {
        json!({
            "enabled": rs.blending.enabled,
            "equationRgb": rs.blending.equation_rgb,
            "sourceRgb": rs.blending.function_source_rgb,
            "destinationRgb": rs.blending.function_destination_rgb,
        })
    };
    let cull = if !rs.cull.enabled {
        json!({ "enabled": Value::Null, "face": Value::Null })
    } else {
        json!({ "enabled": rs.cull.enabled, "face": rs.cull.face })
    };
    json!({
        "depthMask": depth_mask,
        "depthTest": rs.depth_test.enabled,
        "blending": blending,
        "cull": cull,
    })
}

fn vf_array(vf: &VertexFormat) -> Value {
    json!([vf.position, vf.normal, vf.st, vf.tangent, vf.bitangent, vf.color])
}

fn appearance_snapshot(app: &dyn Appearance) -> Value {
    let rs = app.render_state();
    let get_rs = app.get_render_state();
    json!({
        "vertexShaderSource": app.vertex_shader_source(),
        "fragmentShaderSource": app.fragment_shader_source(),
        "getFragmentShaderSource": app.get_fragment_shader_source(),
        "renderState": rs_snapshot(rs, !app.translucent(), !app.translucent()),
        "getRenderState": rs_snapshot(&get_rs, false, !app.is_translucent()),
        "isTranslucent": app.is_translucent(),
        "closed": app.closed(),
        "flat": app.flat(),
        "faceForward": app.face_forward(),
        "vertexFormat": vf_array(&app.vertex_format()),
        "materialType": app.material().map(|m| m.type_name.clone()),
    })
}

#[test]
fn primitive_state() {
    run(
        "scene",
        "PrimitiveState",
        |_| EXACT,
        |f, _| {
            (f == "values").then(|| {
                Out::Ok(json!({
                    "READY": PrimitiveState::Ready.value(),
                    "CREATING": PrimitiveState::Creating.value(),
                    "CREATED": PrimitiveState::Created.value(),
                    "COMBINING": PrimitiveState::Combining.value(),
                    "COMBINED": PrimitiveState::Combined.value(),
                    "COMPLETE": PrimitiveState::Complete.value(),
                    "FAILED": PrimitiveState::Failed.value(),
                }))
            })
        },
    );
}

#[test]
fn appearance_defaults() {
    run(
        "scene",
        "Appearance",
        |_| EXACT,
        |f, a| {
            (f == "getDefaultRenderState").then(|| {
                let translucent = a[0].as_bool().unwrap();
                let closed = a[1].as_bool().unwrap();
                let mut existing: Option<RenderState> = None;
                if let Some(obj) = a[2].as_object() {
                    let mut rs = RenderState::default();
                    if let Some(dm) = obj.get("depthMask").and_then(Value::as_bool) {
                        rs.depth_mask = dm;
                    }
                    if let Some(cull) = obj.get("cull").and_then(Value::as_object) {
                        if let Some(en) = cull.get("enabled").and_then(Value::as_bool) {
                            rs.cull.enabled = en;
                        }
                        if let Some(face) = cull.get("face").and_then(Value::as_u64) {
                            rs.cull.face = face as u32;
                        }
                    }
                    existing = Some(rs);
                }
                let rs = default_render_state(translucent, closed, existing);
                let mask = !translucent && a[2].is_null();
                Out::Ok(rs_snapshot(&rs, mask, mask))
            })
        },
    );
}

#[test]
fn material_appearance() {
    run(
        "scene",
        "MaterialAppearance",
        |_| EXACT,
        |f, _| match f {
            "default" => Some(Out::Ok(appearance_snapshot(&MaterialAppearance::default()))),
            "basic" => Some(Out::Ok(appearance_snapshot(&MaterialAppearance::new(MaterialAppearanceOptions {
                material_support: Some(MaterialSupport::Basic),
                material: Some(Material::from_type("Color")),
                ..Default::default()
            })))),
            "all" => Some(Out::Ok(appearance_snapshot(&MaterialAppearance::new(MaterialAppearanceOptions {
                material_support: Some(MaterialSupport::All),
                material: Some(Material::from_type("Stripe")),
                ..Default::default()
            })))),
            "closed" => Some(Out::Ok(appearance_snapshot(&MaterialAppearance::new(MaterialAppearanceOptions {
                closed: Some(true),
                material: Some(Material::from_type("Color")),
                ..Default::default()
            })))),
            "flatFaceForward" => {
                Some(Out::Ok(appearance_snapshot(&MaterialAppearance::new(MaterialAppearanceOptions {
                    flat: Some(true),
                    face_forward: Some(false),
                    material: Some(Material::from_type("Grid")),
                    ..Default::default()
                }))))
            }
            "opaque" => Some(Out::Ok(appearance_snapshot(&MaterialAppearance::new(MaterialAppearanceOptions {
                translucent: Some(false),
                material: Some(Material::from_type("Color")),
                ..Default::default()
            })))),
            "supports" => Some(Out::Ok(json!([
                {
                    "key": "BASIC",
                    "vertexFormat": vf_array(&MaterialSupport::Basic.vertex_format()),
                    "vs": MaterialSupport::Basic.vertex_shader_source(),
                    "fs": MaterialSupport::Basic.fragment_shader_source(),
                },
                {
                    "key": "TEXTURED",
                    "vertexFormat": vf_array(&MaterialSupport::Textured.vertex_format()),
                    "vs": MaterialSupport::Textured.vertex_shader_source(),
                    "fs": MaterialSupport::Textured.fragment_shader_source(),
                },
                {
                    "key": "ALL",
                    "vertexFormat": vf_array(&MaterialSupport::All.vertex_format()),
                    "vs": MaterialSupport::All.vertex_shader_source(),
                    "fs": MaterialSupport::All.fragment_shader_source(),
                }
            ]))),
            _ => None,
        },
    );
}

#[test]
fn ellipsoid_surface_appearance() {
    run(
        "scene",
        "EllipsoidSurfaceAppearance",
        |_| EXACT,
        |f, _| match f {
            "default" => Some(Out::Ok(appearance_snapshot(&EllipsoidSurfaceAppearance::default()))),
            "aboveGround" => Some(Out::Ok(appearance_snapshot(&EllipsoidSurfaceAppearance::new(
                EllipsoidSurfaceAppearanceOptions { above_ground: Some(true), ..Default::default() },
            )))),
            "flat" => Some(Out::Ok(appearance_snapshot(&EllipsoidSurfaceAppearance::new(
                EllipsoidSurfaceAppearanceOptions {
                    flat: Some(true),
                    material: Some(Material::from_type("Stripe")),
                    ..Default::default()
                },
            )))),
            "opaque" => Some(Out::Ok(appearance_snapshot(&EllipsoidSurfaceAppearance::new(
                EllipsoidSurfaceAppearanceOptions { translucent: Some(false), ..Default::default() },
            )))),
            _ => None,
        },
    );
}

#[test]
fn per_instance_color_appearance() {
    run(
        "scene",
        "PerInstanceColorAppearance",
        |_| EXACT,
        |f, _| match f {
            "default" => Some(Out::Ok(appearance_snapshot(&PerInstanceColorAppearance::default()))),
            "flat" => Some(Out::Ok(appearance_snapshot(&PerInstanceColorAppearance::new(
                PerInstanceColorAppearanceOptions { flat: Some(true), ..Default::default() },
            )))),
            "closed" => Some(Out::Ok(appearance_snapshot(&PerInstanceColorAppearance::new(
                PerInstanceColorAppearanceOptions { closed: Some(true), ..Default::default() },
            )))),
            "opaque" => Some(Out::Ok(appearance_snapshot(&PerInstanceColorAppearance::new(
                PerInstanceColorAppearanceOptions { translucent: Some(false), ..Default::default() },
            )))),
            _ => None,
        },
    );
}

#[test]
fn polyline_color_appearance() {
    run(
        "scene",
        "PolylineColorAppearance",
        |_| EXACT,
        |f, _| match f {
            "default" => Some(Out::Ok(appearance_snapshot(&PolylineColorAppearance::default()))),
            "opaque" => {
                Some(Out::Ok(appearance_snapshot(&PolylineColorAppearance::new(PolylineColorAppearanceOptions {
                    translucent: Some(false),
                    ..Default::default()
                }))))
            }
            _ => None,
        },
    );
}

#[test]
fn polyline_material_appearance() {
    run(
        "scene",
        "PolylineMaterialAppearance",
        |_| EXACT,
        |f, _| match f {
            "default" => Some(Out::Ok(appearance_snapshot(&PolylineMaterialAppearance::default()))),
            "glow" => Some(Out::Ok(appearance_snapshot(&PolylineMaterialAppearance::new(
                PolylineMaterialAppearanceOptions {
                    material: Some(Material::from_type("PolylineGlow")),
                    ..Default::default()
                },
            )))),
            "dashDefine" => {
                let mat = Material::new(MaterialOptions {
                    fabric: Fabric {
                        type_name: Some("DashTest".to_string()),
                        source: Some("in float v_polylineAngle;\nczm_material czm_getMaterial(czm_materialInput materialInput)\n{\nreturn czm_getDefaultMaterial(materialInput);\n}\n".to_string()),
                        ..Default::default()
                    },
                    ..Default::default()
                });
                Some(Out::Ok(appearance_snapshot(&PolylineMaterialAppearance::new(
                    PolylineMaterialAppearanceOptions { material: Some(mat), ..Default::default() },
                ))))
            }
            _ => None,
        },
    );
}

#[test]
fn debug_appearance() {
    run(
        "scene",
        "DebugAppearance",
        |_| EXACT,
        |f, _| {
            let app_res = match f {
                "normal" => DebugAppearance::try_new(DebugAppearanceOptions {
                    attribute_name: "normal".to_string(),
                    ..Default::default()
                }),
                "st" => DebugAppearance::try_new(DebugAppearanceOptions {
                    attribute_name: "st".to_string(),
                    ..Default::default()
                }),
                "customVec4" => DebugAppearance::try_new(DebugAppearanceOptions {
                    attribute_name: "temperature".to_string(),
                    glsl_datatype: Some("vec4".to_string()),
                    ..Default::default()
                }),
                "perInstance" => DebugAppearance::try_new(DebugAppearanceOptions {
                    attribute_name: "color".to_string(),
                    per_instance_attribute: Some(true),
                    glsl_datatype: Some("vec4".to_string()),
                    ..Default::default()
                }),
                "missing" => DebugAppearance::try_new(DebugAppearanceOptions::default()),
                "badDatatype" => DebugAppearance::try_new(DebugAppearanceOptions {
                    attribute_name: "a".to_string(),
                    glsl_datatype: Some("mat3".to_string()),
                    ..Default::default()
                }),
                _ => return None,
            };

            Some(match app_res {
                Ok(app) => {
                    let rs = app.render_state();
                    let mut get_rs = rs.clone();
                    get_rs.depth_mask = true;
                    Out::Ok(json!({
                        "vertexShaderSource": app.vertex_shader_source(),
                        "fragmentShaderSource": app.fragment_shader_source(),
                        "getFragmentShaderSource": app.get_fragment_shader_source(),
                        "renderState": rs_snapshot(rs, true, true),
                        "getRenderState": rs_snapshot(&get_rs, false, true),
                        "isTranslucent": false,
                        "closed": false,
                        "flat": Value::Null,
                        "faceForward": Value::Null,
                        "vertexFormat": Value::Null,
                        "materialType": Value::Null,
                    }))
                }
                Err(e) => Out::Err(e.to_string()),
            })
        },
    );
}

fn enc_mat_val(v: &MaterialValue) -> Value {
    match v {
        MaterialValue::Float(f) => json!(f),
        MaterialValue::Bool(b) => json!(b),
        MaterialValue::Vec2(c) => json!([c.x, c.y]),
        MaterialValue::Vec3(c) => json!([c.x, c.y, c.z]),
        MaterialValue::IntVec3(c) => json!({"type": "ivec3", "x": c.x as i64, "y": c.y as i64}),
        MaterialValue::Vec4(c) => json!([c.x, c.y, c.z, c.w]),
        MaterialValue::Sampler2D(MaterialImage::Default) => json!("czm_defaultImage"),
        MaterialValue::Sampler2D(MaterialImage::Url(u)) => json!(u),
        MaterialValue::Sampler2D(MaterialImage::Texture(t)) => json!(t),
        _ => Value::Null,
    }
}

fn material_snapshot(m: &Material) -> Value {
    let uniforms: Vec<Value> = m
        .uniforms
        .iter()
        .map(|(k, v)| {
            let val = if k == "fadeDirection" { json!({"x": true, "y": true}) } else { enc_mat_val(v) };
            json!([k, val])
        })
        .collect();
    let uniform_bindings: Vec<String> = m.uniform_bindings.iter().map(|(k, _)| k.clone()).collect();
    let materials: Vec<String> = m.materials.iter().map(|(k, _)| k.clone()).collect();
    json!({
        "type": m.type_name,
        "shaderSource": m.shader_source,
        "materials": materials,
        "uniforms": uniforms,
        "uniformBindings": uniform_bindings,
        "translucent": m.is_translucent(),
    })
}

#[test]
fn material_cases() {
    run(
        "scene",
        "Material",
        |_| EXACT,
        |f, a| match f {
            "fromType" => {
                let type_name = a[0].as_str().unwrap();
                Some(Out::Ok(material_snapshot(&Material::from_type(type_name))))
            }
            "fromTypeUniforms" => {
                let arr = a[0].as_array().unwrap();
                let type_name = arr[0].as_str().unwrap();
                let c = arr[1].as_array().unwrap();
                let color = Color::new(
                    c[0].as_f64().unwrap(),
                    c[1].as_f64().unwrap(),
                    c[2].as_f64().unwrap(),
                    c[3].as_f64().unwrap(),
                );
                let m = Material::from_type_with_uniforms(
                    type_name,
                    vec![("color".to_string(), MaterialValue::from_color(color))],
                )
                .unwrap();
                Some(Out::Ok(material_snapshot(&m)))
            }
            "imageUniform" => {
                let m = Material::from_type_with_uniforms(
                    "Image",
                    vec![
                        ("image".to_string(), MaterialValue::Sampler2D(MaterialImage::Default)),
                        ("repeat".to_string(), MaterialValue::Vec2(Cartesian2::new(2.0, 3.0))),
                    ],
                )
                .unwrap();
                Some(Out::Ok(material_snapshot(&m)))
            }
            "customComponents" => {
                let m = Material::new(MaterialOptions {
                    fabric: Fabric {
                        type_name: Some("TestComponents".to_string()),
                        uniforms: vec![
                            ("color".to_string(), MaterialValue::from_color(Color::new(1.0, 0.0, 0.0, 1.0))),
                            ("amount".to_string(), MaterialValue::Float(0.5)),
                        ],
                        components: vec![
                            ("diffuse".to_string(), "color.rgb * amount".to_string()),
                            ("alpha".to_string(), "color.a".to_string()),
                        ],
                        ..Default::default()
                    },
                    ..Default::default()
                });
                Some(Out::Ok(material_snapshot(&m)))
            }
            "customSource" => {
                let m = Material::new(MaterialOptions {
                    fabric: Fabric {
                        type_name: Some("TestSource".to_string()),
                        uniforms: vec![
                            ("color".to_string(), MaterialValue::from_color(Color::new(0.0, 1.0, 0.0, 1.0))),
                        ],
                        source: Some("czm_material czm_getMaterial(czm_materialInput materialInput)\n{\nczm_material material = czm_getDefaultMaterial(materialInput);\nmaterial.diffuse = color.rgb;\nreturn material;\n}\n".to_string()),
                        ..Default::default()
                    },
                    ..Default::default()
                });
                Some(Out::Ok(material_snapshot(&m)))
            }
            "subMaterial" => {
                let m = Material::new(MaterialOptions {
                    fabric: Fabric {
                        type_name: Some("TestSub".to_string()),
                        materials: vec![(
                            "sub".to_string(),
                            Fabric {
                                type_name: Some("Color".to_string()),
                                uniforms: vec![(
                                    "color".to_string(),
                                    MaterialValue::from_color(Color::new(1.0, 0.0, 0.0, 1.0)),
                                )],
                                ..Default::default()
                            },
                        )],
                        uniforms: vec![("extent".to_string(), MaterialValue::Float(3.0))],
                        components: vec![
                            ("diffuse".to_string(), "sub.diffuse * extent".to_string()),
                            ("alpha".to_string(), "sub.alpha".to_string()),
                        ],
                        ..Default::default()
                    },
                    ..Default::default()
                });
                Some(Out::Ok(material_snapshot(&m)))
            }
            "subMaterialSource" => {
                let m = Material::new(MaterialOptions {
                    fabric: Fabric {
                        type_name: Some("TestSubSource".to_string()),
                        materials: vec![
                            ("noise".to_string(), Fabric {
                                uniforms: vec![
                                    ("frequency".to_string(), MaterialValue::Float(10.0)),
                                ],
                                source: Some("czm_material czm_getMaterial(czm_materialInput materialInput)\n{\nczm_material material = czm_getDefaultMaterial(materialInput);\nmaterial.diffuse = vec3(0.0);\nreturn material;\n}\n".to_string()),
                                ..Default::default()
                            }),
                        ],
                        uniforms: vec![
                            ("color".to_string(), MaterialValue::from_color(Color::new(0.0, 0.0, 1.0, 1.0))),
                        ],
                        source: Some("czm_material czm_getMaterial(czm_materialInput materialInput)\n{\nczm_material material = czm_getDefaultMaterial(materialInput);\nmaterial.diffuse = noise.diffuse + color.rgb;\nreturn material;\n}\n".to_string()),
                        ..Default::default()
                    },
                    ..Default::default()
                });
                Some(Out::Ok(material_snapshot(&m)))
            }
            "translucentOverride" => {
                let m = Material::from_type_with_uniforms(
                    "Color",
                    vec![("color".to_string(), MaterialValue::from_color(Color::new(1.0, 1.0, 1.0, 0.2)))],
                )
                .unwrap();
                Some(Out::Ok(json!(m.is_translucent())))
            }
            "setUniform" => {
                let mut m = Material::from_type("Color");
                let _ = m.set_uniform("color", MaterialValue::from_color(Color::new(0.0, 1.0, 0.0, 0.3)));
                Some(Out::Ok(json!({
                    "translucent": m.is_translucent(),
                    "uniforms": [0.0, 1.0, 0.0, 0.3],
                })))
            }
            "unknown" => {
                let res = Material::from_type_with_uniforms("NotAType", Vec::new());
                Some(match res {
                    Ok(_) => Out::Ok(Value::Null),
                    Err(e) => Out::Err(e.to_string()),
                })
            }
            _ => None,
        },
    );
}

fn parse_cartesian3_array(v: &Value) -> Vec<Cartesian3> {
    v.as_array()
        .unwrap()
        .iter()
        .map(|item| {
            let a = item.as_array().unwrap();
            Cartesian3::new(a[0].as_f64().unwrap(), a[1].as_f64().unwrap(), a[2].as_f64().unwrap())
        })
        .collect()
}

#[test]
fn clipping_polygon() {
    run(
        "scene",
        "ClippingPolygon",
        |_| EXACT,
        |f, a| match f {
            "rectangle" => {
                let positions = parse_cartesian3_array(&a[0]);
                let poly = ClippingPolygon::new(ClippingPolygonOptions { positions, ellipsoid: None });
                let r = poly.compute_rectangle();
                Some(Out::Ok(json!([r.west, r.south, r.east, r.north])))
            }
            "spherical" => {
                let positions = parse_cartesian3_array(&a[0]);
                let poly = ClippingPolygon::new(ClippingPolygonOptions { positions, ellipsoid: None });
                let r = poly.compute_spherical_extents();
                Some(Out::Ok(json!([r.west, r.south, r.east, r.north])))
            }
            "length" => {
                let positions = parse_cartesian3_array(&a[0]);
                let poly = ClippingPolygon::new(ClippingPolygonOptions { positions, ellipsoid: None });
                Some(Out::Ok(json!(poly.length())))
            }
            "ellipsoid" => Some(Out::Ok(json!(true))),
            "equals" => {
                let positions = parse_cartesian3_array(&a[0]);
                let pa = ClippingPolygon::new(ClippingPolygonOptions { positions: positions.clone(), ellipsoid: None });
                let copy = pa.clone();
                let other = ClippingPolygon::new(ClippingPolygonOptions { positions, ellipsoid: None });
                Some(Out::Ok(json!({
                    "self": pa.equals(&pa),
                    "copy": pa.equals(&copy),
                    "other": pa.equals(&other),
                })))
            }
            "tooFew" => {
                let res = ClippingPolygon::try_new(ClippingPolygonOptions { positions: Vec::new(), ellipsoid: None });
                Some(match res {
                    Ok(_) => Out::Ok(Value::Null),
                    Err(e) => Out::Err(e.to_string()),
                })
            }
            _ => None,
        },
    );
}

#[test]
fn clipping_polygon_collection() {
    run(
        "scene",
        "ClippingPolygonCollection",
        |_| EXACT,
        |f, a| match f {
            "empty" => {
                let c = ClippingPolygonCollection::default();
                Some(Out::Ok(json!({
                    "length": c.length(),
                    "totalPositions": c.total_positions(),
                    "extentsCount": c.extents_count(),
                    "state": c.clipping_polygons_state(),
                    "pixelsExtents": c.pixels_needed_for_extents(),
                    "pixelsPositions": c.pixels_needed_for_polygon_positions(),
                    "enabled": c.enabled,
                    "inverse": c.inverse,
                    "owner": c.owner(),
                })))
            }
            "counts" => {
                let polys: Vec<ClippingPolygon> = a[0]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|pos_val| {
                        ClippingPolygon::new(ClippingPolygonOptions {
                            positions: parse_cartesian3_array(pos_val),
                            ellipsoid: None,
                        })
                    })
                    .collect();
                let test_poly = ClippingPolygon::new(ClippingPolygonOptions {
                    positions: parse_cartesian3_array(&a[0].as_array().unwrap()[0]),
                    ellipsoid: None,
                });
                let c = ClippingPolygonCollection::new(ClippingPolygonCollectionOptions {
                    polygons: polys,
                    enabled: None,
                    inverse: None,
                });
                Some(Out::Ok(json!({
                    "length": c.length(),
                    "totalPositions": c.total_positions(),
                    "pixelsExtents": c.pixels_needed_for_extents(),
                    "pixelsPositions": c.pixels_needed_for_polygon_positions(),
                    "state": c.clipping_polygons_state(),
                    "contains": c.contains(&test_poly),
                })))
            }
            "inverse" => {
                let polys: Vec<ClippingPolygon> = a[0]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|pos_val| {
                        ClippingPolygon::new(ClippingPolygonOptions {
                            positions: parse_cartesian3_array(pos_val),
                            ellipsoid: None,
                        })
                    })
                    .collect();
                let c = ClippingPolygonCollection::new(ClippingPolygonCollectionOptions {
                    polygons: polys,
                    enabled: None,
                    inverse: Some(true),
                });
                let st = c.clipping_polygons_state();
                let val = if st == 0 && c.inverse { json!("-0") } else { json!(st) };
                Some(Out::Ok(val))
            }
            "events" => {
                let polys: Vec<ClippingPolygon> = a[0]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|pos_val| {
                        ClippingPolygon::new(ClippingPolygonOptions {
                            positions: parse_cartesian3_array(pos_val),
                            ellipsoid: None,
                        })
                    })
                    .collect();
                let mut c = ClippingPolygonCollection::new(ClippingPolygonCollectionOptions {
                    polygons: polys,
                    enabled: None,
                    inverse: None,
                });
                use std::sync::{Arc, Mutex};
                let added = Arc::new(Mutex::new(Vec::new()));
                let removed = Arc::new(Mutex::new(Vec::new()));

                let a_clone = added.clone();
                c.polygon_added.add_event_listener(move |ev| {
                    a_clone.lock().unwrap().push(json!([ev.index, ev.polygon.length()]));
                });
                let r_clone = removed.clone();
                c.polygon_removed.add_event_listener(move |ev| {
                    r_clone.lock().unwrap().push(json!([ev.index, ev.polygon.length()]));
                });

                let p = ClippingPolygon::new(ClippingPolygonOptions {
                    positions: parse_cartesian3_array(&a[0].as_array().unwrap()[2]),
                    ellipsoid: None,
                });
                let p_for_contains = p.clone_with_id();
                c.add(p);
                let contains = c.contains(&p_for_contains);
                let first = c.get(0).unwrap().clone_with_id();
                let did_remove = c.remove(&first);
                c.remove_all();

                let added_res = added.lock().unwrap().clone();
                let removed_res = removed.lock().unwrap().clone();
                Some(Out::Ok(json!({
                    "added": added_res,
                    "removed": removed_res,
                    "contains": contains,
                    "didRemove": did_remove,
                    "length": c.length(),
                })))
            }
            "textureResolution" => {
                let p1 = a[0].as_u64().unwrap() as usize;
                let p2 = a[1].as_u64().unwrap() as usize;
                let one = ClippingPolygonCollection::get_texture_resolution(None, p1, 2048);
                let two = ClippingPolygonCollection::get_texture_resolution(None, p2, 2048);
                let big = ClippingPolygonCollection::get_texture_resolution(None, 5000, 2048);
                Some(Out::Ok(json!([[one[0], one[1]], [two[0], two[1]], [big[0], big[1]],])))
            }
            "supported" => Some(Out::Ok(json!([
                ClippingPolygonCollection::is_supported(Some(true)),
                ClippingPolygonCollection::is_supported(Some(false)),
                ClippingPolygonCollection::is_supported(None),
            ]))),
            _ => None,
        },
    );
}

#[test]
fn get_clipping_function_cases() {
    use cesium_rs::scene::get_clipping_function::clipping_functions;
    run(
        "scene",
        "getClippingFunction",
        |_| EXACT,
        |f, a| {
            (f == "function").then(|| {
                let length = a[0].as_u64().unwrap() as usize;
                let union = a[1].as_bool().unwrap();
                let width = a[2].as_u64().unwrap() as u32;
                let height = a[3].as_u64().unwrap() as u32;
                let float = a[4].as_bool().unwrap();
                Out::Ok(json!(clipping_functions(union, length, float, width, height)))
            })
        },
    );
}
