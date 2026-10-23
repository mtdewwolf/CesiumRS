//! CesiumJS `findTileMetadata`: the metadata of a tile, from its 3D Tiles 1.1
//! `metadata` or its `3DTILES_metadata` extension.

use super::{has_extension, MetadataError, MetadataSchema, TileMetadata};
use crate::scene::gltf_pipeline::js;
use serde_json::Value;

/// `None` without metadata, without a schema (Cesium also warns once) or
/// when the metadata names no class. A class the schema lacks fails the
/// entity's check.
pub fn find_tile_metadata(
    schema: Option<&MetadataSchema>,
    tile_header: &Value,
) -> Result<Option<TileMetadata>, MetadataError> {
    let metadata_json = if has_extension(tile_header, "3DTILES_metadata") {
        &tile_header["extensions"]["3DTILES_metadata"]
    } else {
        match tile_header.get("metadata") {
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
            TileMetadata::new(metadata_json, class).map(Some)
        }
        None => Ok(None),
    }
}
