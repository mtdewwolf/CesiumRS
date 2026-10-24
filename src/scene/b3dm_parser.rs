//! CesiumJS `B3dmParser`: splits a Batched 3D Model tile into its feature
//! table, batch table and embedded glb.
//!
//! Both legacy headers are recognized, as upstream does, by a batch table
//! length of at least 0x22000000 (the first byte of the JSON or glb that a
//! legacy header puts there). Upstream's `deprecationWarning` calls are
//! returned as [`TileFormatWarning`]s. Byte ranges follow JavaScript's typed
//! array rules over the whole tile buffer: reading the header past the end is
//! the DataView `RangeError`; a section past the end is the Uint8Array
//! `RangeError`; an unaligned glb is copied with `subarray`, which clamps.

use super::gltf_pipeline::BufferSource;
use crate::core::get_string_from_typed_array::get_json_from_typed_array;
use crate::core::RuntimeError;
use serde_json::{json, Value};
use std::sync::Arc;

/// A `deprecationWarning` upstream would log once per id.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TileFormatWarning {
    /// `b3dm-legacy-header`: `[batchLength] [batchTableByteLength]`.
    B3dmLegacyHeader1,
    /// `b3dm-legacy-header`: `[batchTableJsonByteLength] [batchTableBinaryByteLength] [batchLength]`.
    B3dmLegacyHeader2,
    /// `b3dm-glb-unaligned`.
    B3dmGlbUnaligned,
    /// `i3dm-glb-unaligned`.
    I3dmGlbUnaligned,
}

impl TileFormatWarning {
    /// Upstream's warning id.
    pub fn id(self) -> &'static str {
        match self {
            Self::B3dmLegacyHeader1 | Self::B3dmLegacyHeader2 => "b3dm-legacy-header",
            Self::B3dmGlbUnaligned => "b3dm-glb-unaligned",
            Self::I3dmGlbUnaligned => "i3dm-glb-unaligned",
        }
    }
}

/// The parts of a b3dm tile.
#[derive(Clone, Debug)]
pub struct ParsedB3dm {
    /// Set only by the legacy headers.
    pub batch_length: Option<u32>,
    pub feature_table_json: Value,
    pub feature_table_binary: BufferSource,
    pub batch_table_json: Option<Value>,
    /// A copy, so the tile buffer can be released.
    pub batch_table_binary: Option<Vec<u8>>,
    /// A view into the tile buffer when 4-byte aligned, otherwise a copy.
    pub gltf: BufferSource,
    pub warnings: Vec<TileFormatWarning>,
}

pub(crate) const SIZE_OF_UINT32: usize = 4;

/// `DataView.getUint32(offset, true)`.
pub(crate) fn read_u32(buffer: &[u8], offset: usize) -> Result<u32, RuntimeError> {
    offset
        .checked_add(SIZE_OF_UINT32)
        .and_then(|end| buffer.get(offset..end))
        .map(|b| u32::from_le_bytes(b.try_into().unwrap()))
        .ok_or_else(|| RuntimeError::new("RangeError: Offset is outside the bounds of the DataView"))
}

/// `new Uint8Array(arrayBuffer, offset, length)`.
pub(crate) fn uint8_view(buffer: &Arc<Vec<u8>>, offset: usize, length: i64) -> Result<BufferSource, RuntimeError> {
    if offset > buffer.len() {
        return Err(RuntimeError::new(format!(
            "RangeError: Start offset {offset} is outside the bounds of the buffer"
        )));
    }
    if length < 0 || offset as i64 + length > buffer.len() as i64 {
        return Err(RuntimeError::new(format!("RangeError: Invalid typed array length: {length}")));
    }
    Ok(BufferSource::view(buffer.clone(), offset, length as usize))
}

/// `new Uint8Array(uint8Array.subarray(start, end))`: clamped, copied.
pub(crate) fn copy_subarray(buffer: &[u8], start: usize, end: i64) -> Vec<u8> {
    let start = start.min(buffer.len());
    let end = end.clamp(start as i64, buffer.len() as i64) as usize;
    buffer[start..end].to_vec()
}

/// `getJsonFromTypedArray`, with JavaScript's SyntaxError for malformed JSON.
pub(crate) fn section_json(buffer: &[u8], offset: usize, length: u32) -> Result<Value, RuntimeError> {
    get_json_from_typed_array(buffer, offset, Some(length as usize))
        .map_err(|e| RuntimeError::new(format!("SyntaxError: {}", e.message)))
}

