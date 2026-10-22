//! CesiumJS `decodeDraco` worker.
//!
//! Decodes a Draco triangular mesh or point cloud with `draco-core` 2.3.2,
//! the same decoder-only crate the I3S worker uses. This module does not share
//! code with `workers::decode_i3s`.
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
//! that dequantized result.

use crate::core::math::SIXTY_FOUR_KILOBYTES;
use crate::core::{ComponentDatatype, IndexDatatype, RuntimeError};
use draco_core::geometry_attribute::GeometryAttributeType;
use draco_core::{
    DataType, DecoderBuffer, Mesh, MeshDecoder, PointAttribute, PointCloud, PointCloudDecoder, PointIndex,
};
use serde_json::{Map, Number, Value};

/// Draco file header: 5-byte magic, major, minor, geometry type, method, flags.
const HEADER_BYTES: usize = 11;
const POINT_CLOUD: u8 = 0;
const TRIANGULAR_MESH: u8 = 1;

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
    if geometry_type(&bytes) != Some(TRIANGULAR_MESH) {
        return Err(RuntimeError::new("Unsupported draco mesh geometry type."));
    }
    let mut mesh = Mesh::new();
    let mut buffer = DecoderBuffer::new(&bytes);
    MeshDecoder::new()
        .decode(&mut buffer, &mut mesh)
        .map_err(|error| RuntimeError::new(format!("Error decoding draco mesh geometry: {}", error.message())))?;

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
        attribute_data.insert(name.clone(), attribute_json(attribute, mesh.num_points())?);
    }

    Ok(serde_json::json!({
        "indexArray": index_array(&mesh),
        "attributeData": Value::Object(attribute_data),
    }))
}

fn decode_point_cloud(parameters: &Map<String, Value>) -> Result<Value, RuntimeError> {
    let absent = Value::Null;
    let bytes = read_bytes(parameters.get("buffer").unwrap_or(&absent), None)?;
    if geometry_type(&bytes) != Some(POINT_CLOUD) {
        return Err(RuntimeError::new("Draco geometry type must be POINT_CLOUD."));
    }
    let mut cloud = PointCloud::new();
    let mut buffer = DecoderBuffer::new(&bytes);
    PointCloudDecoder::new()
        .decode(&mut buffer, &mut cloud)
        .map_err(|error| RuntimeError::new(format!("Error decoding draco point cloud: {}", error.message())))?;

    let properties = parameters
        .get("properties")
        .and_then(Value::as_object)
        .ok_or_else(|| RuntimeError::new("Draco properties must be an object"))?;
    let mut result = Map::new();
    for (name, id) in properties {
        let attribute = if name == "POSITION" || name == "NORMAL" {
            named_attribute(
                &cloud,
                if name == "POSITION" { GeometryAttributeType::Position } else { GeometryAttributeType::Normal },
            )?
        } else {
            let unique_id = json_u32(id)?;
            cloud
                .attribute_by_unique_id(unique_id)
                .ok_or_else(|| RuntimeError::new(format!("Missing Draco attribute {unique_id}")))?
        };
        result.insert(name.clone(), attribute_json(attribute, cloud.num_points())?);
    }
    Ok(Value::Object(result))
}

fn named_attribute(cloud: &PointCloud, kind: GeometryAttributeType) -> Result<&PointAttribute, RuntimeError> {
    let id = cloud.named_attribute_id(kind);
    if id < 0 {
        return Err(RuntimeError::new("Missing Draco attribute"));
    }
    cloud.try_attribute(id).map_err(|error| RuntimeError::new(error.message().to_string()))
}

fn json_u32(value: &Value) -> Result<u32, RuntimeError> {
    value
        .as_u64()
        .and_then(|n| u32::try_from(n).ok())
        .ok_or_else(|| RuntimeError::new("Draco attribute id must be an integer"))
}

fn index_array(mesh: &Mesh) -> Value {
    let mut indices = Vec::with_capacity(mesh.num_faces().saturating_mul(3));
    for face in mesh.faces() {
        indices.extend(face.iter().map(|point| point.0));
    }
    serde_json::json!({
        "typedArray": indices,
        "numberOfIndices": indices.len(),
        "indexDatatype": index_datatype(mesh.num_points()),
    })
}

fn attribute_json(attribute: &PointAttribute, num_points: usize) -> Result<Value, RuntimeError> {
    let components = attribute.num_components() as usize;
    let datatype = component_datatype(attribute.data_type());
    let stored =
        ComponentDatatype::from_gl(datatype).ok_or_else(|| RuntimeError::new("Unsupported Draco attribute type"))?;
    let byte_stride = stored.get_size_in_bytes().saturating_mul(components);
    Ok(serde_json::json!({
        "array": attribute_values(attribute, num_points, components)?,
        "data": {
            "componentsPerAttribute": components,
            "componentDatatype": datatype,
            "byteOffset": 0,
            "byteStride": byte_stride,
            "normalized": attribute.normalized(),
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
    attribute: &PointAttribute,
    num_points: usize,
    components: usize,
) -> Result<Vec<Value>, RuntimeError> {
    let mut values = Vec::with_capacity(num_points.saturating_mul(components));
    if components == 0 {
        return Ok(values);
    }
    let stride = usize::try_from(attribute.byte_stride()).unwrap_or(0);
    let width = attribute.data_type().byte_length();
    let data = attribute.buffer().data();
    for point in 0..num_points {
        let value_index = attribute.mapped_index(PointIndex(point as u32)).0;
        if value_index == u32::MAX {
            values.extend((0..components).map(|_| Value::from(0)));
            continue;
        }
        let start = (value_index as usize).saturating_mul(stride);
        for component in 0..components {
            let offset = start.saturating_add(component.saturating_mul(width));
            values.push(scalar_value(
                attribute.data_type(),
                data.get(offset..offset.saturating_add(width)).unwrap_or(&[]),
            )?);
        }
    }
    Ok(values)
}

fn scalar_value(data_type: DataType, bytes: &[u8]) -> Result<Value, RuntimeError> {
    if data_type != DataType::Invalid && bytes.len() < data_type.byte_length() {
        return Err(RuntimeError::new("Draco attribute is shorter than its values"));
    }
    Ok(match data_type {
        DataType::Invalid => Value::from(0),
        DataType::Int8 | DataType::Bool => Value::from(bytes[0] as i8),
        DataType::Uint8 => Value::from(bytes[0]),
        DataType::Int16 => Value::from(i16::from_le_bytes([bytes[0], bytes[1]])),
        DataType::Uint16 => Value::from(u16::from_le_bytes([bytes[0], bytes[1]])),
        DataType::Int32 => Value::from(i32::from_le_bytes(bytes[..4].try_into().unwrap())),
        DataType::Uint32 => Value::from(u32::from_le_bytes(bytes[..4].try_into().unwrap())),
        DataType::Int64 => Value::from(i64::from_le_bytes(bytes[..8].try_into().unwrap()) as i32),
        DataType::Uint64 => Value::from(u64::from_le_bytes(bytes[..8].try_into().unwrap()) as u32),
        DataType::Float32 => float_value(f32::from_le_bytes(bytes[..4].try_into().unwrap()) as f64),
        DataType::Float64 => float_value(f64::from_le_bytes(bytes[..8].try_into().unwrap()) as f32 as f64),
    })
}

fn float_value(value: f64) -> Value {
    Number::from_f64(value).map(Value::Number).unwrap_or(Value::Null)
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
