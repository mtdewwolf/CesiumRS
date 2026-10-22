//! CesiumJS `MetadataEntity`: the interface of things that carry 3D Metadata
//! properties, and the static helpers its implementations share. Cesium's
//! abstract class is the [`MetadataEntity`] trait; the statics are this
//! module's functions.

use super::metadata_value::{developer, JsMap, MetadataError, MetadataValue};
use super::MetadataClass;

/// An entity's raw property values, in the JSON's key order.
pub type MetadataProperties = JsMap<MetadataValue>;

type Result<T> = std::result::Result<T, MetadataError>;

/// Implemented by the metadata of tilesets, tiles, contents, groups and
/// implicit subtrees.
pub trait MetadataEntity {
    fn class(&self) -> &MetadataClass;
    fn has_property(&self, property_id: &str) -> bool;
    fn has_property_by_semantic(&self, semantic: &str) -> bool;
    fn get_property_ids(&self) -> Vec<String>;
    fn get_property(&self, property_id: &str) -> Result<Option<MetadataValue>>;
    fn set_property(&mut self, property_id: &str, value: MetadataValue) -> Result<bool>;
    fn get_property_by_semantic(&self, semantic: &str) -> Result<Option<MetadataValue>>;
    fn set_property_by_semantic(&mut self, semantic: &str, value: MetadataValue) -> Result<bool>;
}

/// `defined(value)`: present and not `null`.
fn defined(value: Option<&MetadataValue>) -> bool {
    value.is_some_and(|v| !matches!(v, MetadataValue::Null))
}

/// Whether the entity has a value for the property, or the class gives it a
/// default.
pub fn has_property(property_id: &str, properties: &MetadataProperties, class: &MetadataClass) -> bool {
    if defined(properties.get(property_id)) {
        return true;
    }
    class.properties.get(property_id).is_some_and(|p| p.default().is_some())
}

/// Whether the class has a property with the semantic.
pub fn has_property_by_semantic(semantic: &str, class: &MetadataClass) -> bool {
    class.properties_by_semantic.contains_key(semantic)
}

/// The entity's defined properties, then the class properties it lacks that
/// have a default.
pub fn get_property_ids(properties: &MetadataProperties, class: &MetadataClass) -> Vec<String> {
    let mut ids: Vec<String> =
        properties.iter().filter(|(_, v)| !matches!(v, MetadataValue::Null)).map(|(k, _)| k.to_string()).collect();
    for (id, property) in class.properties.iter() {
        if !defined(properties.get(id)) && property.default().is_some() {
            ids.push(id.to_string());
        }
    }
    ids
}

/// The property's value with no-data, default, normalization and offset and
/// scale applied, and vectors and matrices unpacked. A property the class
/// does not define is a `DeveloperError`.
pub fn get_property(
    property_id: &str,
    properties: &MetadataProperties,
    class: &MetadataClass,
) -> Result<Option<MetadataValue>> {
    let class_property = class
        .properties
        .get(property_id)
        .ok_or_else(|| developer(format!("Class definition missing property {property_id}")))?;
    let value = class_property.handle_no_data(properties.get(property_id).cloned());
    let value = value.filter(|v| !matches!(v, MetadataValue::Null));
    let Some(value) = value else {
        return match class_property.default() {
            Some(default) => class_property.unpack_vector_and_matrix_types(default.clone(), true).map(Some),
            None => Ok(None),
        };
    };
    let value = class_property.normalize(value)?;
    let value = class_property.apply_value_transform(value)?;
    class_property.unpack_vector_and_matrix_types(value, true).map(Some)
}

/// Stores a value the entity already has, packed, with offset and scale and
/// normalization undone. `false` when the entity has no value for it.
pub fn set_property(
    property_id: &str,
    value: MetadataValue,
    properties: &mut MetadataProperties,
    class: &MetadataClass,
) -> Result<bool> {
    if !defined(properties.get(property_id)) {
        return Ok(false);
    }
    let value = match class.properties.get(property_id) {
        Some(class_property) => {
            let value = class_property.pack_vector_and_matrix_types(value, true)?;
            let value = class_property.unapply_value_transform(value)?;
            class_property.unnormalize(value)?
        }
        None => value,
    };
    properties.insert(property_id, value);
    Ok(true)
}

/// [`get_property`] of the class property with the semantic.
pub fn get_property_by_semantic(
    semantic: &str,
    properties: &MetadataProperties,
    class: &MetadataClass,
) -> Result<Option<MetadataValue>> {
    match class.properties_by_semantic.get(semantic) {
        Some(property) => get_property(property.id(), properties, class),
        None => Ok(None),
    }
}

/// [`set_property`] of the class property with the semantic.
pub fn set_property_by_semantic(
    semantic: &str,
    value: MetadataValue,
    properties: &mut MetadataProperties,
    class: &MetadataClass,
) -> Result<bool> {
    match class.properties_by_semantic.get(semantic) {
        Some(property) => {
            let id = property.id().to_string();
            set_property(&id, value, properties, class)
        }
        None => Ok(false),
    }
}

/// An entity's properties from its JSON `properties` object.
pub fn properties_from_json(properties: Option<&serde_json::Value>) -> MetadataProperties {
    super::metadata_value::json_entries(properties).into_iter().map(|(k, v)| (k, MetadataValue::from_json(v))).collect()
}
