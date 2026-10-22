//! CesiumJS `MetadataComponentType`: the numeric component types of 3D
//! Metadata, their ranges, and normalization.
//!
//! Cesium represents 64-bit integers as `BigInt` where the platform supports
//! it, which every target of this port does; [`MetadataNumber::BigInt`] is
//! that value.

use crate::core::math::{js_round, sign};
use crate::core::ComponentDatatype;

string_enum!(MetadataComponentType {
    Int8 = "INT8" => "INT8",
    Uint8 = "UINT8" => "UINT8",
    Int16 = "INT16" => "INT16",
    Uint16 = "UINT16" => "UINT16",
    Int32 = "INT32" => "INT32",
    Uint32 = "UINT32" => "UINT32",
    Int64 = "INT64" => "INT64",
    Uint64 = "UINT64" => "UINT64",
    Float32 = "FLOAT32" => "FLOAT32",
    Float64 = "FLOAT64" => "FLOAT64",
});

string_enum!(
    /// Cesium's `ScalarCategories`.
    ScalarCategory {
        Integer = "int" => "INTEGER",
        UnsignedInteger = "uint" => "UNSIGNED_INTEGER",
        Float = "float" => "FLOAT",
    }
);

/// A JavaScript `number` or `BigInt` metadata value.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MetadataNumber {
    Number(f64),
    BigInt(i128),
}

impl MetadataNumber {
    /// JavaScript `Number(value)`: a `BigInt` rounds to the nearest double.
    pub fn to_f64(self) -> f64 {
        match self {
            Self::Number(v) => v,
            Self::BigInt(v) => v as f64,
        }
    }
}

/// `unnormalize` of a 64-bit type converts a non-integral number to `BigInt`,
/// which throws a `RangeError` in JavaScript.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BigIntRangeError(pub String);

impl std::fmt::Display for BigIntRangeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for BigIntRangeError {}

impl MetadataComponentType {
    pub fn is_vector_compatible(self) -> bool {
        !matches!(self, Self::Int64 | Self::Uint64)
    }

    pub fn get_size_in_bytes(self) -> usize {
        match self {
            Self::Int8 | Self::Uint8 => 1,
            Self::Int16 | Self::Uint16 => 2,
            Self::Int32 | Self::Uint32 | Self::Float32 => 4,
            Self::Int64 | Self::Uint64 | Self::Float64 => 8,
        }
    }

    pub fn category(self) -> ScalarCategory {
        match self {
            Self::Int8 | Self::Int16 | Self::Int32 | Self::Int64 => ScalarCategory::Integer,
            Self::Uint8 | Self::Uint16 | Self::Uint32 | Self::Uint64 => ScalarCategory::UnsignedInteger,
            Self::Float32 | Self::Float64 => ScalarCategory::Float,
        }
    }

    pub fn is_integer_type(self) -> bool {
        self.category() != ScalarCategory::Float
    }

    pub fn is_unsigned_integer_type(self) -> bool {
        self.category() == ScalarCategory::UnsignedInteger
    }

    pub fn get_minimum(self) -> MetadataNumber {
        use MetadataNumber::*;
        match self {
            Self::Int8 => Number(-128.0),
            Self::Int16 => Number(-32768.0),
            Self::Int32 => Number(-2147483648.0),
            Self::Int64 => BigInt(i64::MIN as i128),
            Self::Uint8 | Self::Uint16 | Self::Uint32 => Number(0.0),
            Self::Uint64 => BigInt(0),
            Self::Float32 => Number(-(f32::MAX as f64)),
            Self::Float64 => Number(-f64::MAX),
        }
    }

    pub fn get_maximum(self) -> MetadataNumber {
        use MetadataNumber::*;
        match self {
            Self::Int8 => Number(127.0),
            Self::Uint8 => Number(255.0),
            Self::Int16 => Number(32767.0),
            Self::Uint16 => Number(65535.0),
            Self::Int32 => Number(2147483647.0),
            Self::Uint32 => Number(4294967295.0),
            Self::Int64 => BigInt(i64::MAX as i128),
            Self::Uint64 => BigInt(u64::MAX as i128),
            Self::Float32 => Number(f32::MAX as f64),
            Self::Float64 => Number(f64::MAX),
        }
    }

    /// `value / maximum`, clamped below at -1. `None` for float types, where
    /// Cesium throws.
    pub fn normalize(value: MetadataNumber, component_type: Self) -> Option<f64> {
        if !component_type.is_integer_type() {
            return None;
        }
        // `Math.max` propagates NaN, which `f64::max` would drop.
        let value = value.to_f64() / component_type.get_maximum().to_f64();
        Some(if value.is_nan() { value } else { value.max(-1.0) })
    }

