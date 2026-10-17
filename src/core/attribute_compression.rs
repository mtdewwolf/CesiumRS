//! CesiumJS `AttributeCompression`: packed normals, texture coordinates,
//! quantized mesh buffers, and compact RGB colors.

use super::{math as cm, Cartesian2, Cartesian3, Cartesian4, Color};

pub struct AttributeCompression;

impl AttributeCompression {
    pub fn oct_encode_in_range(vector: Cartesian3, range_max: f64) -> Cartesian2 {
        let denominator = vector.x.abs() + vector.y.abs() + vector.z.abs();
        let mut x = vector.x / denominator;
        let mut y = vector.y / denominator;
        if vector.z < 0.0 {
            let old_x = x;
            x = (1.0 - y.abs()) * cm::sign_not_zero(x);
            y = (1.0 - old_x.abs()) * cm::sign_not_zero(y);
        }
        Cartesian2::new(cm::to_snorm(x, range_max), cm::to_snorm(y, range_max))
    }

    pub fn oct_encode(vector: Cartesian3) -> Cartesian2 {
        Self::oct_encode_in_range(vector, 255.0)
    }

    pub fn oct_encode_to_cartesian4(vector: Cartesian3) -> Cartesian4 {
        let encoded = Self::oct_encode_in_range(vector, 65535.0);
        let byte = |value| (cm::to_uint32(value) & 255) as f64;
        Cartesian4::new(byte(encoded.x / 256.0), byte(encoded.x), byte(encoded.y / 256.0), byte(encoded.y))
    }

    pub fn oct_decode_in_range(x: f64, y: f64, range_max: f64) -> Cartesian3 {
        let mut x = cm::from_snorm(x, range_max);
        let mut y = cm::from_snorm(y, range_max);
        let z = 1.0 - (x.abs() + y.abs());
        if z < 0.0 {
            let old_x = x;
            x = (1.0 - y.abs()) * cm::sign_not_zero(old_x);
            y = (1.0 - old_x.abs()) * cm::sign_not_zero(y);
        }
        Cartesian3::new(x, y, z).normalize()
    }

    pub fn oct_decode(x: f64, y: f64) -> Cartesian3 {
        Self::oct_decode_in_range(x, y, 255.0)
    }

    pub fn oct_decode_from_cartesian4(encoded: Cartesian4) -> Cartesian3 {
        Self::oct_decode_in_range(encoded.x * 256.0 + encoded.y, encoded.z * 256.0 + encoded.w, 65535.0)
    }

    pub fn oct_pack_float(encoded: Cartesian2) -> f64 {
        256.0 * encoded.x + encoded.y
    }

    pub fn oct_encode_float(vector: Cartesian3) -> f64 {
        Self::oct_pack_float(Self::oct_encode(vector))
    }

    pub fn oct_decode_float(value: f64) -> Cartesian3 {
        let temp = value / 256.0;
        let x = temp.floor();
        Self::oct_decode(x, (temp - x) * 256.0)
    }

    pub fn oct_pack(v1: Cartesian3, v2: Cartesian3, v3: Cartesian3) -> Cartesian2 {
        let encoded1 = Self::oct_encode_float(v1);
        let encoded2 = Self::oct_encode_float(v2);
        let encoded3 = Self::oct_encode(v3);
        Cartesian2::new(65536.0 * encoded3.x + encoded1, 65536.0 * encoded3.y + encoded2)
    }

    pub fn oct_unpack(packed: Cartesian2) -> [Cartesian3; 3] {
        let temp_x = packed.x / 65536.0;
        let x = temp_x.floor();
        let temp_y = packed.y / 65536.0;
        let y = temp_y.floor();
        [
            Self::oct_decode_float((temp_x - x) * 65536.0),
            Self::oct_decode_float((temp_y - y) * 65536.0),
            Self::oct_decode(x, y),
        ]
    }

    /// Packs texture coordinates with 12 bits per component.
    pub fn compress_texture_coordinates(coordinates: Cartesian2) -> f64 {
        let x = cm::to_int32(coordinates.x * 4095.0);
        let y = cm::to_int32(coordinates.y * 4095.0);
        4096.0 * x as f64 + y as f64
    }

