//! Heterogeneous property values. Math values own their data; JSON objects keep identity.
use super::{JsonReference, PropertyValue};
use crate::core::*;
use serde_json::{json, Value};
use std::rc::Rc;
pub type BagValues = Vec<(String, Option<DynamicValue>)>;
#[derive(Clone)]
pub enum DynamicValue {
    Entity(std::rc::Weak<super::Entity>),
    Availability(std::rc::Weak<super::PropertyIntervals<()>>),
    Null,
    Boolean(bool),
    Number(f64),
    String(String),
    Cartesian2(Cartesian2),
    Cartesian3(Cartesian3),
    Cartesian4(Cartesian4),
    Quaternion(Quaternion),
    Color(Color),
    Matrix3(Matrix3),
    Matrix4(Matrix4),
    Date(Rc<JulianDate>),
    Json(JsonReference),
    Record(BagValues),
}
fn json_order(value: Value) -> Value {
    match value {
        Value::Object(value) => Value::Object(
            crate::core::query::js_object_order(value.into_iter().map(|(k, v)| (k, json_order(v))).collect())
                .into_iter()
                .collect(),
        ),
        Value::Array(value) => Value::Array(value.into_iter().map(json_order).collect()),
        value => value,
    }
}
impl DynamicValue {
    pub fn from_json(value: Value) -> Self {
        match json_order(value) {
            Value::Null => Self::Null,
            Value::Bool(v) => Self::Boolean(v),
            Value::Number(v) => Self::Number(v.as_f64().unwrap_or(f64::NAN)),
            Value::String(v) => Self::String(v),
            Value::Array(v) => Self::Json(JsonReference::array(v)),
            Value::Object(v) => Self::Json(JsonReference::object(v)),
        }
    }
    pub fn to_json(&self) -> Value {
        match self {
            Self::Entity(v) => v.upgrade().map_or(Value::Null, |v| json!(v.id())),
            Self::Availability(v) => v.upgrade().map_or(Value::Null, |v| {
                json!([v.read().start().map(|t| t.to_iso8601(None)), v.read().stop().map(|t| t.to_iso8601(None))])
            }),
            Self::Null => Value::Null,
            Self::Boolean(v) => json!(v),
            Self::Number(v) => json!(v),
            Self::String(v) => json!(v),
            Self::Cartesian2(v) => json!(v.to_array()),
            Self::Cartesian3(v) => json!(v.to_array()),
            Self::Cartesian4(v) => json!(v.to_array()),
            Self::Quaternion(v) => json!([v.x, v.y, v.z, v.w]),
            Self::Color(v) => json!([v.red, v.green, v.blue, v.alpha]),
            Self::Matrix3(v) => json!(v.0),
            Self::Matrix4(v) => json!(v.0),
            Self::Date(v) => json!(v.to_iso8601(None)),
            Self::Json(v) => json_order(v.value().clone()),
            Self::Record(v) => Value::Object(
                crate::core::query::js_object_order(
                    v.iter().filter_map(|(k, v)| v.as_ref().map(|v| (k.clone(), v.to_json()))).collect(),
                )
                .into_iter()
                .collect(),
            ),
        }
    }
    pub fn get(&self, name: &str) -> Option<Self> {
        match self {
            Self::Entity(v) => v.upgrade().and_then(|e| match e.get_property(name) {
                Some(super::PropertyInput::Value(v)) => Some(v),
                _ => None,
            }),
            Self::Record(v) => v.iter().find(|(k, _)| k == name).and_then(|(_, v)| v.clone()),
            Self::Json(v) => v.value().get(name).cloned().map(Self::from_json),
            _ => None,
        }
    }
}
impl PropertyValue for DynamicValue {
    fn property_equals(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Entity(a), Self::Entity(b)) => std::rc::Weak::ptr_eq(a, b),
            (Self::Availability(a), Self::Availability(b)) => std::rc::Weak::ptr_eq(a, b),
            (Self::Null, Self::Null) => true,
            (Self::Boolean(a), Self::Boolean(b)) => a == b,
            (Self::Number(a), Self::Number(b)) => a == b,
            (Self::String(a), Self::String(b)) => a == b,
            (Self::Cartesian2(a), Self::Cartesian2(b)) => a == b,
            (Self::Cartesian3(a), Self::Cartesian3(b)) => a == b,
            (Self::Cartesian4(a), Self::Cartesian4(b)) => a == b,
            (Self::Quaternion(a), Self::Quaternion(b)) => a == b,
            (Self::Color(a), Self::Color(b)) => a == b,
            (Self::Matrix3(a), Self::Matrix3(b)) => a == b,
            (Self::Matrix4(a), Self::Matrix4(b)) => a == b,
            (Self::Date(a), Self::Date(b)) => a == b,
            (Self::Json(a), Self::Json(b)) => a.property_equals(b),
            (Self::Record(a), Self::Record(b)) => {
                a.len() == b.len()
                    && a.iter().zip(b).all(|((k, a), (l, b))| {
                        k == l
                            && match (a, b) {
                                (None, None) => true,
                                (Some(a), Some(b)) => a.property_equals(b),
                                _ => false,
                            }
                    })
            }
            _ => false,
        }
    }
    fn strict_equals(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Null, Self::Null)
            | (Self::Boolean(_), Self::Boolean(_))
            | (Self::Number(_), Self::Number(_))
            | (Self::String(_), Self::String(_)) => self.property_equals(other),
            (Self::Json(a), Self::Json(b)) => a.property_equals(b),
            (Self::Date(a), Self::Date(b)) => Rc::ptr_eq(a, b),
            (Self::Entity(a), Self::Entity(b)) => std::rc::Weak::ptr_eq(a, b),
            (Self::Availability(a), Self::Availability(b)) => std::rc::Weak::ptr_eq(a, b),
            _ => std::ptr::eq(self, other),
        }
    }
    fn has_equals_method(&self) -> bool {
        matches!(
            self,
            Self::Cartesian2(_)
                | Self::Cartesian3(_)
                | Self::Cartesian4(_)
                | Self::Quaternion(_)
                | Self::Color(_)
                | Self::Matrix3(_)
                | Self::Matrix4(_)
                | Self::Date(_)
                | Self::Availability(_)
        )
    }
    fn is_defined(&self) -> bool {
        !matches!(self, Self::Null)
    }
    fn property_string(&self) -> String {
        match self {
            Self::Entity(_) | Self::Availability(_) => "[object Object]".into(),
            Self::Null => "null".into(),
            Self::Boolean(v) => v.property_string(),
            Self::Number(v) => v.property_string(),
            Self::String(v) => v.clone(),
            Self::Cartesian2(v) => v.property_string(),
            Self::Cartesian3(v) => v.property_string(),
            Self::Cartesian4(v) => v.property_string(),
            Self::Quaternion(v) => v.property_string(),
            Self::Color(v) => v.property_string(),
            Self::Matrix3(v) => v.property_string(),
            Self::Matrix4(v) => v.property_string(),
            Self::Date(v) => v.to_iso8601(None),
            Self::Json(v) => v.property_string(),
            Self::Record(_) => "[object Object]".into(),
        }
    }
}
/// Conversion contract used by heterogeneous property adapters.
pub trait ComponentValue: PropertyValue {
    fn into_dynamic(self) -> DynamicValue;
    fn from_dynamic(value: &DynamicValue) -> Option<Self>;
}
macro_rules! value {
    ($t:ty,$variant:ident) => {
        impl From<$t> for DynamicValue {
            fn from(value: $t) -> Self {
                Self::$variant(value)
            }
        }
        impl ComponentValue for $t {
            fn into_dynamic(self) -> DynamicValue {
                DynamicValue::$variant(self)
            }
            fn from_dynamic(value: &DynamicValue) -> Option<Self> {
                if let DynamicValue::$variant(v) = value {
                    Some(v.clone())
                } else {
                    None
                }
            }
        }
    };
}
value!(bool, Boolean);
value!(f64, Number);
value!(String, String);
value!(Cartesian2, Cartesian2);
value!(Cartesian3, Cartesian3);
value!(Cartesian4, Cartesian4);
value!(Quaternion, Quaternion);
value!(Color, Color);
value!(Matrix3, Matrix3);
value!(Matrix4, Matrix4);
value!(Rc<JulianDate>, Date);
value!(JsonReference, Json);
impl From<&str> for DynamicValue {
    fn from(value: &str) -> Self {
        Self::String(value.into())
    }
}
impl ComponentValue for DynamicValue {
    fn into_dynamic(self) -> Self {
        self
    }
    fn from_dynamic(value: &Self) -> Option<Self> {
        Some(value.clone())
    }
}
impl<T: ComponentValue> ComponentValue for Option<T> {
    fn into_dynamic(self) -> DynamicValue {
        self.map_or(DynamicValue::Null, ComponentValue::into_dynamic)
    }
    fn from_dynamic(value: &DynamicValue) -> Option<Self> {
        if matches!(value, DynamicValue::Null) {
            Some(None)
        } else {
            T::from_dynamic(value).map(Some)
        }
    }
}
