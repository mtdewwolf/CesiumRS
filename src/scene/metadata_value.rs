//! The dynamic values 3D Metadata passes around: what CesiumJS keeps in
//! plain JavaScript values (numbers, `BigInt`s, booleans, strings, arrays)
//! and in its Cartesian and Matrix classes.
//!
//! Not a Cesium module. It also holds [`JsMap`], an object with JavaScript's
//! key order, and [`MetadataError`].

use crate::core::math::js_number_to_string;
use crate::core::{Cartesian2, Cartesian3, Cartesian4, Matrix2, Matrix3, Matrix4};
use crate::scene::gltf_pipeline::js::array_index;
use serde_json::Value;

/// A metadata value. `undefined` is `Option::None` wherever a value may be
/// missing; `Null` is JSON `null`.
#[derive(Clone, Debug, PartialEq)]
pub enum MetadataValue {
    Null,
    Boolean(bool),
    Number(f64),
    BigInt(i128),
    String(String),
    Array(Vec<MetadataValue>),
    Cartesian2(Cartesian2),
    Cartesian3(Cartesian3),
    Cartesian4(Cartesian4),
    Matrix2(Matrix2),
    Matrix3(Matrix3),
    Matrix4(Matrix4),
    /// A JSON object, which metadata never expects. It compares by identity
    /// in JavaScript, so it never equals anything.
    Object(Value),
}

/// What Cesium throws. `Developer` is a debug-build `DeveloperError`; the
/// others are JavaScript errors invalid input runs into.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MetadataError {
    Developer(String),
    Type(String),
    Range(String),
    Runtime(String),
}

impl std::fmt::Display for MetadataError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let (kind, message) = match self {
            Self::Developer(m) => ("DeveloperError", m),
            Self::Type(m) => ("TypeError", m),
            Self::Range(m) => ("RangeError", m),
            Self::Runtime(m) => ("RuntimeError", m),
        };
        write!(f, "{kind}: {message}")
    }
}

impl std::error::Error for MetadataError {}

pub(crate) fn developer(message: impl Into<String>) -> MetadataError {
    MetadataError::Developer(message.into())
}

/// A number or a `BigInt`, as JavaScript arithmetic sees them.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Numeric {
    Number(f64),
    BigInt(i128),
}

impl Numeric {
    pub fn value(self) -> MetadataValue {
        match self {
            Self::Number(n) => MetadataValue::Number(n),
            Self::BigInt(b) => MetadataValue::BigInt(b),
        }
    }
}

fn mixing() -> MetadataError {
    MetadataError::Type("Cannot mix BigInt and other types, use explicit conversions".into())
}

/// `a op b` for `+`, `-`, `*` and `/`, which throw when a `BigInt` meets a
/// number. `BigInt` division truncates and throws on zero.
pub(crate) fn arithmetic(a: Numeric, b: Numeric, op: char) -> Result<Numeric, MetadataError> {
    Ok(match (a, b) {
        (Numeric::Number(x), Numeric::Number(y)) => Numeric::Number(match op {
            '+' => x + y,
            '-' => x - y,
            '*' => x * y,
            _ => x / y,
        }),
        (Numeric::BigInt(x), Numeric::BigInt(y)) => Numeric::BigInt(match op {
            '+' => x.wrapping_add(y),
            '-' => x.wrapping_sub(y),
            '*' => x.wrapping_mul(y),
            _ => {
                if y == 0 {
                    return Err(MetadataError::Range("Division by zero".into()));
                }
                x / y
            }
        }),
        _ => return Err(mixing()),
    })
}

impl MetadataValue {
    /// A JSON value, unchanged except that numbers become `f64`.
    pub fn from_json(value: &Value) -> Self {
        match value {
            Value::Null => Self::Null,
            Value::Bool(b) => Self::Boolean(*b),
            Value::Number(n) => Self::Number(n.as_f64().unwrap_or(f64::NAN)),
            Value::String(s) => Self::String(s.clone()),
            Value::Array(items) => Self::Array(items.iter().map(Self::from_json).collect()),
            Value::Object(_) => Self::Object(value.clone()),
        }
    }

    /// `from_json` of an optional value, with `null` as `undefined`, as
    /// Cesium's `defined` treats it.
    pub fn from_optional_json(value: Option<&Value>) -> Option<Self> {
        value.filter(|v| !v.is_null()).map(Self::from_json)
    }

    pub fn is_array(&self) -> bool {
        matches!(self, Self::Array(_))
    }

