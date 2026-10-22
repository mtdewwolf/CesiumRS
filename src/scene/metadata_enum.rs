//! CesiumJS `MetadataEnum`: a 3D Metadata enum and its value lookups.

use super::metadata_value::{developer, json_string, JsMap, MetadataError};
use super::{MetadataComponentType, MetadataEnumValue};
use crate::core::math::js_number_to_string;
use serde_json::Value;

#[derive(Clone, Debug, PartialEq)]
pub struct MetadataEnum {
    pub id: String,
    pub values: Vec<MetadataEnumValue>,
    /// Names keyed by the value's JavaScript property key; later duplicates win.
    pub names_by_value: JsMap<String>,
    /// Values keyed by name; later duplicates win.
    pub values_by_name: JsMap<f64>,
    /// `UINT16` unless the definition names a component type.
    pub value_type: MetadataComponentType,
    pub name: Option<String>,
    pub description: Option<String>,
    pub extras: Option<Value>,
    pub extensions: Option<Value>,
}

impl MetadataEnum {
    pub fn new(
        id: impl Into<String>,
        values: Vec<MetadataEnumValue>,
        value_type: Option<MetadataComponentType>,
    ) -> Self {
        let mut names_by_value = JsMap::new();
        let mut values_by_name = JsMap::new();
        for value in &values {
            names_by_value.insert(js_number_to_string(value.value), value.name.clone());
            values_by_name.insert(value.name.clone(), value.value);
        }
        Self {
            id: id.into(),
            values,
            names_by_value,
            values_by_name,
            value_type: value_type.unwrap_or(MetadataComponentType::Uint16),
            name: None,
            description: None,
            extras: None,
            extensions: None,
        }
    }

    /// `MetadataEnum.fromJson({ id, enum })`.
    pub fn from_json(id: &str, definition: &Value) -> Result<Self, MetadataError> {
        if !definition.is_object() {
            return Err(developer("Expected options.enum to be typeof object"));
        }
        let Some(Value::Array(values)) = definition.get("values") else {
            return Err(MetadataError::Type("Cannot read properties of undefined (reading 'map')".into()));
        };
        let values = values.iter().map(MetadataEnumValue::from_json).collect::<Result<Vec<_>, _>>()?;
        let value_type =
            definition.get("valueType").and_then(Value::as_str).and_then(MetadataComponentType::from_str_value);
        let mut result = Self::new(id, values, value_type);
        result.name = json_string(definition.get("name"));
        result.description = json_string(definition.get("description"));
        result.extras = definition.get("extras").cloned();
        result.extensions = definition.get("extensions").cloned();
        Ok(result)
    }
}
