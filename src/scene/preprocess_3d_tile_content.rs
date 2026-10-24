//! CesiumJS `preprocess3DTileContent`: identifies a tile content payload by
//! its magic number, or, for JSON, by the property that marks a tileset,
//! glTF, implicit subtree, GeoJSON or voxel document.

use super::Cesium3DTileContentType;
use crate::core::get_string_from_typed_array::{get_json_from_typed_array, get_magic};
use crate::core::RuntimeError;
use serde_json::Value;

/// A binary payload is the input bytes; JSON is parsed.
#[derive(Clone, Debug, PartialEq)]
pub enum TileContentPayload<'a> {
    Binary(&'a [u8]),
    Json(Value),
}

#[derive(Clone, Debug, PartialEq)]
pub struct PreprocessedTileContent<'a> {
    pub content_type: Cesium3DTileContentType,
    pub payload: TileContentPayload<'a>,
}

fn invalid() -> RuntimeError {
    RuntimeError::new("Invalid tile content.")
}

pub fn preprocess_3d_tile_content(array_buffer: &[u8]) -> Result<PreprocessedTileContent<'_>, RuntimeError> {
    let mut magic = get_magic(array_buffer, 0);
    // Binary glTF is "glb"; "glTF" is the JSON form.
    if magic == "glTF" {
        magic = "glb".into();
    }
    // For binary formats the enum value is the magic number.
    if let Some(content_type) = Cesium3DTileContentType::from_str_value(&magic).filter(|t| t.is_binary_format()) {
        return Ok(PreprocessedTileContent { content_type, payload: TileContentPayload::Binary(array_buffer) });
    }

    let json = get_json_from_typed_array(array_buffer, 0, None).map_err(|_| invalid())?;
    if json.is_null() {
        return Err(RuntimeError::new("Cannot read properties of null (reading 'root')"));
    }
    let defined = |key: &str| json.get(key).is_some_and(|v| !v.is_null());
    // A tileset also has an asset, so it is checked before glTF.
    let content_type = if defined("root") {
        Cesium3DTileContentType::ExternalTileset
    } else if defined("asset") {
        Cesium3DTileContentType::Gltf
    } else if defined("tileAvailability") {
        Cesium3DTileContentType::ImplicitSubtreeJson
    } else if defined("type") {
        Cesium3DTileContentType::GeoJson
    } else if defined("voxelTable") {
        Cesium3DTileContentType::VoxelJson
    } else {
        return Err(invalid());
    };
    Ok(PreprocessedTileContent { content_type, payload: TileContentPayload::Json(json) })
}
