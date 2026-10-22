//! Scene/GltfPipeline replayed against the pinned CesiumJS fixtures.
//!
//! Buffer sources travel as `{ "$source": [bytes], "pad": n, "tail": m }`: a
//! view that starts `n` bytes into its buffer and has `m` more bytes after it.
//! Pad and tail bytes are 0xEE.
mod common;
use cesium_rs::core::ComponentDatatype;
use cesium_rs::scene::gltf_pipeline::{self as pipeline, for_each, BufferSource, Gltf, Id, PipelineError};
use common::*;
use serde_json::{json, Value};
use std::sync::Arc;

/// Replaces `$source` objects with sources of a new document.
fn decode(value: &Value) -> Gltf {
    let mut gltf = Gltf::default();
    gltf.json = decode_into(&mut gltf, value);
    gltf
}

fn decode_into(gltf: &mut Gltf, value: &Value) -> Value {
    match value {
        Value::Array(items) => Value::Array(items.iter().map(|v| decode_into(gltf, v)).collect()),
        Value::Object(map) if map.contains_key("$source") => gltf.add_source(source(value)),
        Value::Object(map) => Value::Object(map.iter().map(|(k, v)| (k.clone(), decode_into(gltf, v))).collect()),
        other => other.clone(),
    }
}

fn source(value: &Value) -> BufferSource {
    let pad = value.get("pad").map_or(0, |p| n(p) as usize);
    let tail = value.get("tail").map_or(0, |t| n(t) as usize);
    let bytes: Vec<u8> = nums(&value["$source"]).into_iter().map(|b| b as u8).collect();
    let length = bytes.len();
    let mut buffer = vec![0xEE; pad];
    buffer.extend(bytes);
    buffer.extend(std::iter::repeat_n(0xEE, tail));
    BufferSource::view(Arc::new(buffer), pad, length)
}

/// The JSON with sources written back as `{ "$source": [view bytes] }`.
fn encode(gltf: &Gltf, value: &Value) -> Value {
    match value {
        Value::Array(items) => Value::Array(items.iter().map(|v| encode(gltf, v)).collect()),
        Value::Object(_) if Gltf::is_source(value) => {
            json!({ "$source": gltf.source(value).expect("source").bytes().to_vec() })
        }
        Value::Object(map) => Value::Object(map.iter().map(|(k, v)| (k.clone(), encode(gltf, v))).collect()),
        other => other.clone(),
    }
}

fn document(gltf: &Gltf) -> Value {
    encode(gltf, &gltf.json)
}

fn out<T>(result: Result<T, PipelineError>, f: impl FnOnce(T) -> Value) -> Option<Out> {
    Some(match result {
        Ok(v) => Out::Ok(f(v)),
        Err(e) => Out::Err(e.to_string()),
    })
}

fn numbers(values: &[f64]) -> Value {
    Value::Array(values.iter().map(|&v| num(v)).collect())
}

fn id_value(id: &Id) -> Value {
    id.value()
}

#[test]
fn extension_lists() {
    run("scene/gltf_pipeline", "usesExtension", |_| EXACT, |_, a| ok(pipeline::uses_extension(&a[0], s(&a[1]))));
    run(
        "scene/gltf_pipeline",
        "addExtensionsUsed",
        |_| EXACT,
        |_, a| {
            let mut g = a[0].clone();
            out(pipeline::add_extensions_used(&mut g, s(&a[1])), |_| g)
        },
    );
    run(
        "scene/gltf_pipeline",
        "addExtensionsRequired",
        |_| EXACT,
        |_, a| {
            let mut g = a[0].clone();
            out(pipeline::add_extensions_required(&mut g, s(&a[1])), |_| g)
        },
    );
    run(
        "scene/gltf_pipeline",
        "removeExtensionsRequired",
        |_| EXACT,
        |_, a| {
            let mut g = a[0].clone();
            pipeline::remove_extensions_required(&mut g, s(&a[1]));
            ok(g)
        },
    );
    run(
        "scene/gltf_pipeline",
        "removeExtensionsUsed",
        |_| EXACT,
        |_, a| {
            let mut g = a[0].clone();
            pipeline::remove_extensions_used(&mut g, s(&a[1]));
            ok(g)
        },
    );
    run(
        "scene/gltf_pipeline",
        "removeExtension",
        |_| EXACT,
        |_, a| {
            let mut g = decode(&a[0]);
            let removed = pipeline::remove_extension(&mut g.json, s(&a[1]));
            ok(json!([removed.unwrap_or(Value::Null), document(&g)]))
        },
    );
}

