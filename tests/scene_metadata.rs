//! 3D Metadata schemas, classes, properties, enums and the MetadataEntity
//! helpers replayed against the pinned CesiumJS fixtures.
//!
//! Values are JSON with tags: `{ "$bigint": "…" }`, `{ "$Cartesian3": […] }`
//! (and the other Cartesian and Matrix classes) and `{ "$null": true }`;
//! `undefined` is null.
mod common;
use cesium_rs::core::{Cartesian2, Cartesian3, Cartesian4, Matrix2, Matrix3, Matrix4};
use cesium_rs::scene::metadata_entity::{self, properties_from_json};
use cesium_rs::scene::*;
use common::*;
use serde_json::{json, Map, Value};
use std::sync::OnceLock;

fn val(value: &MetadataValue) -> Value {
    match value {
        MetadataValue::Null => json!({ "$null": true }),
        MetadataValue::Boolean(b) => json!(b),
        MetadataValue::Number(x) => num(*x),
        MetadataValue::BigInt(b) => json!({ "$bigint": b.to_string() }),
        MetadataValue::String(s) => json!(s),
        MetadataValue::Array(items) => Value::Array(items.iter().map(val).collect()),
        MetadataValue::Cartesian2(c) => json!({ "$Cartesian2": [num(c.x), num(c.y)] }),
        MetadataValue::Cartesian3(c) => json!({ "$Cartesian3": [num(c.x), num(c.y), num(c.z)] }),
        MetadataValue::Cartesian4(c) => json!({ "$Cartesian4": [num(c.x), num(c.y), num(c.z), num(c.w)] }),
        MetadataValue::Matrix2(m) => json!({ "$Matrix2": m.0.iter().map(|&x| num(x)).collect::<Vec<_>>() }),
        MetadataValue::Matrix3(m) => json!({ "$Matrix3": m.0.iter().map(|&x| num(x)).collect::<Vec<_>>() }),
        MetadataValue::Matrix4(m) => json!({ "$Matrix4": m.0.iter().map(|&x| num(x)).collect::<Vec<_>>() }),
        MetadataValue::Object(v) => v.clone(),
    }
}

fn opt_val(value: Option<&MetadataValue>) -> Value {
    value.map_or(Value::Null, val)
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

fn result<T>(r: Result<T, MetadataError>, f: impl FnOnce(T) -> Value) -> Option<Out> {
    Some(match r {
        Ok(v) => Out::Ok(f(v)),
        Err(e) => Out::Err(e.to_string()),
    })
}

/// The schema every fixture case uses: the first `MetadataSchema` case's input.
fn schema() -> &'static MetadataSchema {
    static SCHEMA: OnceLock<MetadataSchema> = OnceLock::new();
    SCHEMA.get_or_init(|| MetadataSchema::from_json(&load("scene", "MetadataSchema")[0].a[0]).unwrap())
}

fn opt_str(v: Option<&str>) -> Value {
    v.map_or(Value::Null, |s| json!(s))
}

fn property_info(p: &MetadataClassProperty) -> Value {
    json!({
        "id": p.id(), "name": opt_str(p.name()), "description": opt_str(p.description()), "type": p.metadata_type().as_str(),
        "enumType": p.enum_type().map_or(Value::Null, |e| json!(e.id)),
        "componentType": opt_str(p.component_type().map(MetadataComponentType::as_str)),
        "valueType": opt_str(p.value_type().map(MetadataComponentType::as_str)),
        "isArray": p.is_array(), "isVariableLengthArray": p.is_variable_length_array(),
        "arrayLength": p.array_length().cloned().unwrap_or(Value::Null), "normalized": p.normalized(),
        "max": opt_val(p.max()), "min": opt_val(p.min()), "noData": opt_val(p.no_data()), "default": opt_val(p.default()),
        "required": p.required(), "semantic": opt_str(p.semantic()), "hasValueTransform": p.has_value_transform(),
        "offset": val(p.offset()), "scale": val(p.scale()),
        "extras": p.extras().cloned().unwrap_or(Value::Null), "extensions": p.extensions().cloned().unwrap_or(Value::Null),
        "isLegacyExtension": p.is_legacy_extension().map_or(Value::Null, |b| json!(b)),
    })
}