    pub fn as_array(&self) -> Option<&[MetadataValue]> {
        match self {
            Self::Array(items) => Some(items),
            _ => None,
        }
    }

    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::String(s) => Some(s),
            _ => None,
        }
    }

    /// JavaScript `ToNumeric`: numbers and `BigInt`s as they are, the rest
    /// through `ToNumber`.
    pub fn to_numeric(&self) -> Numeric {
        match self {
            Self::BigInt(b) => Numeric::BigInt(*b),
            other => Numeric::Number(other.to_number()),
        }
    }

    /// JavaScript `ToNumber`. A `BigInt` is a `TypeError` there; this gives
    /// its nearest double, for comparisons only.
    pub fn to_number(&self) -> f64 {
        match self {
            Self::Null => 0.0,
            Self::Boolean(b) => f64::from(u8::from(*b)),
            Self::Number(n) => *n,
            Self::BigInt(b) => *b as f64,
            Self::String(s) => {
                let t = s.trim();
                if t.is_empty() {
                    0.0
                } else {
                    t.parse().unwrap_or(f64::NAN)
                }
            }
            Self::Array(items) => match items.as_slice() {
                [] => 0.0,
                [single] => Self::String(single.js_string()).to_number(),
                _ => f64::NAN,
            },
            _ => f64::NAN,
        }
    }

    /// `${value}`: JavaScript's `ToString`, with Cesium's `toString` for the
    /// Cartesian and Matrix classes.
    pub fn js_string(&self) -> String {
        let n = |x: f64| js_number_to_string(x);
        let tuple = |values: &[f64]| format!("({})", values.iter().map(|&v| n(v)).collect::<Vec<_>>().join(", "));
        let matrix = |values: &[f64], size: usize| {
            (0..size)
                .map(|row| tuple(&(0..size).map(|col| values[col * size + row]).collect::<Vec<_>>()))
                .collect::<Vec<_>>()
                .join("\n")
        };
        match self {
            Self::Null => "null".into(),
            Self::Boolean(b) => b.to_string(),
            Self::Number(x) => n(*x),
            Self::BigInt(b) => b.to_string(),
            Self::String(s) => s.clone(),
            Self::Array(items) => items
                .iter()
                .map(|item| if matches!(item, Self::Null) { String::new() } else { item.js_string() })
                .collect::<Vec<_>>()
                .join(","),
            Self::Cartesian2(c) => tuple(&[c.x, c.y]),
            Self::Cartesian3(c) => tuple(&[c.x, c.y, c.z]),
            Self::Cartesian4(c) => tuple(&[c.x, c.y, c.z, c.w]),
            Self::Matrix2(m) => matrix(&m.0, 2),
            Self::Matrix3(m) => matrix(&m.0, 3),
            Self::Matrix4(m) => matrix(&m.0, 4),
            Self::Object(_) => "[object Object]".into(),
        }
    }

    /// `left === right` for primitives. Objects, arrays and the math types
    /// compare by identity in JavaScript, so a copy is never equal.
    pub fn strict_equals(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Null, Self::Null) => true,
            (Self::Boolean(a), Self::Boolean(b)) => a == b,
            (Self::Number(a), Self::Number(b)) => a == b,
            (Self::BigInt(a), Self::BigInt(b)) => a == b,
            (Self::String(a), Self::String(b)) => a == b,
            _ => false,
        }
    }
}

/// `value < bound` and `value > bound` for a number or `BigInt` against a
/// number or `BigInt`, compared exactly as JavaScript compares them.
pub(crate) fn compare(value: Numeric, bound: Numeric) -> Option<std::cmp::Ordering> {
    use std::cmp::Ordering;
    match (value, bound) {
        (Numeric::Number(a), Numeric::Number(b)) => a.partial_cmp(&b),
        (Numeric::BigInt(a), Numeric::BigInt(b)) => Some(a.cmp(&b)),
        (Numeric::Number(a), Numeric::BigInt(b)) => {
            compare(Numeric::BigInt(b), Numeric::Number(a)).map(Ordering::reverse)
        }
        (Numeric::BigInt(a), Numeric::Number(b)) => {
            if b.is_nan() {
                return None;
            }
            if b >= 2f64.powi(127) {
                return Some(Ordering::Less);
            }
            if b < -(2f64.powi(127)) {
                return Some(Ordering::Greater);
            }
            let floor = b.floor();
            match a.cmp(&(floor as i128)) {
                Ordering::Equal if b > floor => Some(Ordering::Less),
                ordering => Some(ordering),
            }
        }
    }
}

/// A JavaScript object's own properties in its key order: array-index keys
/// first in ascending order, then the rest in insertion order.
#[derive(Clone, Debug, PartialEq)]
pub struct JsMap<T> {
    entries: Vec<(String, T)>,
}

