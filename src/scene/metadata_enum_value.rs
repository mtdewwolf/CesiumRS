//! CesiumJS `MetadataEnumValue`: one name and value of a metadata enum.

use super::metadata_value::{developer, json_string, MetadataError};
use serde_json::Value;

#[derive(Clone, Debug, PartialEq)]
pub struct MetadataEnumValue {
    pub value: f64,
    pub name: String,
    pub description: Option<String>,
    pub extras: Option<Value>,
    pub extensions: Option<Value>,
}

impl MetadataEnumValue {
    /// `MetadataEnumValue.fromJson`. A missing number `value` or string
    /// `name` is a `DeveloperError` in Cesium.
    pub fn from_json(value: &Value) -> Result<Self, MetadataError> {
        if !value.is_object() {
            return Err(developer("Expected value to be typeof object"));
        }
        let number = value
            .get("value")
            .and_then(Value::as_f64)
            .ok_or_else(|| developer("Expected options.value to be typeof number"))?;
        let name =
            json_string(value.get("name")).ok_or_else(|| developer("Expected options.name to be typeof string"))?;
        Ok(Self {
            value: number,
            name,
            description: json_string(value.get("description")),
            extras: value.get("extras").cloned(),
            extensions: value.get("extensions").cloned(),
        })
    }
}
