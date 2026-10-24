//! Native value semantics: owned math values clone/equal; Rc values retain
//! JavaScript object identity. Option<T>::None represents null; an outer
//! Property result of None represents undefined.
use crate::core::*;
use serde_json::Value;
use std::{
    cell::{Ref, RefCell, RefMut},
    rc::Rc,
};
/// Value operations used by property equality, filtering and string conversion.
/// Use `Rc` or `JsonReference` for JavaScript reference identity.
pub trait PropertyValue: Clone + 'static {
    fn property_equals(&self, other: &Self) -> bool;
    fn strict_equals(&self, other: &Self) -> bool {
        self.property_equals(other)
    }
    fn has_equals_method(&self) -> bool {
        true
    }
    fn equals_method(&self, other: &Self) -> Result<bool, RuntimeError> {
        if self.has_equals_method() {
            Ok(self.property_equals(other))
        } else {
            Err(RuntimeError::new("left.equals is not a function"))
        }
    }
    fn is_defined(&self) -> bool {
        true
    }
    fn property_string(&self) -> String {
        "[object Object]".into()
    }
}
/// Values whose Cesium counterpart provides `.clone(result)`.
pub trait CloneablePropertyValue: PropertyValue {}
impl PropertyValue for f64 {
    fn has_equals_method(&self) -> bool {
        false
    }
    fn property_equals(&self, o: &Self) -> bool {
        self == o
    }
    fn property_string(&self) -> String {
        math::js_number_to_string(*self)
    }
}
macro_rules! scalar {
    ($($t:ty),*) => {
        $(impl PropertyValue for $t {
            fn has_equals_method(&self) -> bool { false }
            fn property_equals(&self, other: &Self) -> bool { self == other }
            fn property_string(&self) -> String { self.to_string() }
        })*
    };
}
scalar!(bool, String, i32, u32, i64, u64, usize, i128, u128);
impl<T: PropertyValue> PropertyValue for Option<T> {
    fn has_equals_method(&self) -> bool {
        self.as_ref().is_some_and(PropertyValue::has_equals_method)
    }
    fn property_equals(&self, o: &Self) -> bool {
        match (self, o) {
            (None, None) => true,
            (Some(a), Some(b)) => a.property_equals(b),
            _ => false,
        }
    }
    fn strict_equals(&self, o: &Self) -> bool {
        match (self, o) {
            (None, None) => true,
            (Some(a), Some(b)) => a.strict_equals(b),
            _ => false,
        }
    }
    fn is_defined(&self) -> bool {
        self.is_some()
    }
    fn property_string(&self) -> String {
        self.as_ref().map_or_else(|| "null".into(), PropertyValue::property_string)
    }
}
impl<T: 'static> PropertyValue for Rc<T> {
    fn has_equals_method(&self) -> bool {
        false
    }
    fn property_equals(&self, o: &Self) -> bool {
        Rc::ptr_eq(self, o)
    }
}
impl<T: PropertyValue> PropertyValue for Vec<T> {
    fn has_equals_method(&self) -> bool {
        false
    }
    fn property_equals(&self, o: &Self) -> bool {
        self.len() == o.len() && self.iter().zip(o).all(|(a, b)| a.property_equals(b))
    }
    fn strict_equals(&self, o: &Self) -> bool {
        std::ptr::eq(self, o)
    }
    fn property_string(&self) -> String {
        self.iter()
            .map(|v| if v.is_defined() { v.property_string() } else { String::new() })
            .collect::<Vec<_>>()
            .join(",")
    }
}
fn tuple(values: &[f64]) -> String {
    format!("({})", values.iter().map(|v| math::js_number_to_string(*v)).collect::<Vec<_>>().join(", "))
}
macro_rules! math_value {
    ($t:ty,$text:expr) => {
        impl PropertyValue for $t {
            fn property_equals(&self, o: &Self) -> bool {
                self == o
            }
            fn strict_equals(&self, o: &Self) -> bool {
                std::ptr::eq(self, o)
            }
            fn property_string(&self) -> String {
                ($text)(self)
            }
        }
        impl CloneablePropertyValue for $t {}
    };
}
math_value!(Cartesian2, |v: &Cartesian2| tuple(&[v.x, v.y]));
math_value!(Cartesian3, |v: &Cartesian3| tuple(&[v.x, v.y, v.z]));
math_value!(Cartesian4, |v: &Cartesian4| tuple(&[v.x, v.y, v.z, v.w]));
math_value!(Color, |v: &Color| tuple(&[v.red, v.green, v.blue, v.alpha]));
math_value!(Quaternion, |v: &Quaternion| tuple(&[v.x, v.y, v.z, v.w]));
fn matrix(values: &[f64], n: usize) -> String {
    (0..n).map(|r| tuple(&(0..n).map(|c| values[c * n + r]).collect::<Vec<_>>())).collect::<Vec<_>>().join("\n")
}
math_value!(Matrix2, |v: &Matrix2| matrix(&v.0, 2));
math_value!(Matrix3, |v: &Matrix3| matrix(&v.0, 3));
math_value!(Matrix4, |v: &Matrix4| matrix(&v.0, 4));
macro_rules! object_value {($($t:ty),*)=>{$(math_value!($t,|_:&$t|"[object Object]".into());)*};}
object_value!(Rectangle, BoundingRectangle, BoundingSphere, NearFarScalar, DistanceDisplayCondition);
math_value!(Ellipsoid, |v: &Ellipsoid| tuple(&[v.radii.x, v.radii.y, v.radii.z]));
impl PropertyValue for VertexFormat {
    fn has_equals_method(&self) -> bool {
        false
    }
    fn property_equals(&self, _: &Self) -> bool {
        false
    }
    fn strict_equals(&self, o: &Self) -> bool {
        std::ptr::eq(self, o)
    }
}
impl CloneablePropertyValue for VertexFormat {}
/// Mutable JSON objects/arrays with reference equality and shared reads.
#[derive(Clone)]
pub struct JsonReference(Rc<RefCell<Value>>);
impl JsonReference {
    pub fn array(value: Vec<Value>) -> Self {
        Self(Rc::new(RefCell::new(Value::Array(value))))
    }
    pub fn object(value: serde_json::Map<String, Value>) -> Self {
        Self(Rc::new(RefCell::new(Value::Object(value))))
    }
    pub fn value(&self) -> Ref<'_, Value> {
        self.0.borrow()
    }
    pub fn value_mut(&self) -> RefMut<'_, Value> {
        self.0.borrow_mut()
    }
}
fn json_string(v: &Value) -> String {
    match v {
        Value::Null => "null".into(),
        Value::Bool(b) => b.to_string(),
        Value::Number(n) => math::js_number_to_string(n.as_f64().unwrap_or(f64::NAN)),
        Value::String(s) => s.clone(),
        Value::Object(_) => "[object Object]".into(),
        Value::Array(a) => {
            a.iter().map(|v| if v.is_null() { String::new() } else { json_string(v) }).collect::<Vec<_>>().join(",")
        }
    }
}
impl PropertyValue for JsonReference {
    fn has_equals_method(&self) -> bool {
        false
    }
    fn property_equals(&self, o: &Self) -> bool {
        Rc::ptr_eq(&self.0, &o.0)
    }
    fn property_string(&self) -> String {
        json_string(&self.0.borrow())
    }
}

impl PropertyValue for crate::scene::HeightReference {
    fn property_equals(&self, other: &Self) -> bool {
        self == other
    }
    fn has_equals_method(&self) -> bool {
        false
    }
    fn property_string(&self) -> String {
        (*self as i32).to_string()
    }
}
