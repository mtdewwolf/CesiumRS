//! CesiumJS `TilesetMetadata`: the metadata of a whole tileset (3D Tiles 1.1
//! `metadata`, or the `3DTILES_metadata` extension's `tileset`).

use super::metadata_entity::{entity_json, json_metadata_entity};
use super::{MetadataClass, MetadataError, MetadataProperties};
use serde_json::Value;
use std::sync::Arc;

#[derive(Clone, Debug, PartialEq)]
pub struct TilesetMetadata {
    class: Arc<MetadataClass>,
    properties: MetadataProperties,
    extras: Option<Value>,
    extensions: Option<Value>,
}

impl TilesetMetadata {
    /// The entity JSON and the class it conforms to.
    pub fn new(tileset: &Value, class: Arc<MetadataClass>) -> Result<Self, MetadataError> {
        let json = entity_json(tileset, "tileset")?;
        Ok(Self { class, properties: json.properties, extras: json.extras, extensions: json.extensions })
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

json_metadata_entity!(TilesetMetadata);