    pub fn decompress_texture_coordinates(compressed: f64) -> Cartesian2 {
        let x = (compressed / 4096.0).floor();
        Cartesian2::new(x / 4095.0, (compressed - x * 4096.0) / 4095.0)
    }

    /// Mutates terrain buffers in place, preserving Uint16Array wraparound.
    /// Returns `None` for mismatched lengths, without changing any buffer.
    pub fn zig_zag_delta_decode(u: &mut [u16], v: &mut [u16], height: Option<&mut [u16]>) -> Option<()> {
        if u.len() != v.len() || height.as_ref().is_some_and(|h| h.len() != u.len()) {
            return None;
        }
        fn decode(buffer: &mut [u16]) {
            let mut accumulator = 0u16;
            for value in buffer {
                let encoded = *value as i32;
                let delta = (encoded >> 1) ^ -(encoded & 1);
                accumulator = accumulator.wrapping_add(delta as u16);
                *value = accumulator;
            }
        }
        decode(u);
        decode(v);
        if let Some(h) = height {
            decode(h);
        }
        Some(())
    }

    /// Dequantizes signed/unsigned 8-, 16-, or 32-bit components to Float32.
    ///
    /// Rust's element type replaces Cesium's ComponentDatatype argument.
    /// `components` is the number of components for SCALAR, VEC2/3/4 or
    /// MAT2/3/4 (1, 2, 3, 4, 4, 9 or 16). Extra input components are ignored.
    pub fn dequantize<T: QuantizedComponent>(values: &[T], components: usize, count: usize) -> Option<Vec<f32>> {
        if ![1, 2, 3, 4, 9, 16].contains(&components) {
            return None;
        }
        let length = components.checked_mul(count)?;
        let values = values.get(..length)?;
        Some(values.iter().map(|v| cm::js_max(v.as_f64() / T::DIVISOR, -1.0) as f32).collect())
    }

    pub fn encode_rgb8(color: Color) -> f64 {
        let byte = |v| cm::js_round(cm::clamp(v * 255.0, 0.0, 255.0));
        byte(color.red) * 65536.0 + byte(color.green) * 256.0 + byte(color.blue)
    }

    /// Decodes RGB with opaque alpha. Use `decode_rgb8_into` to retain alpha.
    pub fn decode_rgb8(encoded: f64) -> Color {
        let mut result = Color::default();
        Self::decode_rgb8_into(encoded, &mut result);
        result
    }

    /// Preserves the result's alpha, just as Cesium's result parameter does.
    pub fn decode_rgb8_into(encoded: f64, result: &mut Color) {
        let encoded = cm::to_int32(encoded.floor());
        result.red = ((encoded >> 16) & 255) as f64 / 255.0;
        result.green = ((encoded >> 8) & 255) as f64 / 255.0;
        result.blue = (encoded & 255) as f64 / 255.0;
    }

    pub fn decode_rgb565(values: &[u16]) -> Vec<f32> {
        let mut result = Vec::with_capacity(values.len() * 3);
        for &value in values {
            result.push(((value >> 11) as f64 * (1.0 / 31.0)) as f32);
            result.push((((value >> 5) & 63) as f64 * (1.0 / 63.0)) as f32);
            result.push(((value & 31) as f64 * (1.0 / 31.0)) as f32);
        }
        result
    }
}

mod sealed {
    pub trait Sealed {}
}

/// The six integer component types accepted by Cesium's dequantizer.
pub trait QuantizedComponent: sealed::Sealed {
    const DIVISOR: f64;
    fn as_f64(&self) -> f64;
}

macro_rules! component {
    ($ty:ty, $divisor:expr) => {
        impl sealed::Sealed for $ty {}
        impl QuantizedComponent for $ty {
            const DIVISOR: f64 = $divisor;
            fn as_f64(&self) -> f64 {
                *self as f64
            }
        }
    };
}

component!(i8, 127.0);
component!(u8, 255.0);
component!(i16, 32767.0);
component!(u16, 65535.0);
component!(i32, 2147483647.0);
component!(u32, 4294967295.0);
