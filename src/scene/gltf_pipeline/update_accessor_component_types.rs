//! CesiumJS `updateAccessorComponentTypes`: rewrites skinning accessors to
//! the unsigned component types glTF 2.0 requires.

use super::for_each::{self, collect_values};
use super::{add_buffer, js, read_accessor_packed, BufferSource, Gltf, Result};
use crate::core::webgl_constants as gl;
use crate::core::{ComponentDatatype, TypedArray};
use serde_json::Value;

/// `JOINTS_0` accessors become `UNSIGNED_BYTE` (from `BYTE`) or
/// `UNSIGNED_SHORT` (from anything else but the two unsigned types);
/// `WEIGHTS_0` accessors become `UNSIGNED_BYTE` from `BYTE` and
/// `UNSIGNED_SHORT` from `SHORT`. Converted data goes in a new buffer.
pub fn update_accessor_component_types(gltf: &mut Gltf) -> Result<()> {
    let joints = collect_values(|visit| {
        for_each::accessor_with_semantic(&gltf.json, "JOINTS_0", visit);
    });
    for accessor_id in joints {
        let component_type = component_type(gltf, &accessor_id)?;
        if component_type == gl::BYTE as f64 {
            convert_type(gltf, &accessor_id, ComponentDatatype::UnsignedByte)?;
        } else if component_type != gl::UNSIGNED_BYTE as f64 && component_type != gl::UNSIGNED_SHORT as f64 {
            convert_type(gltf, &accessor_id, ComponentDatatype::UnsignedShort)?;
        }
    }
    let weights = collect_values(|visit| {
        for_each::accessor_with_semantic(&gltf.json, "WEIGHTS_0", visit);
    });
    for accessor_id in weights {
        let component_type = component_type(gltf, &accessor_id)?;
        if component_type == gl::BYTE as f64 {
            convert_type(gltf, &accessor_id, ComponentDatatype::UnsignedByte)?;
        } else if component_type == gl::SHORT as f64 {
            convert_type(gltf, &accessor_id, ComponentDatatype::UnsignedShort)?;
        }
    }
    Ok(())
}

fn component_type(gltf: &Gltf, accessor_id: &Value) -> Result<f64> {
    let accessor = js::object(js::index(gltf.json.get("accessors"), accessor_id), "componentType")?;
    // Cesium compares with `===`, so only a number matches.
    Ok(accessor.get("componentType").and_then(Value::as_f64).unwrap_or(f64::NAN))
}

fn convert_type(gltf: &mut Gltf, accessor_id: &Value, updated: ComponentDatatype) -> Result<()> {
    let accessor = js::object(js::index(gltf.json.get("accessors"), accessor_id), "componentType")?.clone();
    let values = read_accessor_packed(gltf, &accessor)?;
    let bytes = typed_array_bytes(&TypedArray::from_values(updated, &values));
    let buffer_view = add_buffer(gltf, BufferSource::new(bytes))?;
    let accessor = js::object_mut(js::index_mut(gltf.json.get_mut("accessors"), accessor_id), "bufferView")?;
    js::set(accessor, "bufferView", Value::from(buffer_view))?;
    js::set(accessor, "componentType", Value::from(updated as u32))?;
    js::set(accessor, "byteOffset", Value::from(0))
}

/// The little-endian bytes of a typed array.
pub(crate) fn typed_array_bytes(array: &TypedArray) -> Vec<u8> {
    match array {
        TypedArray::Int8(v) => v.iter().flat_map(|x| x.to_le_bytes()).collect(),
        TypedArray::Uint8(v) => v.clone(),
        TypedArray::Int16(v) => v.iter().flat_map(|x| x.to_le_bytes()).collect(),
        TypedArray::Uint16(v) => v.iter().flat_map(|x| x.to_le_bytes()).collect(),
        TypedArray::Int32(v) => v.iter().flat_map(|x| x.to_le_bytes()).collect(),
        TypedArray::Uint32(v) => v.iter().flat_map(|x| x.to_le_bytes()).collect(),
        TypedArray::Float32(v) => v.iter().flat_map(|x| x.to_le_bytes()).collect(),
        TypedArray::Float64(v) => v.iter().flat_map(|x| x.to_le_bytes()).collect(),
    }
}