#[test]
fn small_helpers() {
    run(
        "scene/gltf_pipeline",
        "addToArray",
        |_| EXACT,
        |_, a| {
            let mut array = a[0].as_array().unwrap().clone();
            // Fixture arguments write -0 as "-0".
            let element = if a[1] == json!("-0") { json!(-0.0) } else { a[1].clone() };
            let index = pipeline::add_to_array(&mut array, element, a[2].as_bool().unwrap());
            ok(json!([index, array]))
        },
    );
    run(
        "scene/gltf_pipeline",
        "numberOfComponentsForType",
        |_| EXACT,
        |_, a| ok(pipeline::number_of_components_for_type(s(&a[0])).map(|c| c as u32)),
    );
    run(
        "scene/gltf_pipeline",
        "getAccessorByteStride",
        |_| EXACT,
        |_, a| out(pipeline::get_accessor_byte_stride(&a[0], &a[1]), num),
    );
    run(
        "scene/gltf_pipeline",
        "addBuffer",
        |_| EXACT,
        |_, a| {
            let mut g = decode(&a[0]);
            let bytes = nums(&a[1]).into_iter().map(|b| b as u8).collect();
            let index = pipeline::add_buffer(&mut g, BufferSource::new(bytes));
            out(index, |i| json!([i, document(&g)]))
        },
    );
    run(
        "scene/gltf_pipeline",
        "addPipelineExtras",
        |_| EXACT,
        |_, a| {
            let mut g = a[0].clone();
            out(pipeline::add_pipeline_extras(&mut g), |_| g)
        },
    );
    run(
        "scene/gltf_pipeline",
        "removePipelineExtras",
        |_| EXACT,
        |_, a| {
            let mut g = a[0].clone();
            out(pipeline::remove_pipeline_extras(&mut g), |_| g)
        },
    );
}

/// Calls the top-level `ForEach` function `name`.
fn top_level(name: &str, gltf: &Value, handler: impl FnMut(&Value, Id) -> Option<()>) {
    match name {
        "accessor" => for_each::accessor(gltf, handler),
        "animation" => for_each::animation(gltf, handler),
        "buffer" => for_each::buffer(gltf, handler),
        "bufferView" => for_each::buffer_view(gltf, handler),
        "camera" => for_each::camera(gltf, handler),
        "image" => for_each::image(gltf, handler),
        "material" => for_each::material(gltf, handler),
        "mesh" => for_each::mesh(gltf, handler),
        "node" => for_each::node(gltf, handler),
        "program" => for_each::program(gltf, handler),
        "sampler" => for_each::sampler(gltf, handler),
        "scene" => for_each::scene(gltf, handler),
        "shader" => for_each::shader(gltf, handler),
        "skin" => for_each::skin(gltf, handler),
        "technique" => for_each::technique(gltf, handler),
        "texture" => for_each::texture(gltf, handler),
        other => panic!("unknown ForEach.{other}"),
    };
}

