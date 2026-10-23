//! CesiumJS `MetadataTable`: a table of binary 3D Metadata properties, one
//! row per feature, of one class.

use super::metadata_table_property::{ByteView, MetadataTableProperty, MetadataTypedArray};
use super::metadata_value::{developer, json_entries, JsMap, MetadataError, MetadataValue};
use super::MetadataClass;
use serde_json::Value;
use std::sync::Arc;

type Result<T> = std::result::Result<T, MetadataError>;

#[derive(Clone, Debug)]
pub struct MetadataTable {
    count: usize,
    class: Arc<MetadataClass>,
    properties: JsMap<MetadataTableProperty>,
    byte_length: usize,
}

impl MetadataTable {
    /// `count` rows of the `properties` JSON, each of which the class must
    /// define.
    pub fn new(
        count: usize,
        properties: Option<&Value>,
        class: Arc<MetadataClass>,
        buffer_views: &JsMap<ByteView<'_>>,
    ) -> Result<Self> {
        if count == 0 {
            return Err(developer("Expected options.count to be greater than 0, actual value was 0"));
        }
        let mut table_properties = JsMap::new();
        let mut byte_length = 0;
        for (id, property) in json_entries(properties) {
            let class_property = class.properties.get(&id).cloned().ok_or_else(|| {
                developer("Expected options.classProperty to be typeof object, actual typeof was undefined")
            })?;
            let property = MetadataTableProperty::new(count, property, class_property, buffer_views)?;
            byte_length += property.byte_length();
            table_properties.insert(id, property);
        }
        Ok(Self { count, class, properties: table_properties, byte_length })
    }

    pub fn count(&self) -> usize {
        self.count
    }

    pub fn class(&self) -> &Arc<MetadataClass> {
        &self.class
    }

    pub fn byte_length(&self) -> usize {
        self.byte_length
    }

    pub fn properties(&self) -> &JsMap<MetadataTableProperty> {
        &self.properties
    }

    /// Whether the table has the property, or the class gives it a default.
    pub fn has_property(&self, property_id: &str) -> bool {
        self.properties.contains_key(property_id)
            || self.class.properties.get(property_id).is_some_and(|p| p.default().is_some())
    }

    pub fn has_property_by_semantic(&self, semantic: &str) -> bool {
        self.class.properties_by_semantic.contains_key(semantic)
    }

    /// The table's properties, then the class properties with a default that
    /// it lacks.
    pub fn get_property_ids(&self) -> Vec<String> {
        let mut ids: Vec<String> = self.properties.keys().map(str::to_string).collect();
        for (id, property) in self.class.properties.iter() {
            if !self.properties.contains_key(id) && property.default().is_some() {
                ids.push(id.to_string());
            }
        }
        ids
    }

    /// Row `index` of the property, or the class default when the table
    /// lacks it.
    pub fn get_property(&mut self, index: usize, property_id: &str) -> Result<Option<MetadataValue>> {
        if let Some(property) = self.properties.get_mut(property_id) {
            return property.get(index);
        }
        let Some(class_property) = self.class.properties.get(property_id) else { return Ok(None) };
        let Some(default) = class_property.default() else { return Ok(None) };
        let value = class_property.normalize(default.clone())?;
        class_property.unpack_vector_and_matrix_types(value, false).map(Some)
    }

    /// `false` when the table lacks the property.
    pub fn set_property(&mut self, index: usize, property_id: &str, value: MetadataValue) -> Result<bool> {
        match self.properties.get_mut(property_id) {
            Some(property) => property.set(index, value).map(|()| true),
            None => Ok(false),
        }
    }

    pub fn get_property_by_semantic(&mut self, index: usize, semantic: &str) -> Result<Option<MetadataValue>> {
        match self.class.properties_by_semantic.get(semantic) {
            Some(property) => {
                let id = property.id().to_string();
                self.get_property(index, &id)
            }
            None => Ok(None),
        }
    }

    pub fn set_property_by_semantic(&mut self, index: usize, semantic: &str, value: MetadataValue) -> Result<bool> {
        match self.class.properties_by_semantic.get(semantic) {
            Some(property) => {
                let id = property.id().to_string();
                self.set_property(index, &id, value)
            }
            None => Ok(false),
        }
    }

    pub fn get_property_typed_array(&self, property_id: &str) -> Option<&MetadataTypedArray> {
        self.properties.get(property_id)?.get_typed_array()
    }

    pub fn get_property_typed_array_by_semantic(&self, semantic: &str) -> Option<&MetadataTypedArray> {
        let property = self.class.properties_by_semantic.get(semantic)?;
        self.get_property_typed_array(property.id())
    }
}
