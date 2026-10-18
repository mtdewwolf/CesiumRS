//! CesiumJS `PixelDatatype` (from the Renderer package): the element type of
//! texture data.

use super::webgl_constants as gl;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u32)]
pub enum PixelDatatype {
    UnsignedByte = gl::UNSIGNED_BYTE,
    UnsignedShort = gl::UNSIGNED_SHORT,
    UnsignedInt = gl::UNSIGNED_INT,
    Float = gl::FLOAT,
    /// Cesium uses the WebGL 1 extension's value.
    HalfFloat = gl::HALF_FLOAT_OES,
    UnsignedInt24_8 = gl::UNSIGNED_INT_24_8,
    UnsignedShort4_4_4_4 = gl::UNSIGNED_SHORT_4_4_4_4,
    UnsignedShort5_5_5_1 = gl::UNSIGNED_SHORT_5_5_5_1,
    UnsignedShort5_6_5 = gl::UNSIGNED_SHORT_5_6_5,
}

/// The JavaScript typed-array type for a pixel datatype.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum PixelArrayType {
    Uint8,
    Uint16,
    Uint32,
    Float32,
}

impl PixelDatatype {
    pub const ALL: [Self; 9] = [
        Self::UnsignedByte,
        Self::UnsignedShort,
        Self::UnsignedInt,
        Self::Float,
        Self::HalfFloat,
        Self::UnsignedInt24_8,
        Self::UnsignedShort4_4_4_4,
        Self::UnsignedShort5_5_5_1,
        Self::UnsignedShort5_6_5,
    ];

    /// Cesium's constant name, such as `UNSIGNED_INT_24_8`.
    pub fn name(self) -> &'static str {
        const NAMES: [&str; 9] = [
            "UNSIGNED_BYTE",
            "UNSIGNED_SHORT",
            "UNSIGNED_INT",
            "FLOAT",
            "HALF_FLOAT",
            "UNSIGNED_INT_24_8",
            "UNSIGNED_SHORT_4_4_4_4",
            "UNSIGNED_SHORT_5_5_5_1",
            "UNSIGNED_SHORT_5_6_5",
        ];
        NAMES[Self::ALL.iter().position(|d| *d == self).unwrap()]
    }

    pub fn from_value(value: u32) -> Option<Self> {
        Self::ALL.into_iter().find(|d| *d as u32 == value)
    }

    pub fn validate(value: u32) -> bool {
        Self::from_value(value).is_some()
    }

    /// The GL constant for uploads; half floats differ between WebGL 1 and 2.
    pub fn to_webgl_constant(self, webgl2: bool) -> u32 {
        match self {
            Self::HalfFloat if webgl2 => gl::HALF_FLOAT,
            other => other as u32,
        }
    }

    pub fn is_packed(self) -> bool {
        matches!(
            self,
            Self::UnsignedInt24_8 | Self::UnsignedShort4_4_4_4 | Self::UnsignedShort5_5_5_1 | Self::UnsignedShort5_6_5
        )
    }

    pub fn size_in_bytes(self) -> usize {
        match self {
            Self::UnsignedByte => 1,
            Self::UnsignedShort
            | Self::UnsignedShort4_4_4_4
            | Self::UnsignedShort5_5_5_1
            | Self::UnsignedShort5_6_5
            | Self::HalfFloat => 2,
            Self::UnsignedInt | Self::Float | Self::UnsignedInt24_8 => 4,
        }
    }

    pub fn get_typed_array_type(self) -> PixelArrayType {
        match self.size_in_bytes() {
            1 => PixelArrayType::Uint8,
            2 => PixelArrayType::Uint16,
            4 if self == Self::Float => PixelArrayType::Float32,
            _ => PixelArrayType::Uint32,
        }
    }
}
