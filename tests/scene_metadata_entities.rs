//! 3D Metadata entities (tileset, tile, content, group and implicit subtree
//! metadata), `Cesium3DTilesetMetadata` and the `find*Metadata` helpers
//! replayed against the pinned CesiumJS fixtures, on the schema of the
//! `MetadataSchema` fixtures.
//!
//! Entity cases run a script of operations, each recorded as
//! `{ "ok": value }` or `{ "throws": errorName }`.
mod common;
use cesium_rs::core::{Cartesian2, Cartesian3, Cartesian4, Matrix2, Matrix3, Matrix4};
use cesium_rs::scene::*;
use common::*;
use serde_json::{json, Value};
use std::sync::{Arc, OnceLock};

/// A value as the fixtures tag it.
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

fn opt(v: Option<&Value>) -> Value {
    v.cloned().unwrap_or(Value::Null)
}

fn script(ops: &Value, mut op: impl FnMut(&str, &[Value]) -> Result<Value, MetadataError>) -> Option<Out> {
    let results = ops.as_array().unwrap().iter().map(|o| {
        let o = o.as_array().unwrap();
        attempt(op(s(&o[0]), &o[1..]))
    });
    Some(Out::Ok(Value::Array(results.collect())))
}

fn schema() -> Arc<MetadataSchema> {
    static SCHEMA: OnceLock<Arc<MetadataSchema>> = OnceLock::new();
    SCHEMA
        .get_or_init(|| Arc::new(MetadataSchema::from_json(&load("scene", "MetadataSchema")[0].a[0]).unwrap()))
        .clone()
}

fn class(id: &str) -> Arc<MetadataClass> {
    schema().classes.get(id).unwrap().clone()
}

/// An entity of the module, with its `[class, extras, extensions, id]`.
fn entity(
    module: &str,
    json: &Value,
    class: Arc<MetadataClass>,
) -> Result<(Box<dyn MetadataEntity>, Value), MetadataError> {
    let info = |id: &str, extras: Option<&Value>, extensions: Option<&Value>, group: Option<&str>| {
        json!([id, opt(extras), opt(extensions), group])
    };
    let class_id = class.id.clone();
    Ok(match module {
        "GroupMetadata" => {
            let e = GroupMetadata::new(json, class, Some("g".into()))?;
            let i = info(&class_id, e.extras(), e.extensions(), e.id());
            (Box::new(e), i)
        }
        "TilesetMetadata" => {
            let e = TilesetMetadata::new(json, class)?;
            let i = info(&class_id, e.extras(), e.extensions(), None);
            (Box::new(e), i)
        }
        "TileMetadata" => {
            let e = TileMetadata::new(json, class)?;
            let i = info(&class_id, e.extras(), e.extensions(), None);
            (Box::new(e), i)
        }
        "ContentMetadata" => {
            let e = ContentMetadata::new(json, class)?;
            let i = info(&class_id, e.extras(), e.extensions(), None);
            (Box::new(e), i)
        }
        "ImplicitSubtreeMetadata" => {
            let e = ImplicitSubtreeMetadata::new(json, class)?;
            let i = info(&class_id, e.extras(), e.extensions(), None);
            (Box::new(e), i)
        }
        _ => unreachable!(),
    })
}

fn entity_port(module: &'static str) -> impl Fn(&str, &[Value]) -> Option<Out> {
    move |f, a| {
        let (mut e, info) = match entity(module, &a[1], class(s(&a[0]))) {
            Ok(e) => e,
            Err(err) => return Some(Out::Err(err.to_string())),
        };
        match f {
            "script" => script(&a[2], |op, x| match op {
                "info" => Ok(info.clone()),
                "ids" => Ok(json!(e.get_property_ids())),
                "has" => Ok(json!(e.has_property(s(&x[0])))),
                "get" => e.get_property(s(&x[0])).map(|v| val(v.as_ref())),
                "hasSem" => Ok(json!(e.has_property_by_semantic(s(&x[0])))),
                "getSem" => e.get_property_by_semantic(s(&x[0])).map(|v| val(v.as_ref())),
                "set" => e.set_property(s(&x[0]), arg(&x[1])).map(|b| json!(b)),
                "setSem" => e.set_property_by_semantic(s(&x[0]), arg(&x[1])).map(|b| json!(b)),
                _ => panic!("unknown op {op}"),
            }),
            "noProperties" => Some(match e.get_property("count") {
                Ok(v) => Out::Ok(json!([e.get_property_ids(), e.has_property("count"), val(v.as_ref())])),
                Err(err) => Out::Err(err.to_string()),
            }),
            _ => None,
        }
    }
}