impl<T> Default for JsMap<T> {
    fn default() -> Self {
        Self { entries: Vec::new() }
    }
}

impl<T> JsMap<T> {
    pub fn new() -> Self {
        Self::default()
    }

    fn position(&self, key: &str) -> Option<usize> {
        self.entries.iter().position(|(k, _)| k == key)
    }

    pub fn get(&self, key: &str) -> Option<&T> {
        self.position(key).map(|i| &self.entries[i].1)
    }

    pub fn get_mut(&mut self, key: &str) -> Option<&mut T> {
        self.position(key).map(move |i| &mut self.entries[i].1)
    }

    pub fn contains_key(&self, key: &str) -> bool {
        self.position(key).is_some()
    }

    /// `object[key] = value`: an existing key keeps its place.
    pub fn insert(&mut self, key: impl Into<String>, value: T) {
        let key = key.into();
        match self.position(&key) {
            Some(i) => self.entries[i].1 = value,
            None => {
                let at = match array_index(&key) {
                    // Index keys sort before the others, in numeric order.
                    Some(index) => self
                        .entries
                        .iter()
                        .position(|(k, _)| array_index(k).is_none_or(|other| other > index))
                        .unwrap_or(self.entries.len()),
                    None => self.entries.len(),
                };
                self.entries.insert(at, (key, value));
            }
        }
    }

    pub fn remove(&mut self, key: &str) -> Option<T> {
        self.position(key).map(|i| self.entries.remove(i).1)
    }

    pub fn keys(&self) -> impl Iterator<Item = &str> {
        self.entries.iter().map(|(k, _)| k.as_str())
    }

    pub fn values(&self) -> impl Iterator<Item = &T> {
        self.entries.iter().map(|(_, v)| v)
    }

    pub fn iter(&self) -> impl Iterator<Item = (&str, &T)> {
        self.entries.iter().map(|(k, v)| (k.as_str(), v))
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

impl<T> FromIterator<(String, T)> for JsMap<T> {
    fn from_iter<I: IntoIterator<Item = (String, T)>>(iter: I) -> Self {
        let mut map = Self::new();
        for (k, v) in iter {
            map.insert(k, v);
        }
        map
    }
}

/// The entries of a JSON object in `for...in` order; an array's are its
/// indices.
pub(crate) fn json_entries(value: Option<&Value>) -> Vec<(String, &Value)> {
    match value {
        Some(Value::Array(items)) => items.iter().enumerate().map(|(i, v)| (i.to_string(), v)).collect(),
        Some(Value::Object(map)) => crate::scene::gltf_pipeline::js::object_keys(map)
            .into_iter()
            .map(|k| {
                let v = &map[&k];
                (k, v)
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// `defined(value)` for an optional JSON value.
pub(crate) fn json_defined(value: Option<&Value>) -> bool {
    value.is_some_and(|v| !v.is_null())
}

/// An optional string property, as Cesium copies it.
pub(crate) fn json_string(value: Option<&Value>) -> Option<String> {
    value.and_then(Value::as_str).map(str::to_string)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cmp::Ordering;

    #[test]
    fn js_map_orders_index_keys_first() {
        let map: JsMap<u8> =
            [("b", 1), ("2", 2), ("a", 3), ("1", 4), ("01", 5)].into_iter().map(|(k, v)| (k.to_string(), v)).collect();
        assert_eq!(map.keys().collect::<Vec<_>>(), ["1", "2", "b", "a", "01"]);
    }

    #[test]
    fn bigint_number_comparisons_are_exact() {
        let big = Numeric::BigInt(9007199254740993);
        assert_eq!(compare(big, Numeric::Number(9007199254740992.0)), Some(Ordering::Greater));
        assert_eq!(compare(Numeric::BigInt(1), Numeric::Number(1.5)), Some(Ordering::Less));
        assert_eq!(compare(Numeric::Number(-1.5), Numeric::BigInt(-1)), Some(Ordering::Less));
        assert_eq!(compare(Numeric::BigInt(1), Numeric::Number(f64::NAN)), None);
    }

    #[test]
    fn strings_follow_javascript() {
        let v = MetadataValue::Array(vec![MetadataValue::Number(1.0), MetadataValue::Null, MetadataValue::BigInt(-3)]);
        assert_eq!(v.js_string(), "1,,-3");
        assert_eq!(MetadataValue::Matrix2(Matrix2([1.0, 2.0, 3.0, 4.0])).js_string(), "(1, 3)\n(2, 4)");
    }
}
