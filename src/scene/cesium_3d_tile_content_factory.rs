//! CesiumJS `Cesium3DTileContentFactory`: maps a tile's magic field or format
//! name to its content loader and payload classification.

use crate::core::RuntimeError;
use crate::scene::Cesium3DTileContentType;
use serde_json::Value;

/// Maps a tile's magic field in its header or format identifier to content
/// classification and dispatch.
#[derive(Clone, Debug, Default)]
pub struct Cesium3DTileContentFactory;

impl Cesium3DTileContentFactory {
    /// Maps a magic 4-byte header or format string to its content type.
    pub fn find(magic_or_format: &str) -> Option<Cesium3DTileContentType> {
        match magic_or_format {
            "b3dm" => Some(Cesium3DTileContentType::Batched3DModel),
            "pnts" => Some(Cesium3DTileContentType::PointCloud),
            "i3dm" => Some(Cesium3DTileContentType::Instanced3DModel),
            "cmpt" => Some(Cesium3DTileContentType::Composite),
            "geom" => Some(Cesium3DTileContentType::Geometry),
            "vctr" => Some(Cesium3DTileContentType::Vector),
            "subt" => Some(Cesium3DTileContentType::ImplicitSubtree),
            "subtreeJson" => Some(Cesium3DTileContentType::ImplicitSubtreeJson),
            "externalTileset" => Some(Cesium3DTileContentType::ExternalTileset),
            "multipleContent" => Some(Cesium3DTileContentType::MultipleContent),
            "glb" | "glTF" => Some(Cesium3DTileContentType::GltfBinary),
            "gltf" => Some(Cesium3DTileContentType::Gltf),
            "geoJson" => Some(Cesium3DTileContentType::GeoJson),
            "voxl" => Some(Cesium3DTileContentType::VoxelBinary),
            "voxelJson" => Some(Cesium3DTileContentType::VoxelJson),
            _ => Cesium3DTileContentType::from_str_value(magic_or_format),
        }
    }

    /// Returns `true` if the specified magic string or format name is supported.
    pub fn is_supported(magic_or_format: &str) -> bool {
        Self::find(magic_or_format).is_some()
    }

    /// Returns `true` if the magic string or format corresponds to binary content.
    pub fn is_binary(magic_or_format: &str) -> bool {
        Self::find(magic_or_format)
            .map(|t| t.is_binary_format())
            .unwrap_or(false)
    }

    /// Inspects raw payload bytes to classify the tile content type.
    pub fn classify(bytes: &[u8]) -> Result<Cesium3DTileContentType, RuntimeError> {
        if bytes.is_empty() {
            return Err(RuntimeError::new("Tile content buffer is empty."));
        }

        if bytes.len() >= 4 {
            let magic = std::str::from_utf8(&bytes[..4]).unwrap_or("");
            if magic == "glTF" {
                if bytes.len() < 12 {
                    return Err(RuntimeError::new("Invalid glb content."));
                }
                return Ok(Cesium3DTileContentType::GltfBinary);
            }
            if let Some(content_type) = Self::find(magic) {
                if content_type.is_binary_format() {
                    return Ok(content_type);
                }
            }
        }

        // Attempt JSON detection
        if let Ok(json) = serde_json::from_slice::<Value>(bytes) {
            if json.get("root").is_some() {
                return Ok(Cesium3DTileContentType::ExternalTileset);
            }
            if json.get("asset").is_some() {
                return Ok(Cesium3DTileContentType::Gltf);
            }
            if json.get("tileAvailability").is_some() || json.get("subtrees").is_some() {
                return Ok(Cesium3DTileContentType::ImplicitSubtreeJson);
            }
            if json.get("type").and_then(|v| v.as_str()) == Some("FeatureCollection")
                || json.get("features").is_some()
            {
                return Ok(Cesium3DTileContentType::GeoJson);
            }
            if json.get("voxel").is_some() {
                return Ok(Cesium3DTileContentType::VoxelJson);
            }
        }

        Err(RuntimeError::new("Unrecognized 3D tile content format."))
    }
}
