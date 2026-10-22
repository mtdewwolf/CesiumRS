//! CesiumJS `decodeDraco` worker.
//!
//! Decodes a Draco triangular mesh or point cloud through `workers::draco`,
//! the helper extracted from `workers::decode_i3s`. Both workers use
//! `draco-core` 2.3.2 and no other Draco crate.
//!
//! The WebAssembly initialization message returns `true`: there is no Draco
//! module to compile. A buffer whose header is a triangular mesh is decoded
//! as one; a point cloud is decoded as one. Any other header, including a
//! truncated header, raises the worker's geometry-type `RuntimeError` before
//! the bitstream is decoded. A header of the right kind that then fails to
//! decode raises `Error decoding draco mesh geometry:` or
//! `Error decoding draco point cloud:` plus the decoder's message.
//!
//! `dequantizeInShader` skips Draco's attribute transforms through the
//! vendored decoder's `SetSkipAttributeTransform`: every listed
//! `attributesToSkipTransform` type on a mesh, and `POSITION` and `NORMAL` on
//! a point cloud. A skipped quantized or octahedral attribute returns its
//! portable integers through the worker's `UInt8` / `UInt16` / `Float32` view
//! (chosen by quantization bits) and `{ quantizationBits, minValues, range,
//! octEncoded }`; octahedral normals keep two components. A skipped
//! integer-encoded attribute becomes Draco's `Int32` portable copy, as in
//! upstream. A name that is not a Draco attribute type reaches
//! `SkipAttributeTransform` as `undefined`, which Draco receives as 0 and so
//! skips `POSITION`.

use super::draco::{
    self, AttributeValues, DracoAttribute, DracoDecodeError, DracoGeometry, GeometryAttributeType,
    SkipAttributeTransforms, SkippedTransform,
};
use crate::core::math::SIXTY_FOUR_KILOBYTES;
use crate::core::{ComponentDatatype, IndexDatatype, RuntimeError};
use draco_core::DataType;
use serde_json::{Map, Number, Value};

/// Draco file header: 5-byte magic, major, minor, geometry type, method, flags.
const HEADER_BYTES: usize = 11;

/// Missing attributes do not get a Draco status string. `GetAttribute` and
/// `GetAttributeByUniqueId` still return an object, its `data_type` misses the
/// typed-array switch, and `draco.destroy` is called on the array that switch
/// never created. The worker throws this TypeError for a missing unique id and
/// for a missing POSITION or NORMAL semantic.
const MISSING_ATTRIBUTE: &str = "Cannot read properties of undefined (reading '__destroy__')";

/// Runs the `decodeDraco` worker on one parameters object.
pub fn decode_draco(parameters: &Value) -> Result<Value, RuntimeError> {
    let parameters = parameters.as_object().ok_or_else(|| RuntimeError::new("Draco parameters must be an object"))?;
    if defined(parameters.get("webAssemblyConfig")) {
        return Ok(Value::Bool(true));
    }
    if defined(parameters.get("bufferView")) {
        decode_mesh(parameters)
    } else {
        decode_point_cloud(parameters)
    }
}

/// `IndexDatatype.createTypedArray`: unsigned short below 64K vertices.
fn index_datatype(num_points: usize) -> u32 {
    if num_points >= SIXTY_FOUR_KILOBYTES as usize {
        IndexDatatype::UnsignedInt as u32
    } else {
        IndexDatatype::UnsignedShort as u32
    }
}

fn defined(value: Option<&Value>) -> bool {
    !matches!(value, None | Some(Value::Null))
}

/// JavaScript truthiness of a structured-cloned parameter.
fn truthy(value: Option<&Value>) -> bool {
    match value {
        None | Some(Value::Null) => false,
        Some(Value::Bool(b)) => *b,
        Some(Value::Number(n)) => n.as_f64().is_some_and(|n| n != 0.0 && !n.is_nan()),
        Some(Value::String(s)) => !s.is_empty(),
        Some(_) => true,
    }
}

/// `draco[name]` for `SkipAttributeTransform`. `INVALID` (-1) skips nothing;
/// any other name is `undefined`, which reaches Draco as 0 (`POSITION`).
fn attribute_type_constant(name: &Value) -> Option<GeometryAttributeType> {
    match name.as_str() {
        Some("INVALID") => None,
        Some("NORMAL") => Some(GeometryAttributeType::Normal),
        Some("COLOR") => Some(GeometryAttributeType::Color),
        Some("TEX_COORD") => Some(GeometryAttributeType::TexCoord),
        Some("GENERIC") => Some(GeometryAttributeType::Generic),
        _ => Some(GeometryAttributeType::Position),
    }
}

fn geometry_type(bytes: &[u8]) -> Option<u8> {
    if bytes.len() < HEADER_BYTES || &bytes[..5] != b"DRACO" {
        return None;
    }
    Some(bytes[7])
}

