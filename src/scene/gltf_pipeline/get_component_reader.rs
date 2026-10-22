//! CesiumJS `getComponentReader`: reads accessor components through a
//! little-endian `DataView`.

use super::{js, PipelineError, Result};
use crate::core::ComponentDatatype;

/// `reader(data, byte_offset, number_of_components, component_byte_length,
/// result)` reads `number_of_components` values, `component_byte_length`
/// bytes apart, into `result`. Offsets are JavaScript numbers: `NaN` reads at
/// 0, and a read past `data` is a `RangeError`.
pub type ComponentReader = fn(&[u8], f64, usize, f64, &mut [f64]) -> Result<()>;

pub fn get_component_reader(component_type: ComponentDatatype) -> ComponentReader {
    match component_type {
        ComponentDatatype::Byte => |d, o, n, l, r| read(d, o, n, l, r, 1, |b| i8::from_le_bytes([b[0]]) as f64),
        ComponentDatatype::UnsignedByte => |d, o, n, l, r| read(d, o, n, l, r, 1, |b| b[0] as f64),
        ComponentDatatype::Short => |d, o, n, l, r| read(d, o, n, l, r, 2, |b| i16::from_le_bytes([b[0], b[1]]) as f64),
        ComponentDatatype::UnsignedShort => {
            |d, o, n, l, r| read(d, o, n, l, r, 2, |b| u16::from_le_bytes([b[0], b[1]]) as f64)
        }
        ComponentDatatype::Int => {
            |d, o, n, l, r| read(d, o, n, l, r, 4, |b| i32::from_le_bytes(b.try_into().unwrap()) as f64)
        }
        ComponentDatatype::UnsignedInt => {
            |d, o, n, l, r| read(d, o, n, l, r, 4, |b| u32::from_le_bytes(b.try_into().unwrap()) as f64)
        }
        ComponentDatatype::Float => {
            |d, o, n, l, r| read(d, o, n, l, r, 4, |b| f32::from_le_bytes(b.try_into().unwrap()) as f64)
        }
        ComponentDatatype::Double => {
            |d, o, n, l, r| read(d, o, n, l, r, 8, |b| f64::from_le_bytes(b.try_into().unwrap()))
        }
    }
}

fn read(
    data: &[u8],
    byte_offset: f64,
    number_of_components: usize,
    component_byte_length: f64,
    result: &mut [f64],
    size: usize,
    decode: impl Fn(&[u8]) -> f64,
) -> Result<()> {
    for (i, slot) in result.iter_mut().enumerate().take(number_of_components) {
        *slot = decode(data_view(data, byte_offset + i as f64 * component_byte_length, size)?);
    }
    Ok(())
}

/// The `size` bytes a `DataView` getter reads at `byte_offset`.
pub(crate) fn data_view(data: &[u8], byte_offset: f64, size: usize) -> Result<&[u8]> {
    let start = js::to_index(byte_offset)?;
    start
        .checked_add(size)
        .and_then(|end| data.get(start..end))
        .ok_or_else(|| PipelineError::Range("Offset is outside the bounds of the DataView".into()))
}
