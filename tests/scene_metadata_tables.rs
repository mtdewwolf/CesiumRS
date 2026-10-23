//! 3D Metadata property tables, the batch table hierarchy and property
//! attributes replayed against the pinned CesiumJS fixtures.
//!
//! Each case builds one object and runs a script of operations, each
//! recorded as `{ "ok": value }` or `{ "throws": errorName }`. Values use
//! the tags of `scene_metadata.rs`; in binary table values an element
//! Cesium reads as `undefined` is null.
mod common;
use cesium_rs::core::{Cartesian2, Cartesian3, Cartesian4, Matrix2, Matrix3, Matrix4};
use cesium_rs::scene::*;
use common::*;
use serde_json::{json, Value};
use std::sync::{Arc, OnceLock};

/// A value as the fixtures tag it. With `undefined_null`, `Null` is the
/// `undefined` a binary table reads inside an array.
fn tag(value: &MetadataValue, undefined_null: bool) -> Value {
    let all = |c: &[f64]| c.iter().map(|&x| num(x)).collect::<Vec<_>>();
    match value {
        MetadataValue::Null if undefined_null => Value::Null,
        MetadataValue::Null => json!({ "$null": true }),
        MetadataValue::Boolean(b) => json!(b),
        MetadataValue::Number(x) => num(*x),
        MetadataValue::BigInt(b) => json!({ "$bigint": b.to_string() }),
        MetadataValue::String(s) => json!(s),
        MetadataValue::Array(items) => Value::Array(items.iter().map(|v| tag(v, undefined_null)).collect()),
        MetadataValue::Cartesian2(c) => json!({ "$Cartesian2": all(&[c.x, c.y]) }),
        MetadataValue::Cartesian3(c) => json!({ "$Cartesian3": all(&[c.x, c.y, c.z]) }),
        MetadataValue::Cartesian4(c) => json!({ "$Cartesian4": all(&[c.x, c.y, c.z, c.w]) }),
        MetadataValue::Matrix2(m) => json!({ "$Matrix2": all(&m.0) }),
        MetadataValue::Matrix3(m) => json!({ "$Matrix3": all(&m.0) }),
        MetadataValue::Matrix4(m) => json!({ "$Matrix4": all(&m.0) }),
        MetadataValue::Object(v) => v.clone(),
    }
}

fn val(value: Option<&MetadataValue>) -> Value {
    value.map_or(Value::Null, |v| tag(v, false))
}

fn tval(value: Option<&MetadataValue>) -> Value {
    value.map_or(Value::Null, |v| tag(v, true))
}

/// A tagged fixture value; `null` is `undefined`.
fn unval(value: &Value) -> Option<MetadataValue> {
    Some(match value {
        Value::Null => return None,
        Value::Bool(b) => MetadataValue::Boolean(*b),
        Value::Number(_) => MetadataValue::Number(n(value)),
        Value::String(s) if matches!(s.as_str(), "NaN" | "Infinity" | "-Infinity" | "-0") => {
            MetadataValue::Number(n(value))
        }
        Value::String(s) => MetadataValue::String(s.clone()),
        Value::Array(items) => {
            MetadataValue::Array(items.iter().map(|v| unval(v).unwrap_or(MetadataValue::Null)).collect())
        }
        Value::Object(map) => {
            if map.contains_key("$null") {
                MetadataValue::Null
            } else if let Some(b) = map.get("$bigint") {
                MetadataValue::BigInt(s(b).parse().unwrap())
            } else if let Some(c) = map.get("$Cartesian2") {
                MetadataValue::Cartesian2(Cartesian2::unpack(&nums(c), 0))
            } else if let Some(c) = map.get("$Cartesian3") {
                MetadataValue::Cartesian3(Cartesian3::unpack(&nums(c), 0))
            } else if let Some(c) = map.get("$Cartesian4") {
                MetadataValue::Cartesian4(Cartesian4::unpack(&nums(c), 0))
            } else if let Some(m) = map.get("$Matrix2") {
                MetadataValue::Matrix2(Matrix2::unpack(&nums(m), 0))
            } else if let Some(m) = map.get("$Matrix3") {
                MetadataValue::Matrix3(Matrix3::unpack(&nums(m), 0))
            } else if let Some(m) = map.get("$Matrix4") {
                MetadataValue::Matrix4(Matrix4::unpack(&nums(m), 0))
            } else {
                MetadataValue::Object(value.clone())
            }
        }
    })
}

