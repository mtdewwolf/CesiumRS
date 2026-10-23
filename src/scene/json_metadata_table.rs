//! CesiumJS `JsonMetadataTable`: the JSON properties of a 3D Tiles 1.0
//! batch table, which have no class.

use super::metadata_value::{developer, json_entries, JsMap, MetadataError, MetadataValue};
use serde_json::Value;

type Result<T> = std::result::Result<T, MetadataError>;

/// A JSON property as JavaScript indexes it: an array, whose holes are
/// `undefined`; an object, by key; a string, by UTF-16 unit. Other values
/// have no elements.
#[derive(Clone, Debug, PartialEq)]
pub(crate) enum JsonColumn {
    Null,
    Array(Vec<Option<MetadataValue>>),
    Object(JsMap<MetadataValue>),
    String(String),
    Primitive(MetadataValue),
}

impl JsonColumn {
    pub(crate) fn from_json(value: &Value) -> Self {
        match value {
            Value::Null => Self::Null,
            Value::Array(items) => Self::Array(items.iter().map(|v| Some(MetadataValue::from_json(v))).collect()),
            Value::Object(_) => Self::Object(
                json_entries(Some(value)).into_iter().map(|(k, v)| (k, MetadataValue::from_json(v))).collect(),
            ),
            Value::String(s) => Self::String(s.clone()),
            other => Self::Primitive(MetadataValue::from_json(other)),
        }
    }

    /// `defined(column)`.
    pub(crate) fn is_defined(&self) -> bool {
        !matches!(self, Self::Null)
    }

    /// `column[index]`.
    pub(crate) fn get(&self, index: usize) -> Option<MetadataValue> {
        match self {
            Self::Array(items) => items.get(index).cloned().flatten(),
            Self::Object(map) => map.get(&index.to_string()).cloned(),
            Self::String(s) => {
                s.encode_utf16().nth(index).map(|unit| MetadataValue::String(String::from_utf16_lossy(&[unit])))
            }
            Self::Null | Self::Primitive(_) => None,
        }
    }

    /// `column[index] = value`. Arrays grow with holes; a primitive throws,
    /// as Cesium's modules are strict.
    pub(crate) fn set(&mut self, index: usize, value: MetadataValue) -> Result<()> {
        match self {
            Self::Array(items) => {
                if items.len() <= index {
                    items.resize(index + 1, None);
                }
                items[index] = Some(value);
                Ok(())
            }
            Self::Object(map) => {
                map.insert(index.to_string(), value);
                Ok(())
            }
            Self::Null => Err(MetadataError::Type(format!("Cannot set properties of null (setting '{index}')"))),
            Self::String(s) => Err(MetadataError::Type(format!("Cannot create property '{index}' on string '{s}'"))),
            Self::Primitive(v) => Err(MetadataError::Type(format!(
                "Cannot create property '{index}' on {} '{}'",
                if matches!(v, MetadataValue::Boolean(_)) { "boolean" } else { "number" },
                v.js_string()
            ))),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct JsonMetadataTable {
    count: usize,
    properties: JsMap<JsonColumn>,
}

impl JsonMetadataTable {
    /// `count` rows of the JSON `properties` object, copied.
    pub fn new(count: usize, properties: &Value) -> Result<Self> {
        if count == 0 {
            return Err(developer("Expected options.count to be greater than 0, actual value was 0"));
        }
        if !(properties.is_object() || properties.is_array()) {
            return Err(developer("Expected options.properties to be typeof object"));
        }
        let properties =
            json_entries(Some(properties)).into_iter().map(|(k, v)| (k, JsonColumn::from_json(v))).collect();
        Ok(Self { count, properties })
    }

    /// Whether the property is defined (not `null`).
    pub fn has_property(&self, property_id: &str) -> bool {
        self.properties.get(property_id).is_some_and(JsonColumn::is_defined)
    }

    pub fn get_property_ids(&self) -> Vec<String> {
        self.properties.iter().filter(|(_, c)| c.is_defined()).map(|(k, _)| k.to_string()).collect()
    }

    /// Row `index` of the property, `None` where that is `undefined`.
    pub fn get_property(&self, index: usize, property_id: &str) -> Result<Option<MetadataValue>> {
        self.check_index(index)?;
        Ok(self.properties.get(property_id).filter(|c| c.is_defined()).and_then(|c| c.get(index)))
    }

    /// Stores the value, creating the property as an array of `count` holes
    /// when it is not defined.
    pub fn set_property(&mut self, index: usize, property_id: &str, value: MetadataValue) -> Result<()> {
        self.check_index(index)?;
        if !self.has_property(property_id) {
            self.properties.insert(property_id, JsonColumn::Array(vec![None; self.count]));
        }
        self.properties.get_mut(property_id).expect("just inserted").set(index, value)
    }

    fn check_index(&self, index: usize) -> Result<()> {
        if index >= self.count {
            return Err(developer(format!("index must be in the range [0, {})", self.count)));
        }
        Ok(())
    }
}
