//! Gaussian splat workers: CesiumJS `gaussianSplatSorter` and
//! `gaussianSplatTextureGenerator`, with the algorithms of `@cesium/wasm-splats`
//! 0.1.0-alpha.2 (CesiumGS cesium-wasm-utils, Apache-2.0) run as native Rust.
//!
//! The workers' WebAssembly initialization message has no counterpart: there is
//! no module to compile. The calculations keep the Wasm release build's
//! semantics: saturating float-to-integer casts, wrapping integer addition and
//! little-endian texel bytes. Inputs too short for `count` return errors where
//! the Wasm module would trap.

use crate::core::RuntimeError;

/// Sorts splat indices by view depth with a four-pass, eight-bit radix sort.
/// Depth is the view-space z of each position, quantized to 1/4096.
pub fn radix_sort_gaussians_indexes(
    positions: &[f32],
    model_view: &[f32],
    count: usize,
) -> Result<Vec<u32>, RuntimeError> {
    if count.checked_mul(3) != Some(positions.len()) {
        return Err(RuntimeError::new("Invalid positions length"));
    }
    if model_view.len() != 16 {
        return Err(RuntimeError::new("Invalid model_view length"));
    }
    let (mv2, mv6, mv10) = (model_view[2], model_view[6], model_view[10]);
    let mut depths = vec![0i32; count];
    let mut min_depth = f32::INFINITY;
    for (i, depth) in depths.iter_mut().enumerate() {
        let z = positions[i * 3] * mv2 + positions[i * 3 + 1] * mv6 + positions[i * 3 + 2] * mv10;
        *depth = (z * 4096.0) as i32;
        min_depth = min_depth.min(*depth as f32);
    }
    let offset = (-min_depth) as i32;
    for depth in &mut depths {
        *depth = depth.wrapping_add(offset);
    }
    let mut indices: Vec<u32> = (0..count as u32).collect();
    let mut sorted_depths = vec![0i32; count];
    let mut sorted_indices = vec![0u32; count];
    for shift in (0..32).step_by(8) {
        let mut counts = [0u32; 256];
        for &depth in &depths {
            counts[((depth >> shift) & 0xff) as usize] += 1;
        }
        let mut total = 0;
        for c in &mut counts {
            let current = *c;
            *c = total;
            total += current;
        }
        for i in 0..count {
            let byte = ((depths[i] >> shift) & 0xff) as usize;
            let position = counts[byte] as usize;
            counts[byte] += 1;
            sorted_depths[position] = depths[i];
            sorted_indices[position] = indices[i];
        }
        depths.copy_from_slice(&sorted_depths);
        indices.copy_from_slice(&sorted_indices);
    }
    Ok(indices)
}

/// The ILM half-float conversion `wasm-splats` uses, returned as raw bits.
fn float_to_half(f: f32) -> u16 {
    let bits = f.to_bits() as i32;
    let sign = (bits >> 16) & 0x8000;
    let mut exponent = ((bits >> 23) & 0xff) - (127 - 15);
    let mut fraction = bits & 0x007f_ffff;
    if exponent <= 0 {
        if exponent < -10 {
            return sign as u16;
        }
        fraction |= 0x0080_0000;
        let t = 14 - exponent;
        let a = (1 << (t - 1)) - 1;
        let b = (fraction >> t) & 1;
        fraction = (fraction + a + b) >> t;
        return (sign | fraction) as u16;
    } else if exponent == 0xff - (127 - 15) {
        if fraction == 0 {
            return (sign | 0x7c00) as u16;
        }
        fraction >>= 13;
        return (sign | 0x7c00 | fraction | (fraction == 0) as i32) as u16;
    }
    fraction = fraction + 0x0fff + ((fraction >> 13) & 1);
    if fraction & 0x0080_0000 != 0 {
        fraction = 0;
        exponent += 1;
    }
    if exponent > 30 {
        return (sign | 0x7c00) as u16;
    }
    (sign | (exponent << 10) | (fraction >> 13)) as u16
}

