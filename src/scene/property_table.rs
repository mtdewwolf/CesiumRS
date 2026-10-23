//! CesiumJS `PropertyTable`: the features of a model or tile with their
//! properties, from a binary [`MetadataTable`], a 3D Tiles 1.0
//! [`JsonMetadataTable`] and a [`BatchTableHierarchy`], asked in that order.
//!
//! The GPU texture Cesium packs a table into is not ported.

use super::metadata_table_property::MetadataTypedArray;
use super::metadata_value::{developer, JsMap, MetadataError, MetadataValue};
use super::{BatchTableHierarchy, JsonMetadataTable, MetadataClass, MetadataTable, MetadataTableProperty};
use serde_json::Value;
use std::sync::Arc;

type Result<T> = std::result::Result<T, MetadataError>;

#[derive(Clone, Debug, Default)]
pub struct PropertyTableOptions {
    pub name: Option<String>,
    pub id: Option<usize>,
    pub count: usize,
    pub extras: Option<Value>,
    pub extensions: Option<Value>,
    pub metadata_table: Option<MetadataTable>,
    pub json_metadata_table: Option<JsonMetadataTable>,
    pub batch_table_hierarchy: Option<BatchTableHierarchy>,
}

#[derive(Clone, Debug)]
pub struct PropertyTable {
    name: Option<String>,
    id: Option<usize>,
    count: usize,
    extras: Option<Value>,
    extensions: Option<Value>,
    metadata_table: Option<MetadataTable>,
    json_metadata_table: Option<JsonMetadataTable>,
    batch_table_hierarchy: Option<BatchTableHierarchy>,
}

/// `defined(value)`.
fn defined(value: &Option<MetadataValue>) -> bool {
    value.as_ref().is_some_and(|v| !matches!(v, MetadataValue::Null))
}

impl PropertyTable {
    pub fn new(options: PropertyTableOptions) -> Self {
        Self {
            name: options.name,
            id: options.id,
            count: options.count,
            extras: options.extras,
            extensions: options.extensions,
            metadata_table: options.metadata_table,
            json_metadata_table: options.json_metadata_table,
            batch_table_hierarchy: options.batch_table_hierarchy,
        }
    }

    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    pub fn id(&self) -> Option<usize> {
        self.id
    }

    pub fn count(&self) -> usize {
        self.count
    }

    /// The binary table's class.
    pub fn class(&self) -> Option<&Arc<MetadataClass>> {
        self.metadata_table.as_ref().map(MetadataTable::class)
    }

    /// The binary table's properties.
    pub fn properties(&self) -> Option<&JsMap<MetadataTableProperty>> {
        self.metadata_table.as_ref().map(MetadataTable::properties)
    }

    pub fn extras(&self) -> Option<&Value> {
        self.extras.as_ref()
    }

    pub fn extensions(&self) -> Option<&Value> {
        self.extensions.as_ref()
    }

    /// The bytes of the binary table and the hierarchy.
    pub fn byte_length(&self) -> usize {
        self.metadata_table.as_ref().map_or(0, MetadataTable::byte_length)
            + self.batch_table_hierarchy.as_ref().map_or(0, BatchTableHierarchy::byte_length)
    }

    /// Whether feature `index` has the property.
    pub fn has_property(&self, index: usize, property_id: &str) -> Result<bool> {
        if self.metadata_table.as_ref().is_some_and(|t| t.has_property(property_id)) {
            return Ok(true);
        }
        if let Some(hierarchy) = &self.batch_table_hierarchy {
            if hierarchy.has_property(index, property_id)? {
                return Ok(true);
            }
        }
        Ok(self.json_metadata_table.as_ref().is_some_and(|t| t.has_property(property_id)))
    }

    pub fn has_property_by_semantic(&self, _index: usize, semantic: &str) -> bool {
        self.property_exists_by_semantic(semantic)
    }

    /// Whether any feature has the property.
    pub fn property_exists(&self, property_id: &str) -> bool {
        self.metadata_table.as_ref().is_some_and(|t| t.has_property(property_id))
            || self.batch_table_hierarchy.as_ref().is_some_and(|h| h.property_exists(property_id))
            || self.json_metadata_table.as_ref().is_some_and(|t| t.has_property(property_id))
    }

