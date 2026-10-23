//! CesiumJS `findContentMetadata`: the metadata of a content, from its 3D Tiles 1.1
//! `metadata` or its `3DTILES_metadata` extension.

use super::{has_extension, ContentMetadata, MetadataError, MetadataSchema};
use crate::scene::gltf_pipeline::js;
use serde_json::Value;

/// `None` without metadata, without a schema (Cesium also warns once) or
/// when the metadata names no class. A class the schema lacks fails the
/// entity's check.
pub fn find_content_metadata(
    schema: Option<&MetadataSchema>,
    content_header: &Value,
) -> Result<Option<ContentMetadata>, MetadataError> {
    let metadata_json = if has_extension(content_header, "3DTILES_metadata") {
        &content_header["extensions"]["3DTILES_metadata"]
    } else {
        match content_header.get("metadata") {
            Some(json) if !json.is_null() => json,
            _ => return Ok(None),
        }
    };
    let Some(schema) = schema else { return Ok(None) };
    match metadata_json.get("class").filter(|v| !v.is_null()) {
        Some(class) => {
            let class = schema.classes.get(&js::key(class)).cloned().ok_or_else(|| {
                MetadataError::Developer(
                    "Expected options.class to be typeof object, actual typeof was undefined".into(),
                )
            })?;
            ContentMetadata::new(metadata_json, class).map(Some)
        }
        None => Ok(None),
    }
}