/// An RGBA32UI splat texture: two texels per splat, 2048 texels wide.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SplatTextureData {
    pub data: Vec<u32>,
    pub width: u32,
    pub height: u32,
}

/// Packs each splat's position (three Float32 bit patterns), its 3D covariance
/// `4 Σ` as six half floats, and its RGBA8 color into eight `u32` values.
pub fn generate_splat_texture(
    positions: &[f32],
    scales: &[f32],
    rotations: &[f32],
    colors: &[u8],
    count: usize,
) -> Result<SplatTextureData, RuntimeError> {
    let short = |len: usize, per: usize| count.checked_mul(per).is_none_or(|n| len < n);
    if short(positions.len(), 3) || short(scales.len(), 3) || short(rotations.len(), 4) || short(colors.len(), 4) {
        return Err(RuntimeError::new("Splat attributes are shorter than the splat count."));
    }
    let width = 2048u32;
    let height = ((2 * count) as f32 / width as f32).ceil() as u32;
    let mut data = vec![0u32; width as usize * height as usize * 4];
    let half_pair = |a: f32, b: f32| float_to_half(4.0 * a) as u32 | (float_to_half(4.0 * b) as u32) << 16;
    for i in 0..count {
        data[8 * i] = positions[3 * i].to_bits();
        data[8 * i + 1] = positions[3 * i + 1].to_bits();
        data[8 * i + 2] = positions[3 * i + 2].to_bits();
        data[8 * i + 7] = u32::from_le_bytes(colors[4 * i..4 * i + 4].try_into().unwrap());
        let (x, y, z, r) = (rotations[4 * i], rotations[4 * i + 1], rotations[4 * i + 2], rotations[4 * i + 3]);
        let rotation = [
            1.0 - 2.0 * (y * y + z * z),
            2.0 * (x * y + r * z),
            2.0 * (x * z - r * y),
            2.0 * (x * y - r * z),
            1.0 - 2.0 * (x * x + z * z),
            2.0 * (y * z + r * x),
            2.0 * (x * z + r * y),
            2.0 * (y * z - r * x),
            1.0 - 2.0 * (x * x + y * y),
        ];
        let (s0, s1, s2) = (scales[3 * i], scales[3 * i + 1], scales[3 * i + 2]);
        let m = [
            rotation[0] * s0,
            rotation[1] * s0,
            rotation[2] * s0,
            rotation[3] * s1,
            rotation[4] * s1,
            rotation[5] * s1,
            rotation[6] * s2,
            rotation[7] * s2,
            rotation[8] * s2,
        ];
        let sigma = [
            m[0] * m[0] + m[3] * m[3] + m[6] * m[6],
            m[0] * m[1] + m[3] * m[4] + m[6] * m[7],
            m[0] * m[2] + m[3] * m[5] + m[6] * m[8],
            m[1] * m[1] + m[4] * m[4] + m[7] * m[7],
            m[1] * m[2] + m[4] * m[5] + m[7] * m[8],
            m[2] * m[2] + m[5] * m[5] + m[8] * m[8],
        ];
        data[8 * i + 4] = half_pair(sigma[0], sigma[1]);
        data[8 * i + 5] = half_pair(sigma[2], sigma[3]);
        data[8 * i + 6] = half_pair(sigma[4], sigma[5]);
    }
    Ok(SplatTextureData { data, width, height })
}

/// CesiumJS `gaussianSplatSorter`: only the `"Index"` sort type produces a result.
pub fn gaussian_splat_sorter(
    positions: &[f32],
    model_view: &[f32],
    count: usize,
    sort_type: &str,
) -> Result<Option<Vec<u32>>, RuntimeError> {
    if sort_type != "Index" {
        return Ok(None);
    }
    radix_sort_gaussians_indexes(positions, model_view, count).map(Some)
}

/// CesiumJS `gaussianSplatTextureGenerator`.
pub fn gaussian_splat_texture_generator(
    positions: &[f32],
    scales: &[f32],
    rotations: &[f32],
    colors: &[u8],
    count: usize,
) -> Result<SplatTextureData, RuntimeError> {
    generate_splat_texture(positions, scales, rotations, colors, count)
}
