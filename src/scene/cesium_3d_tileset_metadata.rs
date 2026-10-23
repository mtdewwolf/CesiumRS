//! CesiumJS `Cesium3DTilesetMetadata`: a tileset's metadata (3D Tiles 1.1, or
//! the `3DTILES_metadata` extension): its tileset entity and its groups.

use super::{GroupMetadata, MetadataError, MetadataSchema, TilesetMetadata};
use crate::scene::gltf_pipeline::js;
use serde_json::Value;
use std::sync::Arc;

type Result<T> = std::result::Result<T, MetadataError>;

#[derive(Clone, Debug, PartialEq)]
pub struct Cesium3DTilesetMetadata {
    schema: Arc<MetadataSchema>,
    groups: Vec<GroupMetadata>,
    group_ids: Vec<String>,
    tileset: Option<TilesetMetadata>,
    statistics: Option<Value>,
    extras: Option<Value>,
    extensions: Option<Value>,
}

/// `schema.classes[json.class]`, which each entity requires.
fn class_of(schema: &MetadataSchema, json: &Value) -> Result<Arc<super::MetadataClass>> {
    let key = json.get("class").map_or_else(|| "undefined".to_string(), js::key);
    schema.classes.get(&key).cloned().ok_or_else(|| {
        MetadataError::Developer("Expected options.class to be typeof object, actual typeof was undefined".into())
    })
}

/// `group.class` throws on `null`; on another primitive the group itself
/// fails its check.
fn group(schema: &MetadataSchema, json: &Value, id: Option<String>) -> Result<GroupMetadata> {
    if json.is_null() {
        return Err(MetadataError::Type("Cannot read properties of null (reading 'class')".into()));
    }
    if !(json.is_object() || json.is_array()) {
        return Err(MetadataError::Developer("Expected options.group to be typeof object".into()));
    }
    GroupMetadata::new(json, class_of(schema, json)?, id)
}

impl Cesium3DTilesetMetadata {
    /// The metadata JSON, with the schema the caller has loaded.
    pub fn new(metadata_json: &Value, schema: Arc<MetadataSchema>) -> Result<Self> {
        if !(metadata_json.is_object() || metadata_json.is_array()) {
            return Err(MetadataError::Developer("Expected options.metadataJson to be typeof object".into()));
        }
        // An older schema kept the tileset entity in `tileset`.
        let metadata = metadata_json.get("metadata").filter(|v| !v.is_null()).or(metadata_json.get("tileset"));
        let tileset = match metadata.filter(|v| !v.is_null()) {
            Some(json) if !(json.is_object() || json.is_array()) => {
                return Err(MetadataError::Developer("Expected options.tileset to be typeof object".into()));
            }
            Some(json) => Some(TilesetMetadata::new(json, class_of(&schema, json)?)?),
            None => None,
        };

        let mut groups = Vec::new();
        let mut group_ids = Vec::new();
        match metadata_json.get("groups") {
            Some(Value::Array(items)) => {
                for item in items {
                    groups.push(group(&schema, item, None)?);
                }
            }
            // The older dictionary of groups, in sorted id order.
            Some(Value::Object(map)) => {
                let mut ids: Vec<String> = map.keys().cloned().collect();
                ids.sort_by(|a, b| a.encode_utf16().cmp(b.encode_utf16()));
                for id in &ids {
                    groups.push(group(&schema, &map[id], Some(id.clone()))?);
                }
                group_ids = ids;
            }
            // A string's characters would each fail the group check.
            Some(Value::String(s)) if !s.is_empty() => {
                return Err(MetadataError::Developer("Expected options.group to be typeof object".into()));
            }
            _ => {}
        }

        Ok(Self {
            schema,
            groups,
            group_ids,
            tileset,
            statistics: metadata_json.get("statistics").cloned(),
            extras: metadata_json.get("extras").cloned(),
            extensions: metadata_json.get("extensions").cloned(),
        })
    }

    pub fn schema(&self) -> &Arc<MetadataSchema> {
        &self.schema
    }

    pub fn groups(&self) -> &[GroupMetadata] {
        &self.groups
    }

    pub fn groups_mut(&mut self) -> &mut [GroupMetadata] {
        &mut self.groups
    }

    /// The ids of dictionary-form groups, in `groups` order; empty for an
    /// array of groups.
    pub fn group_ids(&self) -> &[String] {
        &self.group_ids
    }

    pub fn tileset(&self) -> Option<&TilesetMetadata> {
        self.tileset.as_ref()
    }

    pub fn tileset_mut(&mut self) -> Option<&mut TilesetMetadata> {
        self.tileset.as_mut()
    }

    pub fn statistics(&self) -> Option<&Value> {
        self.statistics.as_ref()
    }

    pub fn extras(&self) -> Option<&Value> {
        self.extras.as_ref()
    }

    pub fn extensions(&self) -> Option<&Value> {
        self.extensions.as_ref()
    }
}
