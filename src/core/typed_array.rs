//! JavaScript typed arrays, as the geometry code uses them: one enum over the
//! element types Cesium's `ComponentDatatype` and `IndexDatatype` name.

use super::component_datatype::ComponentDatatype;

#[derive(Clone, Debug, PartialEq)]
pub enum TypedArray {
    Int8(Vec<i8>),
    Uint8(Vec<u8>),
    Int16(Vec<i16>),
    Uint16(Vec<u16>),
    Int32(Vec<i32>),
    Uint32(Vec<u32>),
    Float32(Vec<f32>),
    Float64(Vec<f64>),
}

/// JavaScript's conversion of a number to an integer element type (ToInt32
/// and friends): truncate, wrap modulo 2ⁿ, NaN and infinities become 0.
fn wrap(value: f64, bits: u32, signed: bool) -> f64 {
    if !value.is_finite() {
        return 0.0;
    }
    let modulus = 2f64.powi(bits as i32);
    let mut v = value.trunc().rem_euclid(modulus);
    if signed && v >= modulus / 2.0 {
        v -= modulus;
    }
    v
}

macro_rules! each {
    ($self:expr, $v:ident => $body:expr) => {
        match $self {
            TypedArray::Int8($v) => $body,
            TypedArray::Uint8($v) => $body,
            TypedArray::Int16($v) => $body,
            TypedArray::Uint16($v) => $body,
            TypedArray::Int32($v) => $body,
            TypedArray::Uint32($v) => $body,
            TypedArray::Float32($v) => $body,
            TypedArray::Float64($v) => $body,
        }
    };
}

impl TypedArray {
    /// Zero-filled, like `new Float32Array(length)`.
    pub fn zeros(datatype: ComponentDatatype, length: usize) -> Self {
        Self::from_values(datatype, &vec![0.0; length])
    }

    /// Converts each value the way assigning into a JavaScript typed array does.
    pub fn from_values(datatype: ComponentDatatype, values: &[f64]) -> Self {
        match datatype {
            ComponentDatatype::Byte => Self::Int8(values.iter().map(|&v| wrap(v, 8, true) as i8).collect()),
            ComponentDatatype::UnsignedByte => Self::Uint8(values.iter().map(|&v| wrap(v, 8, false) as u8).collect()),
            ComponentDatatype::Short => Self::Int16(values.iter().map(|&v| wrap(v, 16, true) as i16).collect()),
            ComponentDatatype::UnsignedShort => {
                Self::Uint16(values.iter().map(|&v| wrap(v, 16, false) as u16).collect())
            }
            ComponentDatatype::Int => Self::Int32(values.iter().map(|&v| wrap(v, 32, true) as i32).collect()),
            ComponentDatatype::UnsignedInt => Self::Uint32(values.iter().map(|&v| wrap(v, 32, false) as u32).collect()),
            ComponentDatatype::Float => Self::Float32(values.iter().map(|&v| v as f32).collect()),
            ComponentDatatype::Double => Self::Float64(values.to_vec()),
        }
    }

    pub fn component_datatype(&self) -> ComponentDatatype {
        match self {
            Self::Int8(_) => ComponentDatatype::Byte,
            Self::Uint8(_) => ComponentDatatype::UnsignedByte,
            Self::Int16(_) => ComponentDatatype::Short,
            Self::Uint16(_) => ComponentDatatype::UnsignedShort,
            Self::Int32(_) => ComponentDatatype::Int,
            Self::Uint32(_) => ComponentDatatype::UnsignedInt,
            Self::Float32(_) => ComponentDatatype::Float,
            Self::Float64(_) => ComponentDatatype::Double,
        }
    }

    pub fn len(&self) -> usize {
        each!(self, v => v.len())
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    #[allow(clippy::useless_conversion)]
    pub fn get(&self, index: usize) -> Option<f64> {
        each!(self, v => v.get(index).map(|&x| f64::from(x)))
    }

    /// Stores `value` with the element type's conversion. Out of range is ignored, as in JavaScript.
    pub fn set(&mut self, index: usize, value: f64) {
        match self {
            Self::Int8(v) => {
                if let Some(x) = v.get_mut(index) {
                    *x = wrap(value, 8, true) as i8
                }
            }
            Self::Uint8(v) => {
                if let Some(x) = v.get_mut(index) {
                    *x = wrap(value, 8, false) as u8
                }
            }
            Self::Int16(v) => {
                if let Some(x) = v.get_mut(index) {
                    *x = wrap(value, 16, true) as i16
                }
            }
            Self::Uint16(v) => {
                if let Some(x) = v.get_mut(index) {
                    *x = wrap(value, 16, false) as u16
                }
            }
            Self::Int32(v) => {
                if let Some(x) = v.get_mut(index) {
                    *x = wrap(value, 32, true) as i32
                }
            }
            Self::Uint32(v) => {
                if let Some(x) = v.get_mut(index) {
                    *x = wrap(value, 32, false) as u32
                }
            }
            Self::Float32(v) => {
                if let Some(x) = v.get_mut(index) {
                    *x = value as f32
                }
            }
            Self::Float64(v) => {
                if let Some(x) = v.get_mut(index) {
                    *x = value
                }
            }
        }
    }

    /// Every element as a number.
    #[allow(clippy::useless_conversion)]
    pub fn to_f64_vec(&self) -> Vec<f64> {
        each!(self, v => v.iter().map(|&x| f64::from(x)).collect())
    }

    /// The underlying `ArrayBuffer` bytes, little-endian as WebGL uploads them.
    pub fn to_le_bytes(&self) -> Vec<u8> {
        each!(self, v => v.iter().flat_map(|x| x.to_le_bytes()).collect())
    }
}
