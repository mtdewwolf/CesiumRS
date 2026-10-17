//! CesiumJS `MortonOrder` (Z-order curve) helpers.
//!
//! Coordinates and indices use integer types so values outside the supported
//! bit ranges are rejected instead of silently truncated by JavaScript's
//! 32-bit bitwise operators.

/// Morton (Z-order) encoding for interleaved 2D and 3D integer coordinates.
pub struct MortonOrder;

impl MortonOrder {
    /// Encodes two 16-bit coordinates into a 32-bit Morton index.
    pub fn encode_2d(x: u32, y: u32) -> Option<u32> {
        if x > u16::MAX as u32 || y > u16::MAX as u32 {
            return None;
        }
        Some(insert_one_spacing(x) | (insert_one_spacing(y) << 1))
    }

    /// Decodes a 32-bit Morton index into its two 16-bit coordinates.
    pub fn decode_2d(index: u32) -> [u16; 2] {
        [remove_one_spacing(index) as u16, remove_one_spacing(index >> 1) as u16]
    }

    /// Encodes three 10-bit coordinates into a 30-bit Morton index.
    pub fn encode_3d(x: u32, y: u32, z: u32) -> Option<u32> {
        if x > 1023 || y > 1023 || z > 1023 {
            return None;
        }
        Some(insert_two_spacing(x) | (insert_two_spacing(y) << 1) | (insert_two_spacing(z) << 2))
    }

    /// Decodes a 30-bit Morton index into its three 10-bit coordinates.
    pub fn decode_3d(index: u32) -> Option<[u16; 3]> {
        if index > 0x3fff_ffff {
            return None;
        }
        Some([
            remove_two_spacing(index) as u16,
            remove_two_spacing(index >> 1) as u16,
            remove_two_spacing(index >> 2) as u16,
        ])
    }
}

fn insert_one_spacing(mut value: u32) -> u32 {
    value = (value ^ (value << 8)) & 0x00ff_00ff;
    value = (value ^ (value << 4)) & 0x0f0f_0f0f;
    value = (value ^ (value << 2)) & 0x3333_3333;
    (value ^ (value << 1)) & 0x5555_5555
}

fn insert_two_spacing(mut value: u32) -> u32 {
    value = (value ^ (value << 16)) & 0x0300_00ff;
    value = (value ^ (value << 8)) & 0x0300_f00f;
    value = (value ^ (value << 4)) & 0x030c_30c3;
    (value ^ (value << 2)) & 0x0924_9249
}

fn remove_one_spacing(mut value: u32) -> u32 {
    value &= 0x5555_5555;
    value = (value ^ (value >> 1)) & 0x3333_3333;
    value = (value ^ (value >> 2)) & 0x0f0f_0f0f;
    value = (value ^ (value >> 4)) & 0x00ff_00ff;
    (value ^ (value >> 8)) & 0x0000_ffff
}

fn remove_two_spacing(mut value: u32) -> u32 {
    value &= 0x0924_9249;
    value = (value ^ (value >> 2)) & 0x030c_30c3;
    value = (value ^ (value >> 4)) & 0x0300_f00f;
    value = (value ^ (value >> 8)) & 0xff00_00ff;
    (value ^ (value >> 16)) & 0x0000_03ff
}
