//! CesiumJS `findAccessorMinMax`: an accessor's per-component bounds.

use super::get_accessor_byte_stride::{component_size, datatype};
use super::read_accessor_packed::{accessor_data, js_max, js_min};
use super::{get_accessor_byte_stride, get_component_reader, js, number_of_components_for_type, Gltf, Result};
use serde_json::Value;

#[derive(Clone, Debug, PartialEq)]
pub struct MinMax {
    pub min: Vec<f64>,
    pub max: Vec<f64>,
}

/// Zeros for an accessor without a buffer view. An unknown accessor type
/// gives one-element arrays (`new Array(undefined)` holds one element) that
/// no component updates.
pub fn find_accessor_min_max(gltf: &Gltf, accessor: &Value) -> Result<MinMax> {
    let components = accessor.get("type").and_then(Value::as_str).and_then(number_of_components_for_type);
    let slots = components.unwrap_or(1);
    let Some(buffer_view_id) = accessor.get("bufferView").filter(|v| !v.is_null()) else {
        return Ok(MinMax { min: vec![0.0; slots], max: vec![0.0; slots] });
    };
    let mut min = vec![f64::INFINITY; slots];
    let mut max = vec![f64::NEG_INFINITY; slots];
    let (data, mut byte_offset) = accessor_data(gltf, accessor, buffer_view_id)?;
    let count = js::to_number(accessor.get("count"));
    let byte_stride = get_accessor_byte_stride(&gltf.json, accessor)?;
    let component_byte_length = component_size(accessor.get("componentType"))? as f64;
    let reader = get_component_reader(datatype(accessor.get("componentType"))?);
    let components = components.unwrap_or(0);
    let mut scratch = vec![0.0; components];
    let mut i = 0.0;
    while i < count {
        reader(data, byte_offset, components, component_byte_length, &mut scratch)?;
        for (j, &value) in scratch.iter().enumerate() {
            min[j] = js_min(min[j], value);
            max[j] = js_max(max[j], value);
        }
        byte_offset += byte_stride;
        i += 1.0;
    }
    Ok(MinMax { min, max })
}