fn byte_length(value: &Value) -> Result<usize, RuntimeError> {
    value
        .as_u64()
        .and_then(|n| usize::try_from(n).ok())
        .ok_or_else(|| RuntimeError::new("Draco bufferView.byteLength must be a byte count"))
}

fn read_bytes(value: &Value, length: Option<usize>) -> Result<Vec<u8>, RuntimeError> {
    let items = value.as_array().ok_or_else(|| RuntimeError::new("Draco buffer must be a byte array"))?;
    let mut bytes = Vec::with_capacity(items.len());
    for item in items {
        let byte = item
            .as_u64()
            .filter(|n| *n <= 255)
            .ok_or_else(|| RuntimeError::new("Draco buffer byte is out of range"))?;
        bytes.push(byte as u8);
    }
    if let Some(length) = length {
        if length > bytes.len() {
            return Err(RuntimeError::new("Draco bufferView is longer than its array"));
        }
        bytes.truncate(length);
    }
    Ok(bytes)
}

fn decode_mesh(parameters: &Map<String, Value>) -> Result<Value, RuntimeError> {
    let view = parameters
        .get("bufferView")
        .and_then(Value::as_object)
        .ok_or_else(|| RuntimeError::new("Draco bufferView must be an object"))?;
    let absent = Value::Null;
    let length = byte_length(view.get("byteLength").unwrap_or(&absent))?;
    let bytes = read_bytes(parameters.get("array").unwrap_or(&absent), Some(length))?;
    if geometry_type(&bytes) != Some(draco::TRIANGULAR_MESH) {
        return Err(RuntimeError::new("Unsupported draco mesh geometry type."));
    }
    let mut skip = SkipAttributeTransforms::default();
    if truthy(parameters.get("dequantizeInShader")) {
        let names = parameters
            .get("attributesToSkipTransform")
            .and_then(Value::as_array)
            .ok_or_else(|| RuntimeError::new("Cannot read properties of undefined (reading 'length')"))?;
        for name in names {
            if let Some(kind) = attribute_type_constant(name) {
                skip.insert(kind);
            }
        }
    }
    let mesh = mesh_or_error(draco::decode_triangular_mesh_skipping(&bytes, skip))?;

    let compressed = parameters
        .get("compressedAttributes")
        .and_then(Value::as_object)
        .ok_or_else(|| RuntimeError::new("Draco compressedAttributes must be an object"))?;
    let mut attribute_data = Map::new();
    for (name, id) in compressed {
        let unique_id = json_u32(id)?;
        let attribute = mesh.attribute_by_unique_id(unique_id).ok_or_else(|| RuntimeError::new(MISSING_ATTRIBUTE))?;
        attribute_data.insert(name.clone(), attribute_json(attribute, mesh.num_points)?);
    }

    Ok(serde_json::json!({
        "indexArray": index_array(&mesh),
        "attributeData": Value::Object(attribute_data),
    }))
}

fn decode_point_cloud(parameters: &Map<String, Value>) -> Result<Value, RuntimeError> {
    let absent = Value::Null;
    let bytes = read_bytes(parameters.get("buffer").unwrap_or(&absent), None)?;
    if geometry_type(&bytes) != Some(draco::POINT_CLOUD) {
        return Err(RuntimeError::new("Draco geometry type must be POINT_CLOUD."));
    }
    let mut skip = SkipAttributeTransforms::default();
    if truthy(parameters.get("dequantizeInShader")) {
        skip.insert(GeometryAttributeType::Position);
        skip.insert(GeometryAttributeType::Normal);
    }
    let cloud = cloud_or_error(draco::decode_point_cloud_skipping(&bytes, skip))?;

    let properties = parameters
        .get("properties")
        .and_then(Value::as_object)
        .ok_or_else(|| RuntimeError::new("Draco properties must be an object"))?;
    let mut result = Map::new();
    for (name, id) in properties {
        let attribute = if name == "POSITION" || name == "NORMAL" {
            let kind = if name == "POSITION" { GeometryAttributeType::Position } else { GeometryAttributeType::Normal };
            cloud.named_attribute(kind).ok_or_else(|| RuntimeError::new(MISSING_ATTRIBUTE))?
        } else {
            let unique_id = json_u32(id)?;
            cloud.attribute_by_unique_id(unique_id).ok_or_else(|| RuntimeError::new(MISSING_ATTRIBUTE))?
        };
        result.insert(name.clone(), attribute_json(attribute, cloud.num_points)?);
    }
    Ok(Value::Object(result))
}

