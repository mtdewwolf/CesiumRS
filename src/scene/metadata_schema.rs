//! CesiumJS `MetadataSchema`: the classes and enums of a 3D Metadata schema.

use super::metadata_value::{developer, json_entries, json_string, JsMap, MetadataError};
use super::{MetadataClass, MetadataEnum};
use serde_json::Value;
use std::sync::Arc;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct MetadataSchema {
    pub classes: JsMap<Arc<MetadataClass>>,
    pub enums: JsMap<Arc<MetadataEnum>>,
    pub id: Option<String>,
    pub name: Option<String>,
    pub description: Option<String>,
    pub version: Option<String>,
    pub extras: Option<Value>,
    pub extensions: Option<Value>,
}

impl MetadataSchema {
    /// `MetadataSchema.fromJson`: enums first, so classes can refer to them.
    pub fn from_json(schema: &Value) -> Result<Self, MetadataError> {
        if !schema.is_object() {
            return Err(developer("Expected schema to be typeof object"));
        }
        let mut enums = JsMap::new();
        for (enum_id, definition) in json_entries(schema.get("enums")) {
            let parsed = MetadataEnum::from_json(&enum_id, definition)?;
            enums.insert(enum_id, Arc::new(parsed));
        }
        let mut classes = JsMap::new();
        for (class_id, definition) in json_entries(schema.get("classes")) {
            let parsed = MetadataClass::from_json(&class_id, definition, &enums)?;
            classes.insert(class_id, Arc::new(parsed));
        }
        Ok(Self {
            classes,
            enums,
            id: json_string(schema.get("id")),
            name: json_string(schema.get("name")),
            description: json_string(schema.get("description")),
            version: json_string(schema.get("version")),
            extras: schema.get("extras").cloned(),
            extensions: schema.get("extensions").cloned(),
        })
    }
}
