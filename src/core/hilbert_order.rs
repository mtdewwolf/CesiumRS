//! CesiumJS `HilbertOrder` helpers for a two-dimensional Hilbert curve.
//!
//! Levels 1–32 are supported. JavaScript's signed bitwise tests and the
//! rounding when converting large BigInt indices to Number are preserved.

use super::math::to_int32;

/// Hilbert curve encoding and decoding helpers.
pub struct HilbertOrder;

impl HilbertOrder {
    /// Encodes integer coordinates at `level` into a Hilbert index.
    ///
    /// Levels 1 through 32 are supported. This follows Cesium's behavior,
    /// including its signed bitwise test at level 32.
    pub fn encode_2d(level: u32, x: u64, y: u64) -> Option<u128> {
        let n = dimension(level)?;
        let (mut x, mut y) = (x as u128, y as u128);
        if x >= n || y >= n {
            return None;
        }

        let mut index = 0u128;
        let mut scale = n / 2;
        while scale > 0 {
            let rx = if to_int32(x as f64) & to_int32(scale as f64) > 0 { 1 } else { 0 };
            let ry = if to_int32(y as f64) & to_int32(scale as f64) > 0 { 1 } else { 0 };
            index += ((3 * rx) ^ ry) * scale * scale;
            rotate(n, &mut x, &mut y, rx, ry);
            scale /= 2;
        }
        Some(index)
    }

    /// Decodes a Hilbert index at `level` into its `(x, y)` coordinates.
    pub fn decode_2d(level: u32, index: u128) -> Option<(u64, u64)> {
        let n = dimension(level)?;
        if index >= (1u128 << (2 * level)) {
            return None;
        }

        let (mut x, mut y) = (0u128, 0u128);
        let (mut scale, mut remaining) = (1u128, index);
        while scale < n {
            let rx = (to_int32((remaining / 2) as f64) & 1) as u128;
            let ry = (to_int32((remaining ^ rx) as f64) & 1) as u128;
            rotate(scale, &mut x, &mut y, rx, ry);
            x += scale * rx;
            y += scale * ry;
            remaining /= 4;
            scale *= 2;
        }
        Some((x as u64, y as u64))
    }
}

fn dimension(level: u32) -> Option<u128> {
    if !(1..=32).contains(&level) {
        return None;
    }
    Some(1u128 << level)
}

fn rotate(n: u128, x: &mut u128, y: &mut u128, rx: u128, ry: u128) {
    if ry != 0 {
        return;
    }
    if rx == 1 {
        *x = n - 1 - *x;
        *y = n - 1 - *y;
    }
    std::mem::swap(x, y);
}