fn nested_visits(g: &Gltf) -> Value {
    let json = &g.json;
    let mut out = Vec::new();
    for_each::mesh::<()>(json, |mesh, _| {
        for_each::mesh_primitive(mesh, |primitive, i| {
            out.push(json!(["primitive", i]));
            for_each::mesh_primitive_attribute::<()>(primitive, |a, semantic| {
                out.push(json!(["attribute", a, semantic]));
                None
            });
            for_each::mesh_primitive_target::<()>(primitive, |target, j| {
                out.push(json!(["target", j]));
                for_each::mesh_primitive_target_attribute::<()>(target, |a, semantic| {
                    out.push(json!(["targetAttribute", a, semantic]));
                    None
                });
                None
            });
            None
        })
    });
    for_each::animation::<()>(json, |animation, _| {
        for_each::animation_channel::<()>(animation, |_, i| {
            out.push(json!(["channel", i]));
            None
        });
        for_each::animation_sampler::<()>(animation, |_, i| {
            out.push(json!(["sampler", i]));
            None
        });
        None
    });
    for_each::material::<()>(json, |material, _| {
        for_each::material_value(material, |value, name| {
            out.push(json!(["value", encode(g, value), name]));
            None
        })
    });
    for_each::skin::<()>(json, |skin, _| {
        for_each::skin_joint(skin, |joint| {
            out.push(json!(["joint", joint]));
            None
        })
    });
    for_each::technique::<()>(json, |technique, _| {
        for_each::technique_attribute::<()>(technique, |v, name| {
            out.push(json!(["techniqueAttribute", v, name]));
            None
        });
        for_each::technique_uniform::<()>(technique, |v, name| {
            out.push(json!(["techniqueUniform", encode(g, v), name]));
            None
        });
        for_each::technique_parameter::<()>(technique, |v, name| {
            out.push(json!(["techniqueParameter", encode(g, v), name]));
            None
        });
        None
    });
    for_each::scene::<()>(json, |scene, _| {
        for_each::node_in_scene(json, scene, |_, id| {
            out.push(json!(["nodeInScene", id]));
            None
        })
    });
    Value::Array(out)
}

#[test]
fn for_each() {
    run(
        "scene/gltf_pipeline",
        "ForEach",
        |_| EXACT,
        |f, a| match f {
            "topLevel" => {
                let g = decode(&a[1]);
                let mut visits = Vec::new();
                top_level(s(&a[0]), &g.json, |value, id| {
                    visits.push(json!([encode(&g, value), id_value(&id)]));
                    None
                });
                ok(Value::Array(visits))
            }
            "stopsEarly" => {
                let g = decode(&a[0]);
                let mut calls = 0;
                let found = for_each::accessor(&g.json, |_, id| {
                    calls += 1;
                    (calls == 2).then(|| id_value(&id))
                });
                ok(json!([found.unwrap_or(Value::Null), calls]))
            }
            "accessors" => {
                let g = decode(&a[1]);
                let mut visits = Vec::new();
                let visit = |id: &Value| {
                    visits.push(id.clone());
                    None::<()>
                };
                match s(&a[0]) {
                    "accessorContainingVertexAttributeData" => {
                        for_each::accessor_containing_vertex_attribute_data(&g.json, visit);
                    }
                    _ => {
                        for_each::accessor_containing_index_data(&g.json, visit);
                    }
                }
                ok(Value::Array(visits))
            }
            "accessorWithSemantic" => {
                let g = decode(&a[1]);
                let mut visits = Vec::new();
                for_each::accessor_with_semantic::<()>(&g.json, s(&a[0]), |id| {
                    visits.push(id.clone());
                    None
                });
                ok(Value::Array(visits))
            }
            "nested" => ok(nested_visits(&decode(&a[0]))),
            "objectLegacy" => {
                let mut visits = Vec::new();
                for_each::object_legacy::<()>(Some(&a[0]), |v, id| {
                    visits.push(json!([v, id]));
                    None
                });
                ok(Value::Array(visits))
            }
            _ => None,
        },
    );
}

#[test]
fn accessor_readers() {
    run(
        "scene/gltf_pipeline",
        "getComponentReader",
        |_| EXACT,
        |_, a| {
            let datatype = ComponentDatatype::from_gl(n(&a[0]) as u32).unwrap();
            let bytes: Vec<u8> = nums(&a[1]).into_iter().map(|b| b as u8).collect();
            let count = n(&a[3]) as usize;
            let mut result = vec![0.0; count];
            let read = pipeline::get_component_reader(datatype)(&bytes, n(&a[2]), count, n(&a[4]), &mut result);
            out(read, |_| numbers(&result))
        },
    );
    // An accessor given inline, or by id: a glTF 1.0 key or a 2.0 index.
    let accessor = |g: &Gltf, v: &Value| -> Value {
        match v {
            Value::Object(_) => v.clone(),
            Value::String(key) => g.json["accessors"][key.as_str()].clone(),
            index => g.json["accessors"][n(index) as usize].clone(),
        }
    };
    run(
        "scene/gltf_pipeline",
        "readAccessorPacked",
        |_| EXACT,
        |_, a| {
            let g = decode(&a[0]);
            out(pipeline::read_accessor_packed(&g, &accessor(&g, &a[1])), |v| numbers(&v))
        },
    );
    run(
        "scene/gltf_pipeline",
        "findAccessorMinMax",
        |_| EXACT,
        |_, a| {
            let g = decode(&a[0]);
            out(
                pipeline::find_accessor_min_max(&g, &accessor(&g, &a[1])),
                |m| json!({ "min": numbers(&m.min), "max": numbers(&m.max) }),
            )
        },
    );
    run(
        "scene/gltf_pipeline",
        "updateAccessorComponentTypes",
        |_| EXACT,
        |_, a| {
            let mut g = decode(&a[0]);
            out(pipeline::update_accessor_component_types(&mut g), |_| document(&g))
        },
    );
}