/// An operation's value, which may not be `undefined`.
fn arg(value: &Value) -> MetadataValue {
    unval(value).unwrap_or(MetadataValue::Null)
}

fn kind(e: &MetadataError) -> &'static str {
    match e {
        MetadataError::Developer(_) => "DeveloperError",
        MetadataError::Type(_) => "TypeError",
        MetadataError::Range(_) => "RangeError",
        MetadataError::Runtime(_) => "RuntimeError",
        MetadataError::Syntax(_) => "SyntaxError",
    }
}

fn attempt(r: Result<Value, MetadataError>) -> Value {
    match r {
        Ok(v) => json!({ "ok": v }),
        Err(e) => json!({ "throws": kind(&e) }),
    }
}

fn idx(v: &Value) -> usize {
    n(v) as usize
}

fn opt(v: Option<&Value>) -> Value {
    v.cloned().unwrap_or(Value::Null)
}

fn typed(array: Option<&MetadataTypedArray>) -> Value {
    match array {
        None => Value::Null,
        Some(MetadataTypedArray::Numbers(a)) => Value::Array(a.to_f64_vec().into_iter().map(num).collect()),
        Some(MetadataTypedArray::BigInt64(v)) => v.iter().map(|x| json!({ "$bigint": x.to_string() })).collect(),
        Some(MetadataTypedArray::BigUint64(v)) => v.iter().map(|x| json!({ "$bigint": x.to_string() })).collect(),
    }
}

/// The buffers of `{ id: { buffer, byteOffset } }`.
fn buffers(views: &Value) -> Vec<(String, Vec<u8>, usize)> {
    views
        .as_object()
        .map(|m| {
            m.iter()
                .map(|(id, v)| {
                    (id.clone(), nums(&v["buffer"]).into_iter().map(|b| b as u8).collect(), idx(&v["byteOffset"]))
                })
                .collect()
        })
        .unwrap_or_default()
}

fn byte_views(buffers: &[(String, Vec<u8>, usize)]) -> JsMap<ByteView<'_>> {
    buffers.iter().map(|(id, b, o)| (id.clone(), ByteView { buffer: b, byte_offset: *o })).collect()
}

/// The schema every case uses: the first `MetadataTableProperty` case's input.
fn schema() -> &'static MetadataSchema {
    static SCHEMA: OnceLock<MetadataSchema> = OnceLock::new();
    SCHEMA.get_or_init(|| MetadataSchema::from_json(&load("scene", "MetadataTableProperty")[0].a[0]).unwrap())
}

fn class(id: &str) -> Arc<MetadataClass> {
    schema().classes.get(id).unwrap().clone()
}

fn script(ops: &Value, mut op: impl FnMut(&str, &[Value]) -> Result<Value, MetadataError>) -> Option<Out> {
    let results = ops.as_array().unwrap().iter().map(|o| {
        let o = o.as_array().unwrap();
        attempt(op(s(&o[0]), &o[1..]))
    });
    Some(Out::Ok(Value::Array(results.collect())))
}

fn build<T>(r: Result<T, MetadataError>) -> Result<T, Option<Out>> {
    r.map_err(|e| Some(Out::Err(e.to_string())))
}

fn table_from(spec: &Value) -> Result<MetadataTable, MetadataError> {
    let bufs = buffers(&spec["views"]);
    let views = byte_views(&bufs);
    let properties = Some(&spec["properties"]).filter(|v| !v.is_null());
    MetadataTable::new(idx(&spec["count"]), properties, class(s(&spec["cls"])), &views)
}

