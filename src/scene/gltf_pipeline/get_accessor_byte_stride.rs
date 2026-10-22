//! CesiumJS `getAccessorByteStride`: the bytes between accessor elements.

use super::{js, number_of_components_for_type, PipelineError, Result};
use crate::core::ComponentDatatype;
use serde_json::Value;

/// The buffer view's positive `byteStride`, else the packed element size.
/// Unknown component or accessor types are an error: Cesium's size lookup
/// throws, and the missing component count makes the result `NaN`.
pub fn get_accessor_byte_stride(gltf: &Value, accessor: &Value) -> Result<f64> {
    if let Some(buffer_view_id) = accessor.get("bufferView").filter(|v| !v.is_null()) {
        let buffer_view = js::object(js::index(gltf.get("bufferViews"), buffer_view_id), "byteStride")?;
        let stride = buffer_view.get("byteStride").filter(|v| !v.is_null());
        if stride.is_some() && js::to_number(stride) > 0.0 {
            return Ok(js::to_number(stride));
        }
    }
    let size = component_size(accessor.get("componentType"))?;
    let components = accessor.get("type").and_then(Value::as_str).and_then(number_of_components_for_type);
    Ok(components.map_or(f64::NAN, |c| (size * c) as f64))
}

/// `ComponentDatatype.getSizeInBytes` of an accessor's `componentType`.
pub(crate) fn component_size(component_type: Option<&Value>) -> Result<usize> {
    datatype(component_type).map(ComponentDatatype::get_size_in_bytes)
}

pub(crate) fn datatype(component_type: Option<&Value>) -> Result<ComponentDatatype> {
    let value = js::to_number(component_type);
    ComponentDatatype::from_gl(value as u32)
        .filter(|_| value.fract() == 0.0 && value >= 0.0)
        .ok_or_else(|| PipelineError::Developer("componentType is not a valid value.".into()))
}