#[test]
fn materials_and_defaults() {
    run(
        "scene/gltf_pipeline",
        "forEachTextureInMaterial",
        |_| EXACT,
        |f, a| match f {
            "visits" => {
                let mut visits = Vec::new();
                pipeline::for_each_texture_in_material::<()>(&a[0], |index, info| {
                    visits.push(json!([index.cloned().unwrap_or(Value::Null), info]));
                    None
                });
                ok(Value::Array(visits))
            }
            "stopsEarly" => ok(pipeline::for_each_texture_in_material(&a[0], |index, _| {
                (index.and_then(Value::as_f64) == Some(1.0)).then(|| json!("found"))
            })
            .unwrap_or(Value::Null)),
            _ => None,
        },
    );
    run(
        "scene/gltf_pipeline",
        "addDefaults",
        |_| EXACT,
        |_, a| {
            let mut g = decode(&a[0]);
            out(pipeline::add_defaults(&mut g.json), |_| document(&g))
        },
    );
}

#[test]
fn parse_glb() {
    run(
        "scene/gltf_pipeline",
        "parseGlb",
        |_| EXACT,
        |_, a| out(pipeline::parse_glb(&source(&a[0])), |g| g.map_or(Value::Null, |g| document(&g))),
    );
}

#[test]
fn techniques_and_cleanup() {
    run(
        "scene/gltf_pipeline",
        "moveTechniqueRenderStates",
        |_| EXACT,
        |_, a| {
            let mut g = decode(&a[0]);
            out(pipeline::move_technique_render_states(&mut g.json), |_| document(&g))
        },
    );
    run(
        "scene/gltf_pipeline",
        "moveTechniquesToExtension",
        |_| EXACT,
        |_, a| {
            let mut g = decode(&a[0]);
            out(pipeline::move_techniques_to_extension(&mut g.json), |_| document(&g))
        },
    );
    run(
        "scene/gltf_pipeline",
        "removeUnusedElements",
        |_| EXACT,
        |_, a| {
            let mut g = decode(&a[0]);
            let types: Option<Vec<_>> =
                a[1].as_array().map(|t| t.iter().filter_map(|t| pipeline::ElementType::from_name(s(t))).collect());
            out(pipeline::remove_unused_elements(&mut g.json, types.as_deref()), |_| document(&g))
        },
    );
}

#[test]
fn update_version() {
    // TIGHT: the sRGB conversion uses the platform `pow`, which can differ
    // from V8's `Math.pow` in the last bit.
    run(
        "scene/gltf_pipeline",
        "updateVersion",
        |_| TIGHT,
        |_, a| {
            let mut g = decode(&a[0]);
            let o = &a[1];
            let names = |key: &str| {
                o.get(key)
                    .and_then(Value::as_array)
                    .map(|v| v.iter().map(|s| s.as_str().unwrap().to_string()).collect())
            };
            let options = pipeline::UpdateVersionOptions {
                target_version: o.get("targetVersion").and_then(Value::as_str).map(str::to_string),
                keep_legacy_extensions: o.get("keepLegacyExtensions").and_then(Value::as_bool).unwrap_or(false),
                base_color_texture_names: names("baseColorTextureNames"),
                base_color_factor_names: names("baseColorFactorNames"),
            };
            out(pipeline::update_version(&mut g, &options), |_| document(&g))
        },
    );
}