#[test]
fn metadata_table_property() {
    run(
        "scene",
        "MetadataTableProperty",
        |_| EXACT,
        |f, a| match f {
            "schema" => ok(json!(schema().classes.keys().collect::<Vec<_>>())),
            "script" => {
                let class_property = class(s(&a[0])).properties.get(s(&a[1])).unwrap().clone();
                let bufs = buffers(&a[4]);
                let views = byte_views(&bufs);
                let mut p = match build(MetadataTableProperty::new(idx(&a[2]), &a[3], class_property, &views)) {
                    Ok(p) => p,
                    Err(out) => return out,
                };
                script(&a[5], |op, x| match op {
                    "get" => p.get(idx(&x[0])).map(|v| tval(v.as_ref())),
                    "set" => p.set(idx(&x[0]), arg(&x[1])).map(|()| Value::Null),
                    "typedArray" => Ok(typed(p.get_typed_array())),
                    "byteLength" => Ok(json!(p.byte_length())),
                    "info" => Ok(json!([
                        p.has_value_transform(),
                        tval(Some(p.offset())),
                        tval(Some(p.scale())),
                        opt(p.extras()),
                        opt(p.extensions()),
                        p.class_property().id()
                    ])),
                    _ => panic!("unknown op {op}"),
                })
            }
            _ => None,
        },
    );
}

#[test]
fn metadata_table() {
    run(
        "scene",
        "MetadataTable",
        |_| EXACT,
        |_, a| {
            let mut t = match build(table_from(&a[0])) {
                Ok(t) => t,
                Err(out) => return out,
            };
            script(&a[1], |op, x| match op {
                "has" => Ok(json!(t.has_property(s(&x[0])))),
                "hasSem" => Ok(json!(t.has_property_by_semantic(s(&x[0])))),
                "ids" => Ok(json!(t.get_property_ids())),
                "get" => t.get_property(idx(&x[0]), s(&x[1])).map(|v| tval(v.as_ref())),
                "set" => t.set_property(idx(&x[0]), s(&x[1]), arg(&x[2])).map(|b| json!(b)),
                "getSem" => t.get_property_by_semantic(idx(&x[0]), s(&x[1])).map(|v| tval(v.as_ref())),
                "setSem" => t.set_property_by_semantic(idx(&x[0]), s(&x[1]), arg(&x[2])).map(|b| json!(b)),
                "typed" => Ok(typed(t.get_property_typed_array(s(&x[0])))),
                "typedSem" => Ok(typed(t.get_property_typed_array_by_semantic(s(&x[0])))),
                "byteLength" => Ok(json!(t.byte_length())),
                "count" => Ok(json!(t.count())),
                "class" => Ok(json!(t.class().id)),
                _ => panic!("unknown op {op}"),
            })
        },
    );
}

#[test]
fn json_metadata_table() {
    run(
        "scene",
        "JsonMetadataTable",
        |_| EXACT,
        |_, a| {
            let mut t = match build(JsonMetadataTable::new(idx(&a[0]), &a[1])) {
                Ok(t) => t,
                Err(out) => return out,
            };
            script(&a[2], |op, x| match op {
                "has" => Ok(json!(t.has_property(s(&x[0])))),
                "ids" => Ok(json!(t.get_property_ids())),
                "get" => t.get_property(idx(&x[0]), s(&x[1])).map(|v| val(v.as_ref())),
                "set" => t.set_property(idx(&x[0]), s(&x[1]), arg(&x[2])).map(|()| Value::Null),
                _ => panic!("unknown op {op}"),
            })
        },
    );
}

