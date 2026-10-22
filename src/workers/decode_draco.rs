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
//! `dequantizeInShader` is not reproduced. Cesium skips
//! `AttributeQuantizationTransform` / `AttributeOctahedronTransform` and
//! returns the portable integers plus `{ quantizationBits, minValues, range,
//! octEncoded }`. `draco-core` 2.3.2 has no skip API: `MeshDecoder` and
//! `PointCloudDecoder` always inverse-transform, and the decoded attribute's
//! `attribute_transform_data` is empty, so the integers and the parameters
//! are gone. This function therefore always returns the dequantized values
//! and `quantization: null`, which is what the worker returns when the flag
//! is absent or no transform is skipped. Callers that set the flag still get
//! that dequantized result. Model's Draco loader turns the flag on by
//! default (`GltfDracoLoader` sets `dequantizeInShader: true`, as do the
//! point-cloud and pnts parsers), so the most common production path differs
//! from CesiumJS.

use super::draco::{self, AttributeValues, DracoAttribute, DracoDecodeError, DracoGeometry, GeometryAttributeType};
use crate::core::math::SIXTY_FOUR_KILOBYTES;
use crate::core::{ComponentDatatype, IndexDatatype, RuntimeError};
use draco_core::DataType;
use serde_json::{Map, Number, Value};

/// Draco file header: 5-byte magic, major, minor, geometry type, method, flags.
const HEADER_BYTES: usize = 11;

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
    let mesh = mesh_or_error(draco::decode_triangular_mesh(&bytes))?;

    let compressed = parameters
        .get("compressedAttributes")
        .and_then(Value::as_object)
        .ok_or_else(|| RuntimeError::new("Draco compressedAttributes must be an object"))?;
    let mut attribute_data = Map::new();
    for (name, id) in compressed {
        let unique_id = json_u32(id)?;
        let attribute = mesh
            .attribute_by_unique_id(unique_id)
            .ok_or_else(|| RuntimeError::new(format!("Missing Draco attribute {unique_id}")))?;
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
    let cloud = cloud_or_error(draco::decode_point_cloud(&bytes))?;

    let properties = parameters
        .get("properties")
        .and_then(Value::as_object)
        .ok_or_else(|| RuntimeError::new("Draco properties must be an object"))?;
    let mut result = Map::new();
    for (name, id) in properties {
        let attribute = if name == "POSITION" || name == "NORMAL" {
            let kind = if name == "POSITION" { GeometryAttributeType::Position } else { GeometryAttributeType::Normal };
            cloud.named_attribute(kind).ok_or_else(|| RuntimeError::new("Missing Draco attribute"))?
        } else {
            let unique_id = json_u32(id)?;
            cloud
                .attribute_by_unique_id(unique_id)
                .ok_or_else(|| RuntimeError::new(format!("Missing Draco attribute {unique_id}")))?
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
        // return false when any value does not fit. The worker still reads
        // the array: values that fit are kept, and the rest stay 0.
        AttributeValues::I64(values) => values.iter().copied().map(|value| Value::from(narrow_i64(value))).collect(),
        AttributeValues::U64(values) => values.iter().copied().map(|value| Value::from(narrow_u64(value))).collect(),
        AttributeValues::F64(values) => values.iter().copied().map(|value| float_value(value as f32 as f64)).collect(),
    };
    Ok(values)
}

fn float_value(value: f64) -> Value {
    Number::from_f64(value).map(Value::Number).unwrap_or(Value::Null)
}

fn narrow_i64(value: i64) -> i32 {
    i32::try_from(value).unwrap_or(0)
}

fn narrow_u64(value: u64) -> u32 {
    u32::try_from(value).unwrap_or(0)
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