#[test]
fn entities() {
    run("scene", "GroupMetadata", |_| EXACT, entity_port("GroupMetadata"));
    run("scene", "TilesetMetadata", |_| EXACT, entity_port("TilesetMetadata"));
    run("scene", "TileMetadata", |_| EXACT, entity_port("TileMetadata"));
    run("scene", "ContentMetadata", |_| EXACT, entity_port("ContentMetadata"));
    run("scene", "ImplicitSubtreeMetadata", |_| EXACT, entity_port("ImplicitSubtreeMetadata"));
}

fn describe(e: Option<&dyn DescribeEntity>) -> Value {
    e.map_or(Value::Null, |e| e.describe())
}

/// `[class, propertyIds, extras, extensions]`.
trait DescribeEntity {
    fn describe(&self) -> Value;
}

macro_rules! describe_entity {
    ($($t:ty),*) => {$(
        impl DescribeEntity for $t {
            fn describe(&self) -> Value {
                json!([MetadataEntity::class(self).id, self.get_property_ids(), opt(self.extras()), opt(self.extensions())])
            }
        }
    )*};
}
describe_entity!(TilesetMetadata, GroupMetadata, TileMetadata, ContentMetadata);

fn group_info(g: &GroupMetadata) -> Value {
    let mut out = vec![json!(g.id())];
    out.extend(g.describe().as_array().unwrap().iter().cloned());
    Value::Array(out)
}

#[test]
fn tileset_metadata_and_finders() {
    run(
        "scene",
        "Cesium3DTilesetMetadata",
        |_| EXACT,
        |_, a| {
            Some(match Cesium3DTilesetMetadata::new(&a[0], schema()) {
                Ok(m) => Out::Ok(json!([
                    describe(m.tileset().map(|t| t as &dyn DescribeEntity)),
                    m.groups().iter().map(group_info).collect::<Vec<_>>(),
                    m.group_ids(),
                    opt(m.statistics()),
                    opt(m.extras()),
                    opt(m.extensions()),
                    m.schema().id
                ])),
                Err(e) => Out::Err(e.to_string()),
            })
        },
    );
    run(
        "scene",
        "findGroupMetadata",
        |_| EXACT,
        |_, a| {
            let metadata =
                if a[0].is_null() { None } else { Some(Cesium3DTilesetMetadata::new(&a[0], schema()).unwrap()) };
            let g = find_group_metadata(metadata.as_ref(), &a[1]);
            ok(g.map_or(Value::Null, |g| json!([g.id(), MetadataEntity::class(g).id, g.get_property_ids()])))
        },
    );
    let finder = |tile: bool| {
        move |_: &str, a: &[Value]| {
            let schema = schema();
            let schema = a[0].as_bool().unwrap().then_some(schema.as_ref());
            let found = if tile {
                find_tile_metadata(schema, &a[1]).map(|e| describe(e.as_ref().map(|e| e as &dyn DescribeEntity)))
            } else {
                find_content_metadata(schema, &a[1]).map(|e| describe(e.as_ref().map(|e| e as &dyn DescribeEntity)))
            };
            Some(match found {
                Ok(v) => Out::Ok(v),
                Err(e) => Out::Err(e.to_string()),
            })
        }
    };
    run("scene", "findTileMetadata", |_| EXACT, finder(true));
    run("scene", "findContentMetadata", |_| EXACT, finder(false));
}
