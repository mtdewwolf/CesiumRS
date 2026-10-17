//! Loads CesiumJS fixtures (`tools/cesium/fixtures`) and compares them with
//! the Rust port.
#![allow(dead_code)]

use cesium_rs::core::*;
use serde_json::{json, Value};
use std::collections::BTreeSet;
use std::path::Path;

pub struct Case {
    pub f: String,
    pub a: Vec<Value>,
    pub r: Value,
}

pub fn load(package: &str, module: &str) -> Vec<Case> {
    let path = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(package)
        .join(format!("{module}.json"));
    let text = std::fs::read_to_string(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let doc: Value = serde_json::from_str(&text).expect("fixture JSON");
    doc["cases"]
        .as_array()
        .expect("cases")
        .iter()
        .map(|c| Case {
            f: c["f"].as_str().expect("f").to_string(),
            a: c["a"].as_array().expect("a").clone(),
            r: c["r"].clone(),
        })
        .collect()
}

// ---- decoding fixture values -------------------------------------------

pub fn n(v: &Value) -> f64 {
    match v {
        Value::Number(x) => x.as_f64().unwrap(),
        Value::String(s) if s == "NaN" => f64::NAN,
        Value::String(s) if s == "Infinity" => f64::INFINITY,
        Value::String(s) if s == "-Infinity" => f64::NEG_INFINITY,
        Value::String(s) if s == "-0" => -0.0,
        Value::Null => f64::NAN,
        other => panic!("not a number: {other}"),
    }
}

pub fn opt(v: &Value) -> Option<f64> {
    if v.is_null() {
        None
    } else {
        Some(n(v))
    }
}

pub fn u(v: &Value) -> u32 {
    n(v) as u32
}

pub fn s(v: &Value) -> &str {
    v.as_str().expect("string")
}

pub fn nums(v: &Value) -> Vec<f64> {
    v.as_array().expect("array").iter().map(n).collect()
}

pub fn list<T>(v: &Value, f: impl Fn(&Value) -> T) -> Vec<T> {
    v.as_array().expect("array").iter().map(f).collect()
}

fn fixed<const K: usize>(v: &Value) -> [f64; K] {
    let a = nums(v);
    assert_eq!(a.len(), K, "expected {K} numbers, got {v}");
    a.try_into().unwrap()
}

pub fn c2(v: &Value) -> Cartesian2 {
    fixed::<2>(v).into()
}
pub fn c3(v: &Value) -> Cartesian3 {
    fixed::<3>(v).into()
}
pub fn c4(v: &Value) -> Cartesian4 {
    fixed::<4>(v).into()
}
pub fn q(v: &Value) -> Quaternion {
    let [x, y, z, w] = fixed::<4>(v);
    Quaternion::new(x, y, z, w)
}
pub fn m2(v: &Value) -> Matrix2 {
    Matrix2(fixed::<4>(v))
}
pub fn m3(v: &Value) -> Matrix3 {
    Matrix3(fixed::<9>(v))
}
pub fn m4(v: &Value) -> Matrix4 {
    Matrix4(fixed::<16>(v))
}
pub fn carto(v: &Value) -> Cartographic {
    let [a, b, c] = fixed::<3>(v);
    Cartographic::new(a, b, c)
}
pub fn hpr(v: &Value) -> HeadingPitchRoll {
    let [a, b, c] = fixed::<3>(v);
    HeadingPitchRoll::new(a, b, c)
}
pub fn sph(v: &Value) -> Spherical {
    let [a, b, c] = fixed::<3>(v);
    Spherical::new(a, b, c)
}
pub fn rect(v: &Value) -> Rectangle {
    let [a, b, c, d] = fixed::<4>(v);
    Rectangle::new(a, b, c, d)
}
pub fn color(v: &Value) -> Color {
    let [a, b, c, d] = fixed::<4>(v);
    Color::new(a, b, c, d)
}
pub fn bsphere(v: &Value) -> BoundingSphere {
    let [x, y, z, r] = fixed::<4>(v);
    BoundingSphere::new(Cartesian3::new(x, y, z), r)
}
pub fn plane(v: &Value) -> Plane {
    let [x, y, z, d] = fixed::<4>(v);
    Plane { normal: Cartesian3::new(x, y, z), distance: d }
}
pub fn brect(v: &Value) -> BoundingRectangle {
    let [a, b, c, d] = fixed::<4>(v);
    BoundingRectangle::new(a, b, c, d)
}

// ---- encoding Rust values ------------------------------------------------

pub trait Enc {
    fn enc(&self) -> Value;
}

pub fn num(x: f64) -> Value {
    if x.is_nan() {
        json!("NaN")
    } else if x == f64::INFINITY {
        json!("Infinity")
    } else if x == f64::NEG_INFINITY {
        json!("-Infinity")
    } else if x == 0.0 && x.is_sign_negative() {
        json!("-0")
    } else {
        json!(x)
    }
}

impl Enc for f64 {
    fn enc(&self) -> Value {
        num(*self)
    }
}
impl Enc for bool {
    fn enc(&self) -> Value {
        json!(self)
    }
}
impl Enc for i32 {
    fn enc(&self) -> Value {
        json!(self)
    }
}
impl Enc for u32 {
    fn enc(&self) -> Value {
        json!(self)
    }
}
impl Enc for String {
    fn enc(&self) -> Value {
        json!(self)
    }
}
impl Enc for Value {
    fn enc(&self) -> Value {
        self.clone()
    }
}
impl<T: Enc> Enc for Option<T> {
    fn enc(&self) -> Value {
        self.as_ref().map_or(Value::Null, Enc::enc)
    }
}
impl<T: Enc> Enc for Vec<T> {
    fn enc(&self) -> Value {
        Value::Array(self.iter().map(Enc::enc).collect())
    }
}
impl<T: Enc, const K: usize> Enc for [T; K] {
    fn enc(&self) -> Value {
        Value::Array(self.iter().map(Enc::enc).collect())
    }
}
impl<A: Enc, B: Enc> Enc for (A, B) {
    fn enc(&self) -> Value {
        Value::Array(vec![self.0.enc(), self.1.enc()])
    }
}
fn arr(v: &[f64]) -> Value {
    Value::Array(v.iter().map(|x| num(*x)).collect())
}
impl Enc for Cartesian2 {
    fn enc(&self) -> Value {
        arr(&self.to_array())
    }
}
impl Enc for Cartesian3 {
    fn enc(&self) -> Value {
        arr(&self.to_array())
    }
}
impl Enc for Cartesian4 {
    fn enc(&self) -> Value {
        arr(&self.to_array())
    }
}
impl Enc for Quaternion {
    fn enc(&self) -> Value {
        arr(&[self.x, self.y, self.z, self.w])
    }
}
impl Enc for Matrix2 {
    fn enc(&self) -> Value {
        arr(&self.0)
    }
}
impl Enc for Matrix3 {
    fn enc(&self) -> Value {
        arr(&self.0)
    }
}
impl Enc for Matrix4 {
    fn enc(&self) -> Value {
        arr(&self.0)
    }
}
impl Enc for Cartographic {
    fn enc(&self) -> Value {
        arr(&[self.longitude, self.latitude, self.height])
    }
}
impl Enc for HeadingPitchRoll {
    fn enc(&self) -> Value {
        arr(&[self.heading, self.pitch, self.roll])
    }
}
impl Enc for Spherical {
    fn enc(&self) -> Value {
        arr(&[self.clock, self.cone, self.magnitude])
    }
}
impl Enc for Rectangle {
    fn enc(&self) -> Value {
        arr(&[self.west, self.south, self.east, self.north])
    }
}
impl Enc for Color {
    fn enc(&self) -> Value {
        arr(&[self.red, self.green, self.blue, self.alpha])
    }
}
impl Enc for BoundingSphere {
    fn enc(&self) -> Value {
        arr(&[self.center.x, self.center.y, self.center.z, self.radius])
    }
}
impl Enc for Plane {
    fn enc(&self) -> Value {
        arr(&[self.normal.x, self.normal.y, self.normal.z, self.distance])
    }
}
impl Enc for Interval {
    fn enc(&self) -> Value {
        arr(&[self.start, self.stop])
    }
}
impl Enc for BoundingRectangle {
    fn enc(&self) -> Value {
        arr(&[self.x, self.y, self.width, self.height])
    }
}
impl Enc for Intersect {
    fn enc(&self) -> Value {
        json!(*self as i8)
    }
}

/// What the port did for one case.
pub enum Out {
    Ok(Value),
    /// The port reported an error (Cesium `RuntimeError` or an invalid argument).
    Err(String),
}

pub fn ok<T: Enc>(v: T) -> Option<Out> {
    Some(Out::Ok(v.enc()))
}

pub fn res<T: Enc, E: std::fmt::Display>(v: Result<T, E>) -> Option<Out> {
    Some(match v {
        Ok(v) => Out::Ok(v.enc()),
        Err(e) => Out::Err(e.to_string()),
    })
}

/// Relative and absolute tolerance for one comparison. With `vector`, a
/// component of a flat numeric array may also differ by `rel` times the
/// array's largest magnitude; use it for positions computed through
/// cancellation, never for mixed-unit arrays like `[lon, lat, height]`.
#[derive(Clone, Copy)]
pub struct Tol {
    pub rel: f64,
    pub abs: f64,
    pub vector: bool,
}

pub const EXACT: Tol = Tol { rel: 0.0, abs: 0.0, vector: false };
pub const TIGHT: Tol = Tol { rel: 1e-12, abs: 1e-12, vector: false };
/// [`TIGHT`] relative to each array's norm.
pub const TIGHT_VECTOR: Tol = Tol { rel: 1e-12, abs: 1e-12, vector: true };

fn flat_scale(x: &[Value], y: &[Value]) -> Option<f64> {
    let mut scale: f64 = 0.0;
    for v in x.iter().chain(y) {
        match v {
            Value::Number(_) => scale = scale.max(n(v).abs()),
            Value::String(_) | Value::Null => {}
            _ => return None,
        }
    }
    Some(scale)
}

fn close(a: &Value, b: &Value, tol: Tol) -> bool {
    match (a, b) {
        (Value::Array(x), Value::Array(y)) => {
            let tol = match flat_scale(x, y) {
                Some(scale) if tol.vector => Tol { abs: tol.abs.max(tol.rel * scale), ..tol },
                _ => tol,
            };
            x.len() == y.len() && x.iter().zip(y).all(|(p, q)| close(p, q, tol))
        }
        (Value::Object(x), Value::Object(y)) => {
            x.len() == y.len() && x.iter().all(|(k, p)| y.get(k).is_some_and(|q| close(p, q, tol)))
        }
        (Value::Number(_) | Value::String(_), Value::Number(_) | Value::String(_)) => {
            let special = |v: &Value| matches!(v.as_str(), Some("NaN" | "Infinity" | "-Infinity" | "-0"));
            if a.is_string() && b.is_string() && !(special(a) && special(b)) {
                return a == b;
            }
            if (a.is_string() && !special(a)) || (b.is_string() && !special(b)) {
                return false;
            }
            let (p, q) = (n(a), n(b));
            if p.is_nan() || q.is_nan() {
                return p.is_nan() && q.is_nan();
            }
            if p.is_infinite() || q.is_infinite() {
                return p == q;
            }
            let diff = (p - q).abs();
            diff <= tol.abs || diff <= tol.rel * p.abs().max(q.abs())
        }
        // JS `undefined` inside an array is written as `null`; the port uses NaN.
        (Value::Null, Value::String(s)) | (Value::String(s), Value::Null) => s == "NaN",
        _ => a == b,
    }
}

/// Runs every case of `package/module`. `port` maps a Cesium function name
/// and its arguments to the Rust result, or `None` if the name is unknown.
/// `tol` picks the tolerance per function.
pub fn run(package: &str, module: &str, tol: impl Fn(&str) -> Tol, port: impl Fn(&str, &[Value]) -> Option<Out>) {
    let cases = load(package, module);
    let mut unknown = BTreeSet::new();
    let mut failures = Vec::new();
    let mut checked = 0;
    let mut preconditions = 0;
    for (i, c) in cases.iter().enumerate() {
        let expected_throw = c.r.get("throws").is_some();
        let kind = c.r.get("kind").and_then(Value::as_str).unwrap_or("");
        if expected_throw && kind == "DeveloperError" {
            // Debug-build argument check; release Cesium and the port do not raise it.
            preconditions += 1;
            continue;
        }
        let out = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| port(&c.f, &c.a)));
        let out = match out {
            Ok(Some(o)) => o,
            Ok(None) => {
                unknown.insert(c.f.clone());
                continue;
            }
            Err(p) => {
                let msg = p
                    .downcast_ref::<String>()
                    .cloned()
                    .or_else(|| p.downcast_ref::<&str>().map(|s| s.to_string()))
                    .unwrap_or_default();
                Out::Err(format!("panic: {msg}"))
            }
        };
        checked += 1;
        match (out, expected_throw) {
            (Out::Err(_), true) => {}
            (Out::Ok(v), false) if close(&v, &c.r, tol(&c.f)) => {}
            (Out::Ok(v), _) => failures.push(format!("#{i} {}({}) expected {} got {}", c.f, Value::Array(c.a.clone()), c.r, v)),
            (Out::Err(e), false) => failures.push(format!("#{i} {}({}) expected {} got error {e}", c.f, Value::Array(c.a.clone()), c.r)),
        }
    }
    let mut report = String::new();
    if !unknown.is_empty() {
        report += &format!("{module}: no port for {unknown:?}\n");
    }
    for f in failures.iter().take(15) {
        report += f;
        report += "\n";
    }
    assert!(
        unknown.is_empty() && failures.is_empty(),
        "{report}{module}: {} of {checked} cases failed",
        failures.len()
    );
    eprintln!("{module}: {checked} cases match CesiumJS ({preconditions} debug-only preconditions skipped)");
}