fn hierarchy_from(extension: &Value, body: &Value) -> Result<BatchTableHierarchy, MetadataError> {
    let bytes: Vec<u8> =
        if body.is_null() { Vec::new() } else { nums(&body["buffer"]).into_iter().map(|b| b as u8).collect() };
    let view = (!body.is_null()).then(|| ByteView { buffer: &bytes, byte_offset: idx(&body["byteOffset"]) });
    BatchTableHierarchy::new(extension, view)
}

#[test]
fn batch_table_hierarchy() {
    run(
        "scene",
        "BatchTableHierarchy",
        |_| EXACT,
        |_, a| {
            let mut h = match build(hierarchy_from(&a[0], &a[1])) {
                Ok(h) => h,
                Err(out) => return out,
            };
            script(&a[2], |op, x| match op {
                "has" => h.has_property(idx(&x[0]), s(&x[1])).map(|b| json!(b)),
                "exists" => Ok(json!(h.property_exists(s(&x[0])))),
                "ids" => h.get_property_ids(idx(&x[0])).map(|ids| json!(ids)),
                "get" => h.get_property(idx(&x[0]), s(&x[1])).map(|v| val(v.as_ref())),
                "set" => h.set_property(idx(&x[0]), s(&x[1]), arg(&x[2])).map(|b| json!(b)),
                "isClass" => h.is_class(idx(&x[0]), s(&x[1])).map(|b| json!(b)),
                "className" => h.get_class_name(idx(&x[0])).map(opt),
                "byteLength" => Ok(json!(h.byte_length())),
                _ => panic!("unknown op {op}"),
            })
        },
    );
}

#[test]
fn property_table() {
    run(
        "scene",
        "PropertyTable",
        |_| EXACT,
        |_, a| {
            let spec = &a[0];
            let count = idx(&spec["count"]);
            let metadata_table = match spec.get("table") {
                Some(t) => match build(table_from(t)) {
                    Ok(t) => Some(t),
                    Err(out) => return out,
                },
                None => None,
            };
            let json_metadata_table = match spec.get("json") {
                Some(j) => match build(JsonMetadataTable::new(count, j)) {
                    Ok(t) => Some(t),
                    Err(out) => return out,
                },
                None => None,
            };
            let batch_table_hierarchy = match spec.get("hierarchy") {
                Some(h) => match build(hierarchy_from(h, &Value::Null)) {
                    Ok(h) => Some(h),
                    Err(out) => return out,
                },
                None => None,
            };
            let mut t = PropertyTable::new(PropertyTableOptions {
                name: spec.get("name").and_then(Value::as_str).map(str::to_string),
                id: spec.get("id").map(idx),
                count,
                extras: spec.get("extras").cloned(),
                extensions: spec.get("extensions").cloned(),
                metadata_table,
                json_metadata_table,
                batch_table_hierarchy,
            });
            script(&a[1], |op, x| match op {
                "info" => Ok(json!([
                    t.name(),
                    t.id(),
                    t.count(),
                    t.class().map(|c| c.id.clone()),
                    t.properties().map(|p| p.keys().collect::<Vec<_>>()),
                    opt(t.extras()),
                    opt(t.extensions())
                ])),
                "byteLength" => Ok(json!(t.byte_length())),
                "has" => t.has_property(idx(&x[0]), s(&x[1])).map(|b| json!(b)),
                "hasSem" => Ok(json!(t.has_property_by_semantic(idx(&x[0]), s(&x[1])))),
                "exists" => Ok(json!(t.property_exists(s(&x[0])))),
                "existsSem" => Ok(json!(t.property_exists_by_semantic(s(&x[0])))),
                "ids" => t.get_property_ids(idx(&x[0])).map(|ids| json!(ids)),
                "get" => t.get_property(idx(&x[0]), s(&x[1])).map(|v| val(v.as_ref())),
                "set" => t.set_property(idx(&x[0]), s(&x[1]), arg(&x[2])).map(|()| Value::Null),
                "getSem" => t.get_property_by_semantic(idx(&x[0]), s(&x[1])).map(|v| val(v.as_ref())),
                "setSem" => t.set_property_by_semantic(idx(&x[0]), s(&x[1]), arg(&x[2])).map(|b| json!(b)),
                "typed" => Ok(typed(t.get_property_typed_array(s(&x[0])))),
                "typedSem" => Ok(typed(t.get_property_typed_array_by_semantic(s(&x[0])))),
                "isClass" => t.is_class(idx(&x[0]), s(&x[1])).map(|b| json!(b)),
                "isExact" => t.is_exact_class(idx(&x[0]), s(&x[1])).map(|b| json!(b)),
                "exactName" => t.get_exact_class_name(idx(&x[0])).map(opt),
                _ => panic!("unknown op {op}"),
            })
        },
    );
}