    /// Scales a normalized value back to the integer range: rounded, then
    /// clamped to `[-maximum, maximum]` (`[0, maximum]` when unsigned). 64-bit
    /// types return a `BigInt`, except that `UINT64` clamps below to the
    /// number `0`, as Cesium's `min` is. `Ok(None)` for float types, where
    /// Cesium throws.
    pub fn unnormalize(value: f64, component_type: Self) -> Result<Option<MetadataNumber>, BigIntRangeError> {
        if !component_type.is_integer_type() {
            return Ok(None);
        }
        let max = component_type.get_maximum();
        let value = sign(value) * js_round(value.abs() * max.to_f64());
        let unsigned = component_type.is_unsigned_integer_type();
        let result = match max {
            MetadataNumber::BigInt(max) => {
                if value.fract() != 0.0 || !value.is_finite() {
                    let shown = crate::core::math::js_number_to_string(value);
                    return Err(BigIntRangeError(format!(
                        "The number {shown} cannot be converted to a BigInt because it is not an integer"
                    )));
                }
                // `as` saturates past i128, which is past the clamp too.
                let big = value as i128;
                if unsigned && big < 0 {
                    MetadataNumber::Number(0.0)
                } else {
                    MetadataNumber::BigInt(big.clamp(-max, max))
                }
            }
            MetadataNumber::Number(max) => {
                let min = if unsigned { 0.0 } else { -max };
                // NaN fails both comparisons and is returned as is.
                MetadataNumber::Number(if value > max {
                    max
                } else if value < min {
                    min
                } else {
                    value
                })
            }
        };
        Ok(Some(result))
    }

    pub fn apply_value_transform(value: f64, offset: f64, scale: f64) -> f64 {
        scale * value + offset
    }

    pub fn unapply_value_transform(value: f64, offset: f64, scale: f64) -> f64 {
        if scale == 0.0 {
            return 0.0;
        }
        (value - offset) / scale
    }

    pub fn from_component_datatype(datatype: ComponentDatatype) -> Self {
        match datatype {
            ComponentDatatype::Byte => Self::Int8,
            ComponentDatatype::UnsignedByte => Self::Uint8,
            ComponentDatatype::Short => Self::Int16,
            ComponentDatatype::UnsignedShort => Self::Uint16,
            ComponentDatatype::Int => Self::Int32,
            ComponentDatatype::UnsignedInt => Self::Uint32,
            ComponentDatatype::Float => Self::Float32,
            ComponentDatatype::Double => Self::Float64,
        }
    }

    /// `None` for `INT64` and `UINT64`.
    pub fn to_component_datatype(self) -> Option<ComponentDatatype> {
        Some(match self {
            Self::Int8 => ComponentDatatype::Byte,
            Self::Uint8 => ComponentDatatype::UnsignedByte,
            Self::Int16 => ComponentDatatype::Short,
            Self::Uint16 => ComponentDatatype::UnsignedShort,
            Self::Int32 => ComponentDatatype::Int,
            Self::Uint32 => ComponentDatatype::UnsignedInt,
            Self::Float32 => ComponentDatatype::Float,
            Self::Float64 => ComponentDatatype::Double,
            Self::Int64 | Self::Uint64 => return None,
        })
    }

    /// Cesium's `getDataViewSetter`: writes `value` little-endian at `offset`
    /// the way the matching `DataView` setter does. Numbers wrap like typed
    /// array stores; 64-bit types take `BigInt(value)` and wrap modulo 2^64.
    /// `None` when the write is out of bounds (a `RangeError` in JavaScript)
    /// or a number given to a 64-bit type is not an integer.
    pub fn set_data_view_value(self, bytes: &mut [u8], offset: usize, value: MetadataNumber) -> Option<()> {
        let encoded: Vec<u8> = match (self, value) {
            (Self::Int64 | Self::Uint64, _) => {
                let big = match value {
                    MetadataNumber::BigInt(v) => v,
                    // Doubles from 2^117 up are multiples of 2^64, so they wrap to 0.
                    MetadataNumber::Number(v) if v.is_finite() && v.fract() == 0.0 => {
                        if v.abs() >= 2f64.powi(117) {
                            0
                        } else {
                            v as i128
                        }
                    }
                    MetadataNumber::Number(_) => return None,
                };
                (big as u64).to_le_bytes().to_vec()
            }
            // The other setters apply ToNumber, so a BigInt throws.
            (_, MetadataNumber::BigInt(_)) => return None,
            (Self::Float32, MetadataNumber::Number(v)) => (v as f32).to_le_bytes().to_vec(),
            (Self::Float64, MetadataNumber::Number(v)) => v.to_le_bytes().to_vec(),
            (integer, MetadataNumber::Number(v)) => {
                let wrapped = crate::core::math::to_int32(v) as u32;
                wrapped.to_le_bytes()[..integer.get_size_in_bytes()].to_vec()
            }
        };
        bytes.get_mut(offset..offset.checked_add(encoded.len())?)?.copy_from_slice(&encoded);
        Some(())
    }
}
