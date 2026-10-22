//! CesiumJS `readAccessorPacked`: an accessor's values, tightly packed.

use super::get_accessor_byte_stride::{component_size, datatype};
use super::{get_accessor_byte_stride, get_component_reader, js, number_of_components_for_type, Gltf};
use super::{PipelineError, Result};
use serde_json::Value;

/// `count * components` numbers, read with the accessor's stride. An accessor
/// without a buffer view reads as zeros.
pub fn read_accessor_packed(gltf: &Gltf, accessor: &Value) -> Result<Vec<f64>> {
    let byte_stride = get_accessor_byte_stride(&gltf.json, accessor)?;
    let component_byte_length = component_size(accessor.get("componentType"))? as f64;
    let components = accessor.get("type").and_then(Value::as_str).and_then(number_of_components_for_type);
    let count = js::to_number(accessor.get("count"));
    // `new Array(components * count)` needs an array length.
    let length = components.map_or(f64::NAN, |c| c as f64 * count);
    if !(length >= 0.0 && length.fract() == 0.0 && length <= u32::MAX as f64) {
        return Err(PipelineError::Range("Invalid array length".into()));
    }
    let (components, count) = (components.unwrap_or(0), count as usize);
    let mut values = vec![0.0; length as usize];
    let Some(buffer_view_id) = accessor.get("bufferView").filter(|v| !v.is_null()) else {
        return Ok(values);
    };
    let reader = get_component_reader(datatype(accessor.get("componentType"))?);
    let (data, mut byte_offset) = accessor_data(gltf, accessor, buffer_view_id)?;
    let mut scratch = vec![0.0; components];
    for i in 0..count {
        reader(data, byte_offset, components, component_byte_length, &mut scratch)?;
        values[i * components..(i + 1) * components].copy_from_slice(&scratch);
        byte_offset += byte_stride;
    }
    Ok(values)
}

/// The underlying buffer and the accessor's first byte in it:
/// `accessor.byteOffset + bufferView.byteOffset + source.byteOffset`.
pub(crate) fn accessor_data<'a>(gltf: &'a Gltf, accessor: &Value, buffer_view_id: &Value) -> Result<(&'a [u8], f64)> {
    let buffer_view = js::object(js::index(gltf.json.get("bufferViews"), buffer_view_id), "buffer")?;
    let buffer =
        js::object(js::index(gltf.json.get("buffers"), buffer_view.get("buffer").unwrap_or(&Value::Null)), "extras")?;
    let pipeline = js::object(js::object(buffer.get("extras"), "_pipeline")?.get("_pipeline"), "source")?;
    let marker = js::object(pipeline.get("source"), "byteOffset")?;
    let source = gltf.source(marker).ok_or_else(|| PipelineError::Type("source is not a Uint8Array".into()))?;
    let byte_offset = js::to_number(accessor.get("byteOffset"))
        + js::to_number(buffer_view.get("byteOffset"))
        + source.byte_offset as f64;
    Ok((&source.buffer[..], byte_offset))
}

/// `Math.min`, which propagates NaN and orders -0 below 0.
pub(crate) fn js_min(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else if a == b {
        if a.is_sign_negative() {
            a
        } else {
            b
        }
    } else {
        a.min(b)
    }
}

/// `Math.max`, which propagates NaN and orders 0 above -0.
pub(crate) fn js_max(a: f64, b: f64) -> f64 {
    if a.is_nan() || b.is_nan() {
        f64::NAN
    } else if a == b {
        if a.is_sign_positive() {
            a
        } else {
            b
        }
    } else {
        a.max(b)
    }
}
