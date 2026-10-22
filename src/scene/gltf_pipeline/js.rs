//! The JavaScript object semantics the glTF pipeline relies on, over
//! `serde_json` values: property keys, `defined`, `for...in` order, `ToNumber`
//! and strict equality.
//!
//! JSON has no `undefined`. A missing property is `None`, and a property
//! Cesium sets to `undefined` is left unset, which is what `JSON.stringify`
//! writes for it.

use serde_json::{Map, Value};

use super::PipelineError;
use crate::core::math::js_number_to_string;

/// Cesium's `defined`: present and not `null`.
pub fn defined(value: Option<&Value>) -> bool {
    value.is_some_and(|v| !v.is_null())
}

/// `ToPropertyKey` of a value used as an index: numbers print as JavaScript
/// does, so `1` and `"1"` name the same property.
pub fn key(value: &Value) -> String {
    match value {
        Value::String(s) => s.clone(),
        Value::Number(n) => js_number_to_string(n.as_f64().unwrap_or(f64::NAN)),
        Value::Bool(b) => b.to_string(),
        Value::Null => "null".into(),
        Value::Array(items) => {
            items.iter().map(|v| if v.is_null() { String::new() } else { key(v) }).collect::<Vec<_>>().join(",")
        }
        Value::Object(_) => "[object Object]".into(),
    }
}

/// An array index in the `OrdinaryOwnPropertyKeys` sense: a canonical
/// base-10 string below `2^32 - 1`.
pub fn array_index(key: &str) -> Option<usize> {
    let bytes = key.as_bytes();
    if bytes.is_empty() || bytes.len() > 10 || (bytes.len() > 1 && bytes[0] == b'0') {
        return None;
    }
    if !bytes.iter().all(u8::is_ascii_digit) {
        return None;
    }
    let index = key.parse::<u64>().ok()?;
    (index < u32::MAX as u64).then_some(index as usize)
}

/// `container[key]`, or `None` when the container or the property is missing.
pub fn get<'a>(container: Option<&'a Value>, key: &str) -> Option<&'a Value> {
    match container? {
        Value::Object(map) => map.get(key),
        Value::Array(items) => array_index(key).and_then(|i| items.get(i)),
        _ => None,
    }
}

pub fn get_mut<'a>(container: Option<&'a mut Value>, key: &str) -> Option<&'a mut Value> {
    match container? {
        Value::Object(map) => map.get_mut(key),
        Value::Array(items) => array_index(key).and_then(move |i| items.get_mut(i)),
        _ => None,
    }
}

/// `container[key]` where `key` is a JavaScript value.
pub fn index<'a>(container: Option<&'a Value>, index: &Value) -> Option<&'a Value> {
    get(container, &key(index))
}

pub fn index_mut<'a>(container: Option<&'a mut Value>, index: &Value) -> Option<&'a mut Value> {
    get_mut(container, &key(index))
}

fn type_error(what: &str) -> PipelineError {
    PipelineError::Type(format!("Cannot read properties of undefined (reading '{what}')"))
}

/// A value whose properties are about to be read. Reading a property of
/// `undefined` or `null` throws a `TypeError` in JavaScript.
pub fn object<'a>(value: Option<&'a Value>, reading: &str) -> Result<&'a Value, PipelineError> {
    value.filter(|v| !v.is_null()).ok_or_else(|| type_error(reading))
}

pub fn object_mut<'a>(value: Option<&'a mut Value>, reading: &str) -> Result<&'a mut Value, PipelineError> {
    value.filter(|v| !v.is_null()).ok_or_else(|| type_error(reading))
}

/// `container[key] = value`. Arrays grow as JavaScript arrays do, with the
/// holes written as `null`, as `JSON.stringify` writes them. Assigning a
/// property of a primitive throws in strict-mode code.
pub fn set(container: &mut Value, key: &str, value: Value) -> Result<(), PipelineError> {
    match container {
        Value::Object(map) => {
            map.insert(key.to_string(), value);
            Ok(())
        }
        Value::Array(items) => match array_index(key) {
            Some(i) => {
                if i >= items.len() {
                    items.resize(i + 1, Value::Null);
                }
                items[i] = value;
                Ok(())
            }
            // Non-index properties of arrays are not JSON; JSON.stringify drops them.
            None => Ok(()),
        },
        _ => Err(PipelineError::Type(format!("Cannot create property '{key}' on a primitive"))),
    }
}

/// `delete container[key]`.
pub fn delete(container: &mut Value, key: &str) {
    if let Value::Object(map) = container {
        map.shift_remove(key);
    }
}

