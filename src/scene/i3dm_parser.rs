//! CesiumJS `I3dmParser`: splits an Instanced 3D Model tile into its feature
//! table, batch table and glTF (embedded glb, or a UTF-8 URI when
//! `gltfFormat` is 0). Byte ranges follow the same JavaScript typed-array
//! rules as [`super::b3dm_parser`].

use super::b3dm_parser::{
    batch_table, embedded_glb, read_u32, section_json, uint8_view, TileFormatWarning, SIZE_OF_UINT32,
};
use super::gltf_pipeline::BufferSource;
use crate::core::RuntimeError;
use serde_json::Value;
use std::sync::Arc;

/// The parts of an i3dm tile.
#[derive(Clone, Debug)]
pub struct ParsedI3dm {
    /// 0: `gltf` holds a URI; 1: `gltf` holds a glb.
    pub gltf_format: u32,
    pub feature_table_json: Value,
    pub feature_table_binary: BufferSource,
    pub batch_table_json: Option<Value>,
    /// A copy, so the tile buffer can be released.
    pub batch_table_binary: Option<Vec<u8>>,
    /// A view into the tile buffer when 4-byte aligned, otherwise a copy.
    pub gltf: BufferSource,
    pub warnings: Vec<TileFormatWarning>,
}

/// `I3dmParser.parse(arrayBuffer, byteOffset)`.
pub fn parse(array_buffer: &Arc<Vec<u8>>, byte_offset: usize) -> Result<ParsedI3dm, RuntimeError> {
    let buffer = array_buffer.as_slice();
    let byte_start = byte_offset;
    let mut offset = byte_start + SIZE_OF_UINT32; // magic
    let version = read_u32(buffer, offset)?;
    if version != 1 {
        return Err(RuntimeError::new(format!(
            "Only Instanced 3D Model version 1 is supported. Version {version} is not."
        )));
    }
    offset += SIZE_OF_UINT32;
    let byte_length = read_u32(buffer, offset)?;
    offset += SIZE_OF_UINT32;
    let feature_table_json_length = read_u32(buffer, offset)?;
    if feature_table_json_length == 0 {
        return Err(RuntimeError::new("featureTableJsonByteLength is zero, the feature table must be defined."));
    }
    offset += SIZE_OF_UINT32;
    let feature_table_binary_length = read_u32(buffer, offset)?;
    offset += SIZE_OF_UINT32;
    let batch_table_json_length = read_u32(buffer, offset)?;
    offset += SIZE_OF_UINT32;
    let batch_table_binary_length = read_u32(buffer, offset)?;
    offset += SIZE_OF_UINT32;
    let gltf_format = read_u32(buffer, offset)?;
    if gltf_format != 1 && gltf_format != 0 {
        return Err(RuntimeError::new(format!(
            "Only glTF format 0 (uri) or 1 (embedded) are supported. Format {gltf_format} is not."
        )));
    }
    offset += SIZE_OF_UINT32;

    let feature_table_json = section_json(buffer, offset, feature_table_json_length)?;
    offset += feature_table_json_length as usize;
    let feature_table_binary = uint8_view(array_buffer, offset, feature_table_binary_length as i64)?;
    offset += feature_table_binary_length as usize;
    let (batch_table_json, batch_table_binary) =
        batch_table(array_buffer, &mut offset, batch_table_json_length, batch_table_binary_length)?;
    let mut warnings = Vec::new();
    let gltf = embedded_glb(
        array_buffer,
        byte_start,
        byte_length,
        offset,
        TileFormatWarning::I3dmGlbUnaligned,
        &mut warnings,
    )?;
    Ok(ParsedI3dm {
        gltf_format,
        feature_table_json,
        feature_table_binary,
        batch_table_json,
        batch_table_binary,
        gltf,
        warnings,
    })
}
