//! CesiumJS `ImplicitMetadataView`: one entity (a tile or a content) of an
//! implicit subtree, viewed through the row of the subtree's metadata table.
//!
//! The views of a subtree share its table, so the table sits behind a lock.

use super::{MetadataClass, MetadataEntity, MetadataError, MetadataTable, MetadataValue};
use serde_json::Value;
use std::sync::{Arc, Mutex, MutexGuard};

#[derive(Clone, Debug)]
pub struct ImplicitMetadataView {
    class: Arc<MetadataClass>,
    metadata_table: Arc<Mutex<MetadataTable>>,
    entity_id: usize,
    extras: Option<Value>,
    extensions: Option<Value>,
}

impl ImplicitMetadataView {
    /// Row `entity_id` of the table; `extras` and `extensions` come from the
    /// property table JSON.
    pub fn new(
        metadata_table: Arc<Mutex<MetadataTable>>,
        class: Arc<MetadataClass>,
        entity_id: usize,
        property_table_json: &Value,
    ) -> Result<Self, MetadataError> {
        if !(property_table_json.is_object() || property_table_json.is_array()) {
            return Err(MetadataError::Developer("Expected options.propertyTableJson to be typeof object".into()));
        }
        Ok(Self {
            class,
            metadata_table,
            entity_id,
            extras: property_table_json.get("extras").cloned(),
            extensions: property_table_json.get("extensions").cloned(),
        })
    }

    pub fn class_arc(&self) -> &Arc<MetadataClass> {
        &self.class
    }

    pub fn entity_id(&self) -> usize {
        self.entity_id
    }

    pub fn extras(&self) -> Option<&Value> {
        self.extras.as_ref()
    }

    pub fn extensions(&self) -> Option<&Value> {
        self.extensions.as_ref()
    }

    fn table(&self) -> MutexGuard<'_, MetadataTable> {
        self.metadata_table.lock().unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

impl MetadataEntity for ImplicitMetadataView {
    fn class(&self) -> &MetadataClass {
        &self.class
    }

    fn has_property(&self, property_id: &str) -> bool {
        self.table().has_property(property_id)
    }

    fn has_property_by_semantic(&self, semantic: &str) -> bool {
        self.table().has_property_by_semantic(semantic)
    }

    fn get_property_ids(&self) -> Vec<String> {
        self.table().get_property_ids()
    }

    fn get_property(&self, property_id: &str) -> Result<Option<MetadataValue>, MetadataError> {
        self.table().get_property(self.entity_id, property_id)
    }

    fn set_property(&mut self, property_id: &str, value: MetadataValue) -> Result<bool, MetadataError> {
        self.table().set_property(self.entity_id, property_id, value)
    }

    fn get_property_by_semantic(&self, semantic: &str) -> Result<Option<MetadataValue>, MetadataError> {
        self.table().get_property_by_semantic(self.entity_id, semantic)
    }

    fn set_property_by_semantic(&mut self, semantic: &str, value: MetadataValue) -> Result<bool, MetadataError> {
        self.table().set_property_by_semantic(self.entity_id, semantic, value)
    }
}
