//! CesiumJS HilbertOrder, including high-level Number rounding, signed
//! bitwise tests and arbitrary-precision decode indices. Number-based encode
//! supports finite dimensions through level 1023; decode supports levels
//! through 511, where Cesium's 4^level BigInt range check remains finite.

use super::math::to_int32;
use num_bigint::BigUint;
use num_traits::{One, ToPrimitive};

pub struct HilbertOrder;

impl HilbertOrder {
    /// Integer-coordinate convenience wrapper. Coordinates undergo the same
    /// conversion to Number as integers supplied to CesiumJS.
    pub fn encode_2d(level: u32, x: u64, y: u64) -> Option<u128> {
        Self::encode_2d_number(level, x as f64, y as f64)
    }

    pub fn encode_2d_number(level: u32, mut x: f64, mut y: f64) -> Option<u128> {
        let n = dimension(level, 1023)?;
        if x < 0.0 || x >= n || y < 0.0 || y >= n {
            return None;
        }
        let mut index = 0u128;
        let mut scale = n / 2.0;
        // Fractional scales in Cesium's loop have zero bitwise masks and
        // cannot contribute to the index. Stop before those redundant swaps.
        while scale >= 1.0 {
            let rx = u32::from(to_int32(x) & to_int32(scale) > 0);
            let ry = u32::from(to_int32(y) & to_int32(scale) > 0);
            // At scales >=2^31 the signed mask never tests positive. Thus
            // every nonzero contribution fits u128, even at level 1023.
            index += (((3 * rx) ^ ry) as f64 * scale * scale) as u128;
            rotate(n, &mut x, &mut y, rx, ry);
            scale /= 2.0;
        }
        Some(index)
    }

    /// Compatibility helper for exact integer coordinates through level 32.
    /// Use decode_2d_big or decode_2d_decimal for higher levels.
    pub fn decode_2d(level: u32, index: u128) -> Option<(u64, u64)> {
        if level > 32 {
            return None;
        }
        let (x, y) = Self::decode_2d_big(level, &BigUint::from(index))?;
        Some((x as u64, y as u64))
    }

    pub fn decode_2d_decimal(level: u32, index: &str) -> Option<(f64, f64)> {
        Self::decode_2d_big(level, &BigUint::parse_bytes(index.as_bytes(), 10)?)
    }

    pub fn decode_2d_big(level: u32, index: &BigUint) -> Option<(f64, f64)> {
        let n = dimension(level, 511)?;
        if index >= &(BigUint::one() << (2 * level) as usize) {
            return None;
        }
        let (mut x, mut y, mut scale) = (0.0, 0.0, 1.0);
        let mut remaining = index.clone();
        while scale < n {
            let rx = (to_int32((&remaining >> 1usize).to_f64()?) & 1) as u32;
            let ry = (to_int32((&remaining ^ BigUint::from(rx)).to_f64()?) & 1) as u32;
            rotate(scale, &mut x, &mut y, rx, ry);
            x += scale * rx as f64;
            y += scale * ry as f64;
            remaining >>= 2usize;
            scale *= 2.0;
        }
        Some((x, y))
    }
}

fn dimension(level: u32, maximum: u32) -> Option<f64> {
    if level == 0 || level > maximum {
        return None;
    }
    Some(f64::from_bits(((level as u64) + 1023) << 52))
}
fn rotate(n: f64, x: &mut f64, y: &mut f64, rx: u32, ry: u32) {
    if ry != 0 {
        return;
    }
    if rx == 1 {
        *x = n - 1.0 - *x;
        *y = n - 1.0 - *y;
    }
    std::mem::swap(x, y);
}
