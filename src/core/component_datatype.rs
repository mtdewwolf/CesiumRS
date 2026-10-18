//! CesiumJS `ComponentDatatype`: the element type of a vertex attribute.

use super::typed_array::TypedArray;
use super::webgl_constants as gl;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u32)]
pub enum ComponentDatatype {
    Byte = gl::BYTE,
    UnsignedByte = gl::UNSIGNED_BYTE,
    Short = gl::SHORT,
    UnsignedShort = gl::UNSIGNED_SHORT,
    Int = gl::INT,
    UnsignedInt = gl::UNSIGNED_INT,
    Float = gl::FLOAT,
    /// Not a WebGL type; Cesium splits doubles into two floats for the GPU.
    Double = gl::DOUBLE,
}

impl ComponentDatatype {
    pub fn get_size_in_bytes(self) -> usize {
        match self {
            Self::Byte | Self::UnsignedByte => 1,
            Self::Short | Self::UnsignedShort => 2,
            Self::Int | Self::UnsignedInt | Self::Float => 4,
            Self::Double => 8,
        }
    }

    /// The GL enum value, or `None` if it is not a component datatype.
    pub fn from_gl(value: u32) -> Option<Self> {
        [
            Self::Byte,
            Self::UnsignedByte,
            Self::Short,
            Self::UnsignedShort,
            Self::Int,
            Self::UnsignedInt,
            Self::Float,
            Self::Double,
        ]
        .into_iter()
        .find(|d| *d as u32 == value)
    }

    pub fn validate(value: u32) -> bool {
        Self::from_gl(value).is_some()
    }

    pub fn from_typed_array(array: &TypedArray) -> Self {
        array.component_datatype()
    }

    pub fn create_typed_array(self, values: &[f64]) -> TypedArray {
        TypedArray::from_values(self, values)
    }

    /// Reads `length` elements (default: to the end) from little-endian bytes,
    /// like a typed-array view on an `ArrayBuffer`. `None` if out of range.
    pub fn create_array_buffer_view(
        self,
        buffer: &[u8],
        byte_offset: usize,
        length: Option<usize>,
    ) -> Option<TypedArray> {
        let size = self.get_size_in_bytes();
        let length = match length {
            Some(l) => l,
            None => buffer.len().checked_sub(byte_offset)? / size,
        };
        let bytes = buffer.get(byte_offset..byte_offset + length * size)?;
        macro_rules! read {
            ($t:ty, $variant:ident) => {
                TypedArray::$variant(
                    bytes.chunks_exact(size).map(|c| <$t>::from_le_bytes(c.try_into().unwrap())).collect(),
                )
            };
        }
        Some(match self {
            Self::Byte => read!(i8, Int8),
            Self::UnsignedByte => read!(u8, Uint8),
            Self::Short => read!(i16, Int16),
            Self::UnsignedShort => read!(u16, Uint16),
            Self::Int => read!(i32, Int32),
            Self::UnsignedInt => read!(u32, Uint32),
            Self::Float => read!(f32, Float32),
            Self::Double => read!(f64, Float64),
        })
    }

    pub fn from_name(name: &str) -> Option<Self> {
        Some(match name {
            "BYTE" => Self::Byte,
            "UNSIGNED_BYTE" => Self::UnsignedByte,
            "SHORT" => Self::Short,
            "UNSIGNED_SHORT" => Self::UnsignedShort,
            "INT" => Self::Int,
            "UNSIGNED_INT" => Self::UnsignedInt,
            "FLOAT" => Self::Float,
            "DOUBLE" => Self::Double,
            _ => return None,
        })
    }
}