    pub fn property_exists_by_semantic(&self, semantic: &str) -> bool {
        self.metadata_table.as_ref().is_some_and(|t| t.has_property_by_semantic(semantic))
    }

    /// The properties of each source in turn, repeats included.
    pub fn get_property_ids(&self, index: usize) -> Result<Vec<String>> {
        let mut ids = Vec::new();
        if let Some(table) = &self.metadata_table {
            ids.extend(table.get_property_ids());
        }
        if let Some(hierarchy) = &self.batch_table_hierarchy {
            ids.extend(hierarchy.get_property_ids(index)?);
        }
        if let Some(table) = &self.json_metadata_table {
            ids.extend(table.get_property_ids());
        }
        Ok(ids)
    }

    /// The first defined value of the property among the sources.
    pub fn get_property(&mut self, index: usize, property_id: &str) -> Result<Option<MetadataValue>> {
        if let Some(table) = &mut self.metadata_table {
            let result = table.get_property(index, property_id)?;
            if defined(&result) {
                return Ok(result);
            }
        }
        if let Some(hierarchy) = &self.batch_table_hierarchy {
            let result = hierarchy.get_property(index, property_id)?;
            if defined(&result) {
                return Ok(result);
            }
        }
        if let Some(table) = &self.json_metadata_table {
            let result = table.get_property(index, property_id)?;
            if defined(&result) {
                return Ok(result);
            }
        }
        Ok(None)
    }

    /// Stores the value in the first source that has the property, else in
    /// the JSON table, which is made if there is none.
    pub fn set_property(&mut self, index: usize, property_id: &str, value: MetadataValue) -> Result<()> {
        if let Some(table) = &mut self.metadata_table {
            if table.set_property(index, property_id, value.clone())? {
                return Ok(());
            }
        }
        if let Some(hierarchy) = &mut self.batch_table_hierarchy {
            if hierarchy.set_property(index, property_id, value.clone())? {
                return Ok(());
            }
        }
        if self.json_metadata_table.is_none() {
            self.json_metadata_table = Some(JsonMetadataTable::new(self.count, &Value::Object(Default::default()))?);
        }
        self.json_metadata_table.as_mut().expect("just made").set_property(index, property_id, value)
    }

    pub fn get_property_by_semantic(&mut self, index: usize, semantic: &str) -> Result<Option<MetadataValue>> {
        match &mut self.metadata_table {
            Some(table) => table.get_property_by_semantic(index, semantic),
            None => Ok(None),
        }
    }

    pub fn set_property_by_semantic(&mut self, index: usize, semantic: &str, value: MetadataValue) -> Result<bool> {
        match &mut self.metadata_table {
            Some(table) => table.set_property_by_semantic(index, semantic, value),
            None => Ok(false),
        }
    }

    pub fn get_property_typed_array(&self, property_id: &str) -> Option<&MetadataTypedArray> {
        self.metadata_table.as_ref()?.get_property_typed_array(property_id)
    }

    pub fn get_property_typed_array_by_semantic(&self, semantic: &str) -> Option<&MetadataTypedArray> {
        self.metadata_table.as_ref()?.get_property_typed_array_by_semantic(semantic)
    }

    fn check_feature_id(&self, feature_id: usize) -> Result<()> {
        if feature_id >= self.count {
            // Cesium's message subtracts a string, so it reads "NaN".
            return Err(developer("NaN"));
        }
        Ok(())
    }

    /// Whether the feature or a hierarchy ancestor is of the class.
    pub fn is_class(&self, feature_id: usize, class_name: &str) -> Result<bool> {
        self.check_feature_id(feature_id)?;
        match &self.batch_table_hierarchy {
            Some(hierarchy) => hierarchy.is_class(feature_id, class_name),
            None => Ok(false),
        }
    }

    pub fn is_exact_class(&self, feature_id: usize, class_name: &str) -> Result<bool> {
        self.check_feature_id(feature_id)?;
        Ok(self.get_exact_class_name(feature_id)?.and_then(Value::as_str) == Some(class_name))
    }

    /// The `name` of the feature's hierarchy class.
    pub fn get_exact_class_name(&self, feature_id: usize) -> Result<Option<&Value>> {
        self.check_feature_id(feature_id)?;
        match &self.batch_table_hierarchy {
            Some(hierarchy) => hierarchy.get_class_name(feature_id),
            None => Ok(None),
        }
    }
}