/// The batch table JSON and its binary copy, when the JSON is non-empty in length.
pub(crate) fn batch_table(
    buffer: &Arc<Vec<u8>>,
    offset: &mut usize,
    json_length: u32,
    binary_length: u32,
) -> Result<(Option<Value>, Option<Vec<u8>>), RuntimeError> {
    if json_length == 0 {
        return Ok((None, None));
    }
    let json = section_json(buffer, *offset, json_length)?;
    *offset += json_length as usize;
    let mut binary = None;
    if binary_length > 0 {
        binary = Some(uint8_view(buffer, *offset, binary_length as i64)?.bytes().to_vec());
        *offset += binary_length as usize;
    }
    Ok((Some(json), binary))
}

/// The embedded glb from `offset` to the end of the tile's `byteLength`.
pub(crate) fn embedded_glb(
    buffer: &Arc<Vec<u8>>,
    byte_start: usize,
    byte_length: u32,
    offset: usize,
    unaligned: TileFormatWarning,
    warnings: &mut Vec<TileFormatWarning>,
) -> Result<BufferSource, RuntimeError> {
    let gltf_byte_length = byte_start as i64 + byte_length as i64 - offset as i64;
    if gltf_byte_length == 0 {
        return Err(RuntimeError::new("glTF byte length must be greater than 0."));
    }
    if offset.is_multiple_of(4) {
        uint8_view(buffer, offset, gltf_byte_length)
    } else {
        warnings.push(unaligned);
        Ok(BufferSource::new(copy_subarray(buffer, offset, offset as i64 + gltf_byte_length)))
    }
}

/// `B3dmParser.parse(arrayBuffer, byteOffset)`.
pub fn parse(array_buffer: &Arc<Vec<u8>>, byte_offset: usize) -> Result<ParsedB3dm, RuntimeError> {
    let buffer = array_buffer.as_slice();
    let byte_start = byte_offset;
    let mut offset = byte_start + SIZE_OF_UINT32; // magic
    let version = read_u32(buffer, offset)?;
    if version != 1 {
        return Err(RuntimeError::new(format!(
            "Only Batched 3D Model version 1 is supported.  Version {version} is not."
        )));
    }
    offset += SIZE_OF_UINT32;
    let byte_length = read_u32(buffer, offset)?;
    offset += SIZE_OF_UINT32;
    let mut feature_table_json_length = read_u32(buffer, offset)?;
    offset += SIZE_OF_UINT32;
    let mut feature_table_binary_length = read_u32(buffer, offset)?;
    offset += SIZE_OF_UINT32;
    let mut batch_table_json_length = read_u32(buffer, offset)?;
    offset += SIZE_OF_UINT32;
    let mut batch_table_binary_length = read_u32(buffer, offset)?;
    offset += SIZE_OF_UINT32;

    let mut warnings = Vec::new();
    let mut batch_length = None;
    if batch_table_json_length >= 570_425_344 {
        // Legacy header #1: [batchLength] [batchTableByteLength].
        offset -= SIZE_OF_UINT32 * 2;
        batch_length = Some(feature_table_json_length);
        batch_table_json_length = feature_table_binary_length;
        batch_table_binary_length = 0;
        feature_table_json_length = 0;
        feature_table_binary_length = 0;
        warnings.push(TileFormatWarning::B3dmLegacyHeader1);
    } else if batch_table_binary_length >= 570_425_344 {
        // Legacy header #2: [batchTableJsonByteLength] [batchTableBinaryByteLength] [batchLength].
        offset -= SIZE_OF_UINT32;
        batch_length = Some(batch_table_json_length);
        batch_table_json_length = feature_table_json_length;
        batch_table_binary_length = feature_table_binary_length;
        feature_table_json_length = 0;
        feature_table_binary_length = 0;
        warnings.push(TileFormatWarning::B3dmLegacyHeader2);
    }

    let feature_table_json = if feature_table_json_length == 0 {
        json!({ "BATCH_LENGTH": batch_length.unwrap_or(0) })
    } else {
        let json = section_json(buffer, offset, feature_table_json_length)?;
        offset += feature_table_json_length as usize;
        json
    };
    let feature_table_binary = uint8_view(array_buffer, offset, feature_table_binary_length as i64)?;
    offset += feature_table_binary_length as usize;
    let (batch_table_json, batch_table_binary) =
        batch_table(array_buffer, &mut offset, batch_table_json_length, batch_table_binary_length)?;
    let gltf = embedded_glb(
        array_buffer,
        byte_start,
        byte_length,
        offset,
        TileFormatWarning::B3dmGlbUnaligned,
        &mut warnings,
    )?;
    Ok(ParsedB3dm {
        batch_length,
        feature_table_json,
        feature_table_binary,
        batch_table_json,
        batch_table_binary,
        gltf,
        warnings,
    })
}