fn attribute_info(p: &PropertyAttributeProperty) -> Value {
    json!([
        opt(p.attribute()),
        p.has_value_transform(),
        val(Some(p.offset())),
        val(Some(p.scale())),
        p.class_property().id(),
        opt(p.extras()),
        opt(p.extensions())
    ])
}

#[test]
fn property_attributes() {
    run(
        "scene",
        "PropertyAttributeProperty",
        |_| EXACT,
        |_, a| {
            let class_property = class("t").properties.get(s(&a[0])).unwrap().clone();
            Some(match PropertyAttributeProperty::new(&a[1], class_property) {
                Ok(p) => Out::Ok(attribute_info(&p)),
                Err(e) => Out::Err(e.to_string()),
            })
        },
    );
    run(
        "scene",
        "PropertyAttribute",
        |_| EXACT,
        |_, a| {
            let name = a[1].as_str().map(str::to_string);
            let id = (!a[2].is_null()).then(|| idx(&a[2]));
            Some(match PropertyAttribute::new(&a[0], class("t"), name, id) {
                Ok(pa) => {
                    let properties: serde_json::Map<String, Value> =
                        pa.properties().iter().map(|(k, p)| (k.to_string(), attribute_info(p))).collect();
                    Out::Ok(json!([
                        pa.name(),
                        pa.id(),
                        pa.class().id,
                        properties,
                        opt(pa.extras()),
                        opt(pa.extensions()),
                        pa.get_property("f32").map(|_| "f32"),
                        pa.get_property("nope").map(|_| "nope")
                    ]))
                }
                Err(e) => Out::Err(e.to_string()),
            })
        },
    );
}

#[test]
fn implicit_metadata_view() {
    run(
        "scene",
        "ImplicitMetadataView",
        |_| EXACT,
        |_, a| {
            let table = match build(table_from(&a[0])) {
                Ok(t) => Arc::new(std::sync::Mutex::new(t)),
                Err(out) => return out,
            };
            let mut views = Vec::new();
            for entity_id in 0..4 {
                match build(ImplicitMetadataView::new(table.clone(), class("t"), entity_id, &a[1])) {
                    Ok(v) => views.push(v),
                    Err(out) => return out,
                }
            }
            script(&a[2], |op, x| {
                let v = &mut views[idx(&x[0])];
                match op {
                    "info" => Ok(json!([MetadataEntity::class(v).id, opt(v.extras()), opt(v.extensions())])),
                    "ids" => Ok(json!(v.get_property_ids())),
                    "has" => Ok(json!(v.has_property(s(&x[1])))),
                    "get" => v.get_property(s(&x[1])).map(|v| tval(v.as_ref())),
                    "hasSem" => Ok(json!(v.has_property_by_semantic(s(&x[1])))),
                    "getSem" => v.get_property_by_semantic(s(&x[1])).map(|v| tval(v.as_ref())),
                    "set" => v.set_property(s(&x[1]), arg(&x[2])).map(|b| json!(b)),
                    "setSem" => v.set_property_by_semantic(s(&x[1]), arg(&x[2])).map(|b| json!(b)),
                    _ => panic!("unknown op {op}"),
                }
            })
        },
    );
}
