//! CesiumJS `parseGlb`: reads binary glTF 1.0 and 2.0 containers.

use super::get_component_reader::data_view;
use super::{add_pipeline_extras, js, remove_extensions_used, BufferSource, Gltf, PipelineError, Result};
use crate::core::get_string_from_typed_array::{get_magic, get_string_from_typed_array};
use serde_json::Value;

const JSON_CHUNK: u32 = 0x4e4f534a;
const BIN_CHUNK: u32 = 0x004e4942;

/// The JSON, with pipeline extras added, and the binary body as the source
/// of the binary buffer: `binary_glTF` / `KHR_binary_glTF` in version 1, the
/// first buffer in version 2. The body is a view into `glb`'s buffer.
///
/// A version 2 container without a JSON chunk gives `Ok(None)`, where Cesium
/// returns `undefined`. Like Cesium, it reads the five-word version 1 header
/// before looking at the version, through the view's whole buffer: a version
/// 2 container whose buffer ends within 20 bytes is a `RangeError`, even
/// with no chunks.
pub fn parse_glb(glb: &BufferSource) -> Result<Option<Gltf>> {
    if get_magic(glb.bytes(), 0) != "glTF" {
        return Err(PipelineError::Runtime("File is not valid binary glTF".into()));
    }
    let header = read_header(glb, 0, 5)?;
    match header[1] {
        1 => parse_version1(glb, &header).map(Some),
        2 => parse_version2(glb, &header),
        _ => Err(PipelineError::Runtime("Binary glTF version is not 1 or 2".into())),
    }
}

/// Little-endian `Uint32`s through a `DataView` over the whole buffer.
fn read_header(glb: &BufferSource, byte_offset: usize, count: usize) -> Result<Vec<u32>> {
    (0..count)
        .map(|i| {
            let bytes = data_view(&glb.buffer, (glb.byte_offset + byte_offset + i * 4) as f64, 4)?;
            Ok(u32::from_le_bytes(bytes.try_into().unwrap()))
        })
        .collect()
}

/// `glb.subarray(start, end)`, clamped as `subarray` clamps.
fn subarray(glb: &BufferSource, start: usize, end: usize) -> BufferSource {
    let start = start.min(glb.byte_length);
    let end = end.clamp(start, glb.byte_length);
    BufferSource::view(glb.buffer.clone(), glb.byte_offset + start, end - start)
}

fn parse_json(text: &str) -> Result<Value> {
    serde_json::from_str(text).map_err(|e| PipelineError::Syntax(e.to_string()))
}

fn parse_version1(glb: &BufferSource, header: &[u32]) -> Result<Gltf> {
    let (length, content_length, content_format) = (header[2] as usize, header[3] as usize, header[4]);
    if content_format != 0 {
        return Err(PipelineError::Runtime("Binary glTF scene format is not JSON".into()));
    }
    let json_start = 20;
    let binary_start = json_start + content_length;
    let mut json = parse_json(&get_string_from_typed_array(glb.bytes(), json_start, Some(content_length)))?;
    add_pipeline_extras(&mut json)?;
    let mut gltf = Gltf::new(json);
    let binary = subarray(glb, binary_start, length);
    let buffers = gltf.json.get("buffers");
    if js::defined(buffers) && js::key_count(buffers) > 0 {
        let key = ["binary_glTF", "KHR_binary_glTF"].into_iter().find(|k| js::defined(js::get(buffers, k)));
        if let Some(key) = key {
            let marker = gltf.add_source(binary);
            let buffer = js::get_mut(gltf.json.get_mut("buffers"), key).expect("checked above");
            let pipeline = js::object_mut(buffer.pointer_mut("/extras/_pipeline"), "source")?;
            js::set(pipeline, "source", marker)?;
            js::delete(buffer, "uri");
        }
    }
    remove_extensions_used(&mut gltf.json, "KHR_binary_glTF");
    Ok(gltf)
}

fn parse_version2(glb: &BufferSource, header: &[u32]) -> Result<Option<Gltf>> {
    let length = header[2] as usize;
    let mut byte_offset = 12;
    let mut json = None;
    let mut binary = None;
    while byte_offset < length {
        let chunk_header = read_header(glb, byte_offset, 2)?;
        let (chunk_length, chunk_type) = (chunk_header[0] as usize, chunk_header[1]);
        byte_offset += 8;
        let chunk = subarray(glb, byte_offset, byte_offset + chunk_length);
        byte_offset += chunk_length;
        if chunk_type == JSON_CHUNK {
            let mut parsed = parse_json(&get_string_from_typed_array(chunk.bytes(), 0, None))?;
            add_pipeline_extras(&mut parsed)?;
            json = Some(parsed);
        } else if chunk_type == BIN_CHUNK {
            binary = Some(chunk);
        }
    }
    let Some(json) = json else { return Ok(None) };
    let mut gltf = Gltf::new(json);
    if let Some(binary) = binary {
        let has_buffers = match gltf.json.get("buffers") {
            Some(Value::Array(buffers)) => !buffers.is_empty(),
            Some(Value::String(s)) => !s.is_empty(),
            _ => false,
        };
        if has_buffers {
            let marker = gltf.add_source(binary);
            let buffer = js::get_mut(gltf.json.get_mut("buffers"), "0");
            let pipeline =
                js::object_mut(js::object_mut(buffer, "extras")?.pointer_mut("/extras/_pipeline"), "source")?;
            js::set(pipeline, "source", marker)?;
        }
    }
    Ok(Some(gltf))
}
