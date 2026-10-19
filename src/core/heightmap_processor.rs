//! Native calculation from Cesium's createVerticesFromHeightmap worker.
//! LERC decoding uses a pure Rust codec; browser message transport is omitted.
use super::heightmap_encoding::HeightmapEncoding;
use super::heightmap_tessellator::{self, HeightmapTessellatorOptions, HeightmapVertices};
use super::runtime_error::RuntimeError;
use super::typed_array::TypedArray;

#[derive(Clone, Debug, PartialEq)]
pub struct HeightmapProcessorResult {
    pub terrain: HeightmapVertices,
    pub grid_width: usize,
    pub grid_height: usize,
}

pub fn create_vertices_from_heightmap(
    options: &HeightmapTessellatorOptions<'_>,
    encoding: HeightmapEncoding,
) -> Result<HeightmapProcessorResult, RuntimeError> {
    if encoding == HeightmapEncoding::None {
        let terrain = heightmap_tessellator::compute_vertices(options)
            .ok_or_else(|| RuntimeError::new("Invalid heightmap parameters"))?;
        return Ok(HeightmapProcessorResult { terrain, grid_width: options.width, grid_height: options.height });
    }
    let bytes = match options.heightmap {
        TypedArray::Uint8(bytes) => bytes,
        _ => return Err(RuntimeError::new("LERC requires a byte buffer")),
    };
    // Cesium returns pixels[0] but still decodes later bands and reports
    // their errors. Fewer than 11 trailing bytes are ignored by its loop.
    let mut decoded = lerc_reader::decode_first_to_f64(bytes).map_err(|e| RuntimeError::new(e.to_string()))?;
    let mut offset = decoded.info.blob_size;
    let mask =
        decoded.mask.clone().unwrap_or_else(|| vec![1; decoded.info.width as usize * decoded.info.height as usize]);
    while offset < bytes.len().saturating_sub(10) {
        let next = lerc_reader::decode_first_to_f64_with_mask(&bytes[offset..], &mask)
            .map_err(|e| RuntimeError::new(e.to_string()))?;
        offset += next.info.blob_size;
    }
    let lerc1 = bytes.starts_with(b"CntZImage ");
    if (lerc1 && decoded.info.valid_pixel_count == 0) || (!lerc1 && decoded.info.z_min == f64::MAX) {
        return Err(RuntimeError::new("Invalid tile data"));
    }
    if let Some(mask) = decoded.mask.as_ref() {
        for (i, &valid) in mask.iter().enumerate() {
            if valid == 0 {
                decoded.pixels[i] = if lerc1 { -3.402_799_938_790_148_4e38 } else { 0.0 };
            }
        }
    }
    let buffer = if lerc1 {
        TypedArray::Float32(decoded.pixels.iter().map(|&v| v as f32).collect())
    } else {
        TypedArray::Float64(decoded.pixels)
    };
    let mut parameters = options.clone();
    parameters.heightmap = &buffer;
    parameters.width = decoded.info.width as usize;
    parameters.height = decoded.info.height as usize;
    let terrain = heightmap_tessellator::compute_vertices(&parameters)
        .ok_or_else(|| RuntimeError::new("Invalid heightmap parameters"))?;
    Ok(HeightmapProcessorResult { terrain, grid_width: parameters.width, grid_height: parameters.height })
}