fn mesh_or_error(decoded: Result<DracoGeometry, DracoDecodeError>) -> Result<DracoGeometry, RuntimeError> {
    match decoded {
        Ok(geometry) => Ok(geometry),
        Err(DracoDecodeError::WrongGeometry) => Err(RuntimeError::new("Unsupported draco mesh geometry type.")),
        Err(DracoDecodeError::Failed(message)) => {
            Err(RuntimeError::new(format!("Error decoding draco mesh geometry: {message}")))
        }
    }
}

fn cloud_or_error(decoded: Result<DracoGeometry, DracoDecodeError>) -> Result<DracoGeometry, RuntimeError> {
    match decoded {
        Ok(geometry) => Ok(geometry),
        Err(DracoDecodeError::WrongGeometry) => Err(RuntimeError::new("Draco geometry type must be POINT_CLOUD.")),
        Err(DracoDecodeError::Failed(message)) => {
            Err(RuntimeError::new(format!("Error decoding draco point cloud: {message}")))
        }
    }
}

fn json_u32(value: &Value) -> Result<u32, RuntimeError> {
    value
        .as_u64()
        .and_then(|n| u32::try_from(n).ok())
        .ok_or_else(|| RuntimeError::new("Draco attribute id must be an integer"))
}

fn index_array(mesh: &DracoGeometry) -> Value {
    serde_json::json!({
        "typedArray": mesh.indices,
        "numberOfIndices": mesh.indices.len(),
        "indexDatatype": index_datatype(mesh.num_points),
    })
}

fn attribute_json(attribute: &DracoAttribute, num_points: usize) -> Result<Value, RuntimeError> {
    if let Some(transform) = &attribute.transform {
        return quantized_attribute_json(attribute, transform, num_points);
    }
    let components = attribute.components as usize;
    let datatype = component_datatype(attribute.data_type);
    let stored =
        ComponentDatatype::from_gl(datatype).ok_or_else(|| RuntimeError::new("Unsupported Draco attribute type"))?;
    let byte_stride = stored.get_size_in_bytes().saturating_mul(components);
    // draco-core packs decoded values at the start of the attribute buffer.
    // `PointAttribute` has no public byte offset after decode. Cesium reads
    // `dracoAttribute.byte_offset()`, which is 0 for these files.
    Ok(serde_json::json!({
        "array": attribute_values(attribute, num_points, components)?,
        "data": {
            "componentsPerAttribute": components,
            "componentDatatype": datatype,
            "byteOffset": 0,
            "byteStride": byte_stride,
            "normalized": attribute.normalized,
            "quantization": Value::Null,
        }
    }))
}

/// `decodeQuantizedDracoTypedArray`: up to 8 bits read through
/// `GetAttributeUInt8ForAllPoints`, up to 16 through the `UInt16` getter, and
/// wider values as floats. The integer getters convert each portable value
/// and stop at the first one out of range, leaving the rest 0.
fn quantized_attribute_json(
    attribute: &DracoAttribute,
    transform: &SkippedTransform,
    num_points: usize,
) -> Result<Value, RuntimeError> {
    let components = attribute.components as usize;
    let portable: Vec<i64> = match &attribute.values {
        _ if attribute.short_value => return Err(RuntimeError::new("Draco attribute is shorter than its values")),
        AttributeValues::I32(values) => values.iter().map(|&v| i64::from(v)).collect(),
        AttributeValues::U32(values) => values.iter().map(|&v| i64::from(v)).collect(),
        _ => return Err(RuntimeError::new("Skipped Draco transforms keep 32-bit integer values")),
    };
    let quantization = match transform {
        SkippedTransform::Quantization { quantization_bits, min_values, range } => serde_json::json!({
            "quantizationBits": quantization_bits,
            "minValues": min_values.iter().map(|&v| float_value(f64::from(v))).collect::<Vec<_>>(),
            "range": float_value(f64::from(*range)),
            "octEncoded": false,
        }),
        SkippedTransform::Octahedron { quantization_bits } => serde_json::json!({
            "quantizationBits": quantization_bits,
            "octEncoded": true,
        }),
    };
    let bits = match transform {
        SkippedTransform::Quantization { quantization_bits, .. }
        | SkippedTransform::Octahedron { quantization_bits } => *quantization_bits,
    };
    let length = num_points.saturating_mul(components).min(portable.len());
    let (datatype, array) = if bits <= 8 {
        (ComponentDatatype::UnsignedByte, narrowed(&portable[..length], u8::MAX.into()))
    } else if bits <= 16 {
        (ComponentDatatype::UnsignedShort, narrowed(&portable[..length], u16::MAX.into()))
    } else {
        (ComponentDatatype::Float, portable[..length].iter().map(|&v| float_value(f64::from(v as f32))).collect())
    };
    Ok(serde_json::json!({
        "array": array,
        "data": {
            "componentsPerAttribute": components,
            "componentDatatype": datatype as u32,
            "byteOffset": 0,
            "byteStride": datatype.get_size_in_bytes().saturating_mul(components),
            "normalized": attribute.normalized,
            "quantization": quantization,
        }
    }))
}

