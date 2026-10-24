//! CesiumJS `parseFeatureMetadataLegacy`: builds [`StructuralMetadata`] from
//! the deprecated `EXT_feature_metadata` glTF extension.
//!
//! Its feature tables and textures are dictionaries; they are stored in
//! arrays sorted by key, as `EXT_structural_metadata` would order them, and
//! keep their keys as ids. Feature texture properties are rewritten into the
//! newer form, a texture info with numeric channels. No property table
//! textures are made.

use super::gltf_pipeline::js;
use super::metadata_table_property::ByteView;
use super::metadata_value::{developer, JsMap, MetadataError};
use super::parse_structural_metadata::{class_of, count_of};
use super::property_texture_property::GltfTextures;
use super::structural_metadata::StructuralMetadataOptions;
use super::{
    MetadataId, MetadataSchema, MetadataTable, PropertyTable, PropertyTableOptions, PropertyTexture, StructuralMetadata,
};
use crate::renderer::Gl;
use serde_json::{Map, Value};
use std::sync::Arc;

pub struct ParseFeatureMetadataLegacyOptions<'a, G: Gl> {
    pub extension: &'a Value,
    /// The schema, which the caller loads.
    pub schema: Arc<MetadataSchema>,
    pub buffer_views: &'a JsMap<ByteView<'a>>,
    pub textures: &'a GltfTextures<G>,
}

pub fn parse_feature_metadata_legacy<G: Gl>(
    options: &ParseFeatureMetadataLegacyOptions<'_, G>,
) -> Result<StructuralMetadata<G>, MetadataError> {
    let extension = options.extension;
    if !(extension.is_object() || extension.is_array()) {
        return Err(developer("Expected options.extension to be typeof object"));
    }
    let schema = &options.schema;

    let mut property_tables = Vec::new();
    let feature_tables = extension.get("featureTables").filter(|t| !t.is_null());
    for feature_table_id in sorted_keys(feature_tables)? {
        let feature_table = js::get(feature_tables, &feature_table_id).expect("own key");
        let class = class_of(schema, feature_table);
        let count = count_of(feature_table)?;
        let class = class.ok_or_else(|| developer("Expected options.class to be typeof object"))?;
        let properties = feature_table.get("properties").filter(|p| !p.is_null());
        let metadata_table = MetadataTable::new(count, properties, class, options.buffer_views)?;
        property_tables.push(PropertyTable::new(PropertyTableOptions {
            id: Some(MetadataId::Key(feature_table_id)),
            count,
            metadata_table: Some(metadata_table),
            extras: feature_table.get("extras").cloned(),
            extensions: feature_table.get("extensions").cloned(),
            ..Default::default()
        }));
    }

    let mut property_textures = Vec::new();
    let feature_textures = extension.get("featureTextures").filter(|t| !t.is_null());
    for feature_texture_id in sorted_keys(feature_textures)? {
        let feature_texture = js::get(feature_textures, &feature_texture_id).expect("own key");
        let property_texture = transcode_to_property_texture(feature_texture)?;
        let class =
            class_of(schema, feature_texture).ok_or_else(|| developer("Expected options.class to be typeof object"))?;
        property_textures.push(PropertyTexture::new(
            &property_texture,
            class,
            options.textures,
            None,
            Some(MetadataId::Key(feature_texture_id)),
        )?);
    }

    Ok(StructuralMetadata::new(StructuralMetadataOptions {
        schema: schema.clone(),
        property_tables: Some(property_tables),
        property_textures,
        property_attributes: Vec::new(),
        statistics: extension.get("statistics").cloned(),
        extras: extension.get("extras").cloned(),
        extensions: extension.get("extensions").cloned(),
    }))
}

/// `Object.keys(value).sort()`: the default sort compares UTF-16 code units.
fn sorted_keys(value: Option<&Value>) -> Result<Vec<String>, MetadataError> {
    let mut keys = match value {
        Some(Value::Object(map)) => js::object_keys(map),
        Some(Value::Array(items)) => (0..items.len()).map(|i| i.to_string()).collect(),
        // Each key of a string reads a one-character string, which has no
        // class, so building its table or texture fails a debug check.
        Some(Value::String(s)) if !s.is_empty() => return Err(developer("Expected options.class to be typeof object")),
        _ => Vec::new(),
    };
    keys.sort_by(|a, b| a.encode_utf16().cmp(b.encode_utf16()));
    Ok(keys)
}

/// The feature texture as an `EXT_structural_metadata` property texture:
/// each property's `texture` info merged with numeric `channels` and the
/// property's extras and extensions, the texture info's taking precedence.
fn transcode_to_property_texture(feature_texture: &Value) -> Result<Value, MetadataError> {
    let mut properties = Map::new();
    let old_properties = feature_texture.get("properties");
    for property_id in js::for_in(old_properties.filter(|p| !p.is_null())) {
        let old_property = js::get(old_properties, &property_id).expect("own key");
        let mut property = Map::new();
        property.insert("channels".into(), reformat_channels(old_property.get("channels"))?);
        for key in ["extras", "extensions"] {
            if let Some(v) = old_property.get(key) {
                property.insert(key.into(), v.clone());
            }
        }
        properties
            .insert(property_id, Value::Object(combine(old_property.get("texture"), Some(&Value::Object(property)))));
    }
    let mut property_texture = Map::new();
    if let Some(class) = feature_texture.get("class") {
        property_texture.insert("class".into(), class.clone());
    }
    property_texture.insert("properties".into(), Value::Object(properties));
    Ok(Value::Object(property_texture))
}

/// `"rgb"` as `[0, 1, 2]`: each channel's index in `"rgba"`, or -1.
fn reformat_channels(channels: Option<&Value>) -> Result<Value, MetadataError> {
    let index = |s: &str| "rgba".find(s).map_or(-1, |i| i as i64);
    Ok(match channels {
        None | Some(Value::Null) => {
            let what = if channels.is_some() { "null" } else { "undefined" };
            return Err(MetadataError::Type(format!("Cannot read properties of {what} (reading 'length')")));
        }
        Some(Value::String(s)) => {
            let units: Vec<u16> = s.encode_utf16().collect();
            units.iter().map(|&u| Value::from(index(&String::from_utf16_lossy(&[u])))).collect()
        }
        // "rgba".indexOf(String(element)).
        Some(Value::Array(items)) => {
            items.iter().map(|v| Value::from(index(&if v.is_null() { "null".into() } else { js::key(v) }))).collect()
        }
        // No length: `new Array(undefined)` is `[undefined]`.
        Some(_) => Value::Array(vec![Value::Null]),
    })
}

/// CesiumJS `combine(object1, object2, true)`: `object1`'s properties win,
/// and objects both define are combined recursively.
fn combine(object1: Option<&Value>, object2: Option<&Value>) -> Map<String, Value> {
    let is_object = |v: &Value| matches!(v, Value::Object(_) | Value::Array(_) | Value::Null);
    let object1 = object1.filter(|v| !v.is_null());
    let object2 = object2.filter(|v| !v.is_null());
    let mut result = Map::new();
    for property in js::for_in(object1) {
        let value1 = js::get(object1, &property).expect("own key");
        let value2 = js::get(object2, &property);
        let merged = match value2 {
            Some(value2) if is_object(value1) && is_object(value2) => {
                Value::Object(combine(Some(value1), Some(value2)))
            }
            _ => value1.clone(),
        };
        result.insert(property, merged);
    }
    for property in js::for_in(object2) {
        if !result.contains_key(&property) {
            result.insert(property.clone(), js::get(object2, &property).expect("own key").clone());
        }
    }
    result
}