#[test]
fn enums_classes_and_schemas() {
    run(
        "scene",
        "MetadataEnumValue",
        |_| EXACT,
        |_, a| {
            result(MetadataEnumValue::from_json(&a[0]), |v| {
                json!([
                    v.value,
                    v.name,
                    v.description,
                    v.extras.unwrap_or(Value::Null),
                    v.extensions.unwrap_or(Value::Null)
                ])
            })
        },
    );
    run(
        "scene",
        "MetadataEnum",
        |_| EXACT,
        |_, a| {
            result(MetadataEnum::from_json(s(&a[0]), &a[1]), |e| {
                let names: Map<String, Value> =
                    e.names_by_value.iter().map(|(k, v)| (k.to_string(), json!(v))).collect();
                let values: Map<String, Value> =
                    e.values_by_name.iter().map(|(k, v)| (k.to_string(), num(*v))).collect();
                json!({
                    "id": e.id, "values": e.values.iter().map(|v| json!([v.name, v.value])).collect::<Vec<_>>(),
                    "namesByValue": names, "valuesByName": values, "valueType": e.value_type.as_str(),
                    "name": e.name, "description": e.description, "extras": e.extras.unwrap_or(Value::Null),
                })
            })
        },
    );
    run(
        "scene",
        "MetadataClass",
        |_| EXACT,
        |f, a| match f {
            "BATCH_TABLE_CLASS_NAME" => ok(MetadataClass::BATCH_TABLE_CLASS_NAME.to_string()),
            _ => {
                let c = schema().classes.get(s(&a[0])).unwrap();
                let by_semantic: Map<String, Value> =
                    c.properties_by_semantic.iter().map(|(k, p)| (k.to_string(), json!(p.id()))).collect();
                ok(json!({
                    "id": c.id, "name": c.name, "description": c.description, "properties": c.properties.keys().collect::<Vec<_>>(),
                    "propertiesBySemantic": by_semantic, "extras": c.extras.clone().unwrap_or(Value::Null),
                    "extensions": c.extensions.clone().unwrap_or(Value::Null),
                }))
            }
        },
    );
    run(
        "scene",
        "MetadataSchema",
        |_| EXACT,
        |_, a| {
            result(MetadataSchema::from_json(&a[0]), |m| {
                json!({
                    "id": m.id, "name": m.name, "description": m.description, "version": m.version,
                    "classes": m.classes.keys().collect::<Vec<_>>(), "enums": m.enums.keys().collect::<Vec<_>>(),
                    "extras": m.extras.unwrap_or(Value::Null), "extensions": m.extensions.unwrap_or(Value::Null),
                })
            })
        },
    );
    run(
        "scene",
        "getMetadataClassProperty",
        |_| EXACT,
        |_, a| {
            if a[0] == json!("noSchema") {
                return ok(get_metadata_class_property(None, None, "tree", "height").map(|p| p.id().to_string()));
            }
            let property = get_metadata_class_property(Some(schema()), a[0].as_str(), s(&a[1]), s(&a[2]));
            ok(property.map(|p| p.id().to_string()))
        },
    );
}

#[test]
fn class_property() {
    run(
        "scene",
        "MetadataClassProperty",
        |_| EXACT,
        |f, a| {
            if f == "fromJson" {
                return result(MetadataClassProperty::from_json("p", &a[0], &schema().enums), |p| property_info(&p));
            }
            let p = schema().classes.get(s(&a[0])).unwrap().properties.get(s(&a[1])).unwrap().clone();
            let value = || unval(&a[2]);
            let defined = || value().expect("value methods get defined values");
            match f {
                "info" => ok(property_info(&p)),
                "expandConstant" => result(p.expand_constant(3.0, a[2].as_bool().unwrap()), |v| val(&v)),
                "bytesPerElement" => result(p.bytes_per_element(), num),
                "isGpuCompatible" => result(p.is_gpu_compatible(n(&a[2])), |b| json!(b)),
                "getGlslType" => result(p.get_glsl_type(), |t| opt_str(t)),
                "getGlslTypeWebGL1" => ok(p.get_glsl_type_webgl1().map(str::to_string)),
                "unpackTextureInShader" => {
                    let mut lines = Vec::new();
                    let name = p.unpack_texture_in_shader("texture(u_tex, v_uv)", s(&a[2]), "metadata_x", &mut lines);
                    result(name, |name| json!([name, lines]))
                }
                "unpackTextureInShaderWebGL1" => ok(p.unpack_texture_in_shader_webgl1("texture(u_tex, v_uv).r")),
                "validate" => ok(p.validate(value().as_ref())),
                "handleNoData" => ok(opt_val(p.handle_no_data(value()).as_ref())),
                "normalize" => result(p.normalize(defined()), |v| val(&v)),
                "unnormalize" => result(p.unnormalize(defined()), |v| val(&v)),
                "applyValueTransform" => result(p.apply_value_transform(defined()), |v| val(&v)),
                "unapplyValueTransform" => result(p.unapply_value_transform(defined()), |v| val(&v)),
                "unpackVectorAndMatrixTypes" => {
                    result(p.unpack_vector_and_matrix_types(defined(), a[3].as_bool().unwrap()), |v| val(&v))
                }
                "packVectorAndMatrixTypes" => {
                    result(p.pack_vector_and_matrix_types(defined(), a[3].as_bool().unwrap()), |v| val(&v))
                }
                _ => None,
            }
        },
    );
}

fn properties_json(properties: &MetadataProperties) -> Value {
    Value::Object(properties.iter().map(|(k, v)| (k.to_string(), val(v))).collect())
}

#[test]
fn entity_helpers() {
    run(
        "scene",
        "MetadataEntity",
        |_| EXACT,
        |f, a| {
            let class = schema().classes.get(s(&a[0])).unwrap();
            let mut properties = properties_from_json(Some(&a[1]));
            match f {
                "getPropertyIds" => ok(json!(metadata_entity::get_property_ids(&properties, class))),
                "hasProperty" => ok(metadata_entity::has_property(s(&a[2]), &properties, class)),
                "getProperty" => {
                    result(metadata_entity::get_property(s(&a[2]), &properties, class), |v| opt_val(v.as_ref()))
                }
                "hasPropertyBySemantic" => ok(metadata_entity::has_property_by_semantic(s(&a[2]), class)),
                "getPropertyBySemantic" => {
                    result(metadata_entity::get_property_by_semantic(s(&a[2]), &properties, class), |v| {
                        opt_val(v.as_ref())
                    })
                }
                "setProperty" | "setPropertyBySemantic" => {
                    let value = unval(&a[3]).unwrap();
                    let set = if f == "setProperty" {
                        metadata_entity::set_property(s(&a[2]), value, &mut properties, class)
                    } else {
                        metadata_entity::set_property_by_semantic(s(&a[2]), value, &mut properties, class)
                    };
                    result(set, |b| json!([b, properties_json(&properties)]))
                }
                _ => None,
            }
        },
    );
}
