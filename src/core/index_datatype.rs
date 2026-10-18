//! CesiumJS `IndexDatatype`: the element type of an index buffer.

use super::math::SIXTY_FOUR_KILOBYTES;
use super::typed_array::TypedArray;
use super::webgl_constants as gl;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u32)]
pub enum IndexDatatype {
    UnsignedByte = gl::UNSIGNED_BYTE,
    UnsignedShort = gl::UNSIGNED_SHORT,
    UnsignedInt = gl::UNSIGNED_INT,
}

impl IndexDatatype {
    pub fn get_size_in_bytes(self) -> usize {
        match self {
            Self::UnsignedByte => 1,
            Self::UnsignedShort => 2,
            Self::UnsignedInt => 4,
        }
    }

    pub fn from_size_in_bytes(size_in_bytes: usize) -> Option<Self> {
        match size_in_bytes {
            1 => Some(Self::UnsignedByte),
            2 => Some(Self::UnsignedShort),
            4 => Some(Self::UnsignedInt),
            _ => None,
        }
    }

    pub fn validate(value: u32) -> bool {
        [Self::UnsignedByte, Self::UnsignedShort, Self::UnsignedInt].iter().any(|d| *d as u32 == value)
    }

    /// 16-bit indices below 64K vertices, 32-bit from then on.
    pub fn create_typed_array(number_of_vertices: usize, indices: &[u32]) -> TypedArray {
        if number_of_vertices >= SIXTY_FOUR_KILOBYTES as usize {
            TypedArray::Uint32(indices.to_vec())
        } else {
            TypedArray::Uint16(indices.iter().map(|&i| i as u16).collect())
        }
    }

    /// Zero-filled indices of the right width for `number_of_vertices`.
    pub fn create_typed_array_with_length(number_of_vertices: usize, length: usize) -> TypedArray {
        Self::create_typed_array(number_of_vertices, &vec![0; length])
    }

    pub fn from_typed_array(array: &TypedArray) -> Option<Self> {
        match array {
            TypedArray::Uint8(_) => Some(Self::UnsignedByte),
            TypedArray::Uint16(_) => Some(Self::UnsignedShort),
            TypedArray::Uint32(_) => Some(Self::UnsignedInt),
            _ => None,
        }
    }
}
