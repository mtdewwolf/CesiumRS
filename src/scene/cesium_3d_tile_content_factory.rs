//! CesiumJS `Cesium3DTileContentFactory`: maps a tile's magic field or format
//! identifier to its content type, glTF/Gaussian-splat dispatch target, GLB
//! payload slice validation, and structural/model content instantiation.
use super::{
    empty_3d_tile_content::Empty3DTileContent,
    preprocess_3d_tile_content::preprocess_3d_tile_content,
    tileset_3d_tile_content::Tileset3DTileContent,
    Cesium3DTileContentType, TileId,
};
use crate::core::{Resource, RuntimeError};
use serde_json::Value;

/// Upstream content handler target returned by [`Cesium3DTileContentFactory::dispatch`].
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cesium3DTileContentHandler {
    ModelB3dm,
    ModelPnts,
    ModelI3dm,
    ModelGltf,
    ModelGeoJson,
    GaussianSplatGltf,
    Composite,
    ExternalTileset,
    Geometry,
    Vector,
    ImplicitSubtree,
    ImplicitSubtreeJson,
}

/// Maps a tile's magic field in its header or format identifier to content
/// classification, validation, and dispatch matching `Cesium3DTileContentFactory.js`.
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

    /// Returns `true` if a tileset's `extensionsRequired` mandates Gaussian splatting,
    /// matching `GaussianSplat3DTileContent.tilesetRequiresGaussianSplattingExt`.
    pub fn tileset_requires_gaussian_splatting_ext(extensions_required: &[String]) -> bool {
        extensions_required.iter().any(|ext| {
            ext == "KHR_spz_gaussian_splats_compression"
                || ext == "KHR_gaussian_splatting"
                || ext == "KHR_gaussian_splatting_compression_spz_2"
        })
    }

    /// Resolves the upstream factory handler for a magic/format string and tileset extensions.
    pub fn dispatch(
        magic_or_format: &str,
        extensions_required: &[String],
    ) -> Option<Cesium3DTileContentHandler> {
        let splats = Self::tileset_requires_gaussian_splatting_ext(extensions_required);
        match magic_or_format {
            "b3dm" => Some(Cesium3DTileContentHandler::ModelB3dm),
            "pnts" => Some(Cesium3DTileContentHandler::ModelPnts),
            "i3dm" => Some(Cesium3DTileContentHandler::ModelI3dm),
            "cmpt" => Some(Cesium3DTileContentHandler::Composite),
            "externalTileset" => Some(Cesium3DTileContentHandler::ExternalTileset),
            "geom" => Some(Cesium3DTileContentHandler::Geometry),
            "vctr" => Some(Cesium3DTileContentHandler::Vector),
            "subt" => Some(Cesium3DTileContentHandler::ImplicitSubtree),
            "subtreeJson" => Some(Cesium3DTileContentHandler::ImplicitSubtreeJson),
            "glb" | "glTF" | "gltf" => Some(if splats {
                Cesium3DTileContentHandler::GaussianSplatGltf
            } else {
                Cesium3DTileContentHandler::ModelGltf
            }),
            "geoJson" => Some(Cesium3DTileContentHandler::ModelGeoJson),
            _ => None,
        }
    }

    /// Validates a GLB buffer slice starting at `byte_offset` and extracts the GLB bytes,
    /// matching `Cesium3DTileContentFactory.glb` in CesiumJS.
    pub fn glb_slice(bytes: &[u8], byte_offset: usize) -> Result<&[u8], RuntimeError> {
        if bytes.len() < 12 || byte_offset.saturating_add(12) > bytes.len() {
            return Err(RuntimeError::new("Invalid glb content"));
        }
        let byte_length = u32::from_le_bytes([
            bytes[byte_offset + 8],
            bytes[byte_offset + 9],
            bytes[byte_offset + 10],
            bytes[byte_offset + 11],
        ]) as usize;
        let end = byte_offset
            .checked_add(byte_length)
            .ok_or_else(|| RuntimeError::new("Invalid glb content"))?;
        bytes
            .get(byte_offset..end)
            .ok_or_else(|| RuntimeError::new("Invalid glb content"))
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
        preprocess_3d_tile_content(bytes).map(|content| content.content_type)
    }

    /// Creates an external tileset content instance (`Cesium3DTileContentFactory.externalTileset`).
    pub fn external_tileset(
        tile: TileId,
        resource: Resource,
        json: &Value,
        load: impl FnOnce(&Resource, &Value, TileId) -> Result<(), RuntimeError>,
    ) -> Result<Tileset3DTileContent, RuntimeError> {
        Tileset3DTileContent::from_json(tile, resource, json, load)
    }

    /// Creates an empty leaf content instance.
    pub fn empty() -> Empty3DTileContent {
        Empty3DTileContent::new()
    }
}
