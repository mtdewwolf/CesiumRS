//! Atlas sizing for Cesium's voxel megatexture.

use crate::core::RuntimeError;

/// Compute the texture dimensions for positive tile dimensions and sample size.
/// `maximum_size` is the context's maximum 3D texture dimension.
pub fn get_3d_texture_dimension(
    dimensions: [u32; 3],
    bytes_per_sample: u32,
    available_bytes: u64,
    tile_count: Option<u64>,
    maximum_size: u32,
) -> Result<[u32; 3], RuntimeError> {
    if dimensions.contains(&0) || bytes_per_sample == 0 {
        return Err(RuntimeError::new("Tile dimensions and bytes per sample must be positive."));
    }
    if dimensions.iter().any(|&dimension| dimension > maximum_size) {
        return Err(RuntimeError::new(
            "The GL context does not support a 3D texture large enough to contain a tile with the given dimensions.",
        ));
    }
    let tile_bytes = dimensions.iter().try_fold(u64::from(bytes_per_sample), |v, &d| v.checked_mul(u64::from(d)));
    let max_tiles = tile_bytes.map_or(0, |size| available_bytes / size);
    if max_tiles == 0 {
        return Err(RuntimeError::new(
            "Not enough texture memory available to create a megatexture with the given tile dimensions.",
        ));
    }
    let target = tile_count.unwrap_or(max_tiles).min(max_tiles);
    let mut axes = [0, 1, 2];
    axes.sort_by_key(|&axis| std::cmp::Reverse(dimensions[axis]));
    let mut counts = axes.map(|axis| u64::from(maximum_size / dimensions[axis]));
    for i in 0..3 {
        let volume = counts.iter().product::<u64>();
        if volume < target {
            break;
        }
        let slice = counts[(i + 1) % 3] * counts[(i + 2) % 3];
        if slice != 0 {
            counts[i] -= (volume - target) / slice;
        }
    }
    if counts.iter().product::<u64>() > max_tiles {
        counts[2] = max_tiles / (counts[0] * counts[1]);
    }
    let mut result = [0; 3];
    for i in 0..3 {
        result[axes[i]] = counts[i] as u32 * dimensions[axes[i]];
    }
    Ok(result)
}
