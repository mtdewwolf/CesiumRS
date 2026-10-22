//! CesiumJS `MetadataClass`: a 3D Metadata class and its properties.

use super::metadata_value::{developer, json_entries, json_string, JsMap, MetadataError};
use super::{MetadataClassProperty, MetadataEnum};
use serde_json::Value;
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq)]
pub struct MetadataClass {
    pub id: String,
    pub name: Option<String>,
    pub description: Option<String>,
    /// In the class JSON's key order.
    pub properties: JsMap<Arc<MetadataClassProperty>>,
    /// The properties with a semantic; a later property with the same
    /// semantic wins.
    pub properties_by_semantic: JsMap<Arc<MetadataClassProperty>>,
    pub extras: Option<Value>,
    pub extensions: Option<Value>,
}

impl MetadataClass {
    /// The class Cesium builds for a 3D Tiles 1.0 batch table.
    pub const BATCH_TABLE_CLASS_NAME: &'static str = "_batchTable";

    pub fn new(id: impl Into<String>, properties: JsMap<Arc<MetadataClassProperty>>) -> Self {
        let mut properties_by_semantic = JsMap::new();
        for property in properties.values() {
            if let Some(semantic) = property.semantic() {
                properties_by_semantic.insert(semantic.to_string(), property.clone());
            }
        }
        Self {
            id: id.into(),
            name: None,
            description: None,
            properties,
            properties_by_semantic,
            extras: None,
            extensions: None,
        }
    }

    /// `MetadataClass.fromJson({ id, class, enums })`.
    pub fn from_json(id: &str, definition: &Value, enums: &JsMap<Arc<MetadataEnum>>) -> Result<Self, MetadataError> {
        if !definition.is_object() {
            return Err(developer("Expected options.class to be typeof object"));
        }
        let mut properties = JsMap::new();
        for (property_id, property) in json_entries(definition.get("properties")) {
            let parsed = MetadataClassProperty::from_json(&property_id, property, enums)?;
            properties.insert(property_id, Arc::new(parsed));
        }
        let mut class = Self::new(id, properties);
        class.name = json_string(definition.get("name"));
        class.description = json_string(definition.get("description"));
        class.extras = definition.get("extras").cloned();
        class.extensions = definition.get("extensions").cloned();
        Ok(class)
    }
}
