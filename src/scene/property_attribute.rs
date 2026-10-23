//! CesiumJS `PropertyAttribute`: a set of 3D Metadata properties of one
//! class stored in glTF vertex attributes.

use super::metadata_value::{developer, json_entries, JsMap, MetadataError};
use super::{MetadataClass, PropertyAttributeProperty};
use serde_json::Value;
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq)]
pub struct PropertyAttribute {
    name: Option<String>,
    id: Option<usize>,
    class: Arc<MetadataClass>,
    properties: JsMap<PropertyAttributeProperty>,
    extras: Option<Value>,
    extensions: Option<Value>,
}

impl PropertyAttribute {
    /// The property attribute JSON, each of whose properties the class must
    /// define.
    pub fn new(
        property_attribute: &Value,
        class: Arc<MetadataClass>,
        name: Option<String>,
        id: Option<usize>,
    ) -> Result<Self, MetadataError> {
        if !(property_attribute.is_object() || property_attribute.is_array()) {
            return Err(developer("Expected options.propertyAttribute to be typeof object"));
        }
        let mut properties = JsMap::new();
        for (property_id, property) in json_entries(property_attribute.get("properties")) {
            let class_property = class.properties.get(&property_id).cloned().ok_or_else(|| {
                developer("Expected options.classProperty to be typeof object, actual typeof was undefined")
            })?;
            properties.insert(property_id, PropertyAttributeProperty::new(property, class_property)?);
        }
        Ok(Self {
            name,
            id,
            class,
            properties,
            extras: property_attribute.get("extras").cloned(),
            extensions: property_attribute.get("extensions").cloned(),
        })
    }

    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    pub fn id(&self) -> Option<usize> {
        self.id
    }

    pub fn class(&self) -> &Arc<MetadataClass> {
        &self.class
    }

    pub fn properties(&self) -> &JsMap<PropertyAttributeProperty> {
        &self.properties
    }

    pub fn extras(&self) -> Option<&Value> {
        self.extras.as_ref()
    }

    pub fn extensions(&self) -> Option<&Value> {
        self.extensions.as_ref()
    }

    pub fn get_property(&self, property_id: &str) -> Option<&PropertyAttributeProperty> {
        self.properties.get(property_id)
    }
}