/// `for (const key in value)`: an array's indices, or an object's keys with
/// array indices first in ascending order and the rest in insertion order.
pub fn for_in(value: Option<&Value>) -> Vec<String> {
    match value {
        Some(Value::Array(items)) => (0..items.len()).map(|i| i.to_string()).collect(),
        Some(Value::Object(map)) => object_keys(map),
        _ => Vec::new(),
    }
}

/// `Object.keys` order for a JSON object.
pub fn object_keys(map: &Map<String, Value>) -> Vec<String> {
    let mut indices: Vec<(usize, &String)> = Vec::new();
    let mut rest = Vec::new();
    for k in map.keys() {
        match array_index(k) {
            Some(i) => indices.push((i, k)),
            None => rest.push(k.clone()),
        }
    }
    indices.sort_by_key(|(i, _)| *i);
    indices.into_iter().map(|(_, k)| k.clone()).chain(rest).collect()
}

/// `Object.keys(value).length`, with non-objects counting as empty.
pub fn key_count(value: Option<&Value>) -> usize {
    match value {
        Some(Value::Object(map)) => map.len(),
        Some(Value::Array(items)) => items.len(),
        _ => 0,
    }
}

/// JavaScript `ToNumber`.
pub fn to_number(value: Option<&Value>) -> f64 {
    match value {
        None => f64::NAN,
        Some(Value::Null) => 0.0,
        Some(Value::Bool(b)) => f64::from(u8::from(*b)),
        Some(Value::Number(n)) => n.as_f64().unwrap_or(f64::NAN),
        Some(Value::String(s)) => string_to_number(s),
        Some(v @ Value::Array(_)) => string_to_number(&key(v)),
        Some(Value::Object(_)) => f64::NAN,
    }
}

fn string_to_number(s: &str) -> f64 {
    let t = s.trim_matches(|c: char| c.is_whitespace() || c == '\u{feff}');
    if t.is_empty() {
        return 0.0;
    }
    let radix = |p: &str, r: u32| t.strip_prefix(p).map(|d| u64::from_str_radix(d, r).map_or(f64::NAN, |v| v as f64));
    if let Some(v) = radix("0x", 16).or_else(|| radix("0X", 16)).or_else(|| radix("0o", 8)).or_else(|| radix("0b", 2)) {
        return v;
    }
    match t {
        "Infinity" | "+Infinity" => f64::INFINITY,
        "-Infinity" => f64::NEG_INFINITY,
        _ if t.chars().all(|c| c.is_ascii_digit() || matches!(c, '.' | 'e' | 'E' | '+' | '-')) => {
            t.parse().unwrap_or(f64::NAN)
        }
        _ => f64::NAN,
    }
}

/// A JSON number, written as an integer when it is one, as `JSON.parse`
/// would have read it.
pub fn number(value: f64) -> Value {
    // JSON.stringify writes -0 as 0.
    if value.is_finite() && value.fract() == 0.0 && value.abs() < 9.007_199_254_740_992e15 {
        return Value::from(value as i64);
    }
    // NaN and infinities have no JSON form; JSON.stringify writes null.
    serde_json::Number::from_f64(value).map_or(Value::Null, Value::Number)
}

/// `a === b` for the primitives the pipeline compares. Objects and arrays
/// are compared by identity in JavaScript and never equal a copy.
pub fn strict_eq(a: Option<&Value>, b: Option<&Value>) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(Value::Number(x)), Some(Value::Number(y))) => x.as_f64() == y.as_f64(),
        (Some(Value::String(x)), Some(Value::String(y))) => x == y,
        (Some(Value::Bool(x)), Some(Value::Bool(y))) => x == y,
        (Some(Value::Null), Some(Value::Null)) => true,
        _ => false,
    }
}

/// `array.indexOf(value)` with strict equality.
pub fn index_of(array: &[Value], value: Option<&Value>) -> Option<usize> {
    array.iter().position(|item| strict_eq(Some(item), value))
}

/// JavaScript `ToIndex`, as `DataView` reads apply it to offsets.
pub fn to_index(value: f64) -> Result<usize, PipelineError> {
    let integer = if value.is_nan() { 0.0 } else { value.trunc() };
    if !(0.0..=9_007_199_254_740_991.0).contains(&integer) {
        return Err(PipelineError::Range("Offset is outside the bounds of the DataView".into()));
    }
    Ok(integer as usize)
}

/// `object[key] = object[key] ?? default`. The assignment happens even when
/// the value is defined, so a primitive `object` throws.
pub fn assign_default(object: &mut Value, key: &str, default: Value) -> Result<(), PipelineError> {
    if !matches!(object, Value::Object(_) | Value::Array(_)) {
        return set(object, key, default);
    }
    if !defined(get(Some(object), key)) {
        set(object, key, default)?;
    }
    Ok(())
}
