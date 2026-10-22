//! CesiumJS `addBuffer`: appends a buffer and a buffer view over all of it.

use super::{js, BufferSource, Gltf, PipelineError, Result};
use serde_json::{json, Value};

/// Adds a buffer whose `extras._pipeline.source` is `buffer`, and a buffer
/// view of its whole length. Returns the buffer view's index.
pub fn add_buffer(gltf: &mut Gltf, buffer: BufferSource) -> Result<usize> {
    let byte_length = buffer.byte_length;
    let source = gltf.add_source(buffer);
    let new_buffer = json!({ "byteLength": byte_length, "extras": { "_pipeline": { "source": source } } });
    let buffer_id = push(&mut gltf.json, "buffers", new_buffer)?;
    let buffer_view = json!({ "buffer": buffer_id, "byteOffset": 0, "byteLength": byte_length });
    push(&mut gltf.json, "bufferViews", buffer_view)
}

/// `addToArray(gltf[name], value)` without the duplicate check. Cesium
/// assumes the array exists.
pub(crate) fn push(gltf: &mut Value, name: &str, value: Value) -> Result<usize> {
    match js::get_mut(Some(gltf), name) {
        Some(Value::Array(items)) => Ok(super::add_to_array(items, value, false)),
        _ => Err(PipelineError::Type(format!("Cannot read properties of undefined (reading 'push') for {name}"))),
    }
}