fn narrowed(values: &[i64], max: i64) -> Vec<Value> {
    let mut failed = false;
    values
        .iter()
        .map(|&value| {
            failed |= !(0..=max).contains(&value);
            Value::from(if failed { 0 } else { value })
        })
        .collect()
}

/// Output element type. 64-bit and boolean storage follow the worker, which
/// asks Draco for a 32-bit or 8-bit view.
fn component_datatype(data_type: DataType) -> u32 {
    match data_type {
        DataType::Int8 | DataType::Bool => ComponentDatatype::Byte as u32,
        DataType::Uint8 => ComponentDatatype::UnsignedByte as u32,
        DataType::Int16 => ComponentDatatype::Short as u32,
        DataType::Uint16 => ComponentDatatype::UnsignedShort as u32,
        DataType::Int32 | DataType::Int64 => ComponentDatatype::Int as u32,
        DataType::Uint32 | DataType::Uint64 => ComponentDatatype::UnsignedInt as u32,
        DataType::Float32 | DataType::Float64 | DataType::Invalid => ComponentDatatype::Float as u32,
    }
}

fn attribute_values(
    attribute: &DracoAttribute,
    num_points: usize,
    components: usize,
) -> Result<Vec<Value>, RuntimeError> {
    if attribute.short_value {
        return Err(RuntimeError::new("Draco attribute is shorter than its values"));
    }
    if components == 0 {
        return Ok(Vec::new());
    }
    let values = match &attribute.values {
        // Cesium's typed-array switch has no `DT_INVALID` arm, so a live
        // attribute of that type would throw on `GetValue`. A successful
        // decode never yields one: patching the type byte to 0 makes
        // `DecodeBufferToPointCloud` fail first with "Failed to decode point
        // attributes.", and that status is already the worker error. This arm
        // is only the decoder reporting `Invalid` after the bitstream decoded.
        AttributeValues::Invalid => vec![Value::from(0); num_points.saturating_mul(components)],
        AttributeValues::Unreadable => {
            return Err(RuntimeError::new("Draco attribute is shorter than its values"));
        }
        AttributeValues::F32(values) => values.iter().copied().map(|value| float_value(f64::from(value))).collect(),
        AttributeValues::I8(values) => values.iter().copied().map(Value::from).collect(),
        AttributeValues::U8(values) if attribute.data_type == DataType::Bool => {
            values.iter().copied().map(|value| Value::from(value as i8)).collect()
        }
        AttributeValues::U8(values) => values.iter().copied().map(Value::from).collect(),
        AttributeValues::I16(values) => values.iter().copied().map(Value::from).collect(),
        AttributeValues::U16(values) => values.iter().copied().map(Value::from).collect(),
        AttributeValues::I32(values) => values.iter().copied().map(Value::from).collect(),
        AttributeValues::U32(values) => values.iter().copied().map(Value::from).collect(),
        // `GetAttributeInt32ForAllPoints` / `GetAttributeUInt32ForAllPoints`
        // return false at the first value that does not fit and leave the rest
        // of the zero-filled array unread. Values before that stay. That value
        // and everything after it stay 0, including a later value that would
        // fit: `[2^31, 5]` is `[0, 0]`, and `[5, 2^31, 7]` is `[5, 0, 0]`.
        AttributeValues::I64(values) => narrow_i64(values),
        AttributeValues::U64(values) => narrow_u64(values),
        AttributeValues::F64(values) => values.iter().copied().map(|value| float_value(value as f32 as f64)).collect(),
    };
    Ok(values)
}

fn float_value(value: f64) -> Value {
    Number::from_f64(value).map(Value::Number).unwrap_or(Value::Null)
}

fn narrow_i64(values: &[i64]) -> Vec<Value> {
    let mut failed = false;
    values
        .iter()
        .map(|value| match (failed, i32::try_from(*value)) {
            (false, Ok(narrowed)) => Value::from(narrowed),
            _ => {
                failed = true;
                Value::from(0)
            }
        })
        .collect()
}

fn narrow_u64(values: &[u64]) -> Vec<Value> {
    let mut failed = false;
    values
        .iter()
        .map(|value| match (failed, u32::try_from(*value)) {
            (false, Ok(narrowed)) => Value::from(narrowed),
            _ => {
                failed = true;
                Value::from(0)
            }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::index_datatype;

    #[test]
    fn index_width_follows_sixty_four_kilobytes() {
        assert_eq!(index_datatype(0), 5123);
        assert_eq!(index_datatype(65535), 5123);
        assert_eq!(index_datatype(65536), 5125);
    }
}
