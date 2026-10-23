//! CesiumJS `GroupMetadata`: the metadata of a group of tile contents.

use super::metadata_entity::{entity_json, json_metadata_entity};
use super::{MetadataClass, MetadataError, MetadataProperties};
use serde_json::Value;
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq)]
pub struct GroupMetadata {
    id: Option<String>,
    class: Arc<MetadataClass>,
    properties: MetadataProperties,
    extras: Option<Value>,
    extensions: Option<Value>,
}

impl GroupMetadata {
    /// The group JSON and its class. Only groups kept in the older
    /// dictionary form have an id.
    pub fn new(group: &Value, class: Arc<MetadataClass>, id: Option<String>) -> Result<Self, MetadataError> {
        let json = entity_json(group, "group")?;
        Ok(Self { id, class, properties: json.properties, extras: json.extras, extensions: json.extensions })
    }

    pub fn id(&self) -> Option<&str> {
        self.id.as_deref()
    }

    pub fn class_arc(&self) -> &Arc<MetadataClass> {
        &self.class
    }

    pub fn extras(&self) -> Option<&Value> {
        self.extras.as_ref()
    }

    pub fn extensions(&self) -> Option<&Value> {
        self.extensions.as_ref()
    }
}

json_metadata_entity!(GroupMetadata);
