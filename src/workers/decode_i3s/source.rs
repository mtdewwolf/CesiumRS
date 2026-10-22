//! Binary I3S geometry and Draco (`DRACO` magic) attribute decoding.
//!
//! The binary layout is a little-endian vertex count, a feature count, then the
//! attributes named by `bufferInfo` or by the schema. A read that would throw
//! from a JavaScript typed-array view stops the walk and keeps what was decoded.
//! Unknown `bufferInfo` names are skipped without moving the cursor. Draco
//! attributes are read in attribute-id order, already dequantized to the stored
//! data type, and may be renamed by `i3s-attribute-type` metadata.

use super::components::Values;
use draco_core::geometry_attribute::GeometryAttributeType;
use draco_core::geometry_indices::PointIndex;
use draco_core::{DataType, DecoderBuffer, Mesh, MeshDecoder, PointAttribute};

#[derive(Clone, Debug)]
pub(super) struct DecodedGeometry {
    pub(super) vertex_count: u32,
    /// Draco stores `vertexCount` as a one-element array, so `=== 0` is never true.
    pub(super) vertex_count_is_array: bool,
    pub(super) scale_x: f64,
    pub(super) scale_y: f64,
    pub(super) indices: Option<Values>,
    pub(super) attributes: Vec<(String, Values)>,
}

impl DecodedGeometry {
    pub(super) fn take(&mut self, name: &str) -> Option<Values> {
        let position = self.attributes.iter().position(|(n, _)| n == name)?;
        Some(self.attributes.remove(position).1)
    }

    pub(super) fn get(&self, name: &str) -> Option<&Values> {
        self.attributes.iter().find(|(n, _)| n == name).map(|(_, v)| v)
    }
}

pub(super) fn decode_geometry(
    data: &[u8],
    ordering: Option<&[String]>,
    feature_order: Option<&[String]>,
    buffer_attributes: Option<&[String]>,
) -> Result<DecodedGeometry, ()> {
    if data.len() < 5 {
        // `new Uint8Array(data, 0, 5)` throws before the binary decoder's try/catch.
        return Err(());
    }
    if data.starts_with(b"DRACO") {
        return Ok(decode_draco(data));
    }
    Ok(decode_binary(data, ordering, feature_order, buffer_attributes))
}

fn decode_draco(data: &[u8]) -> DecodedGeometry {
    let mut geometry = empty_draco();
    let mut buffer = DecoderBuffer::new(data);
    let mut mesh = Mesh::new();
    if MeshDecoder::new().decode(&mut buffer, &mut mesh).is_err() {
        return geometry;
    }
    let vertex_count = mesh.num_points();
    geometry.vertex_count = vertex_count as u32;
    let mut indices = Vec::with_capacity(mesh.num_faces() * 3);
    for face in mesh.faces() {
        indices.extend(face.iter().map(|point| point.0));
    }
    geometry.indices = Some(Values::U32(indices));

    for attr_index in 0..mesh.num_attributes() {
        let Ok(attribute) = mesh.try_attribute(attr_index) else {
            continue;
        };
        let Some(values) = read_draco_attribute(attribute, vertex_count) else {
            continue;
        };
        let mut name = semantic_name(attribute.attribute_type());
        if let Some(metadata) = mesh.attribute_metadata_by_unique_id(attribute.unique_id()) {
            apply_metadata(&mut geometry, metadata.metadata(), &mut name);
        }
        if let Some((_, existing)) = geometry.attributes.iter_mut().find(|(n, _)| *n == name) {
            *existing = values;
        } else {
            geometry.attributes.push((name, values));
        }
    }
    geometry
}

fn empty_draco() -> DecodedGeometry {
    DecodedGeometry {
        vertex_count: 0,
        vertex_count_is_array: true,
        scale_x: 1.0,
        scale_y: 1.0,
        indices: None,
        attributes: Vec::new(),
    }
}

fn semantic_name(kind: GeometryAttributeType) -> String {
    match kind {
        GeometryAttributeType::Position => "positions",
        GeometryAttributeType::Normal => "normals",
        GeometryAttributeType::Color => "colors",
        GeometryAttributeType::TexCoord => "uv0s",
        GeometryAttributeType::Invalid | GeometryAttributeType::Generic => "unknown",
    }
    .to_string()
}

fn apply_metadata(geometry: &mut DecodedGeometry, metadata: &draco_core::Metadata, name: &mut String) {
    for entry in metadata.entries().keys() {
        if entry == "i3s-scale_x" {
            geometry.scale_x = metadata.get_f64(entry).unwrap_or(0.0);
        } else if entry == "i3s-scale_y" {
            geometry.scale_y = metadata.get_f64(entry).unwrap_or(0.0);
        } else if entry == "i3s-attribute-type" {
            *name = metadata.get_string(entry).unwrap_or("").to_string();
        }
    }
}

fn read_draco_attribute(attribute: &PointAttribute, vertex_count: usize) -> Option<Values> {
    let components = attribute.num_components() as usize;
    let count = components.checked_mul(vertex_count)?;
    match attribute.data_type() {
        DataType::Invalid | DataType::Int64 | DataType::Uint64 | DataType::Float64 => None,
        DataType::Float32 => Some(Values::F32(read_mapped(attribute, count, read_f32))),
        DataType::Int8 => Some(Values::I8(read_mapped(attribute, count, read_i8))),
        DataType::Uint8 | DataType::Bool => Some(Values::U8(read_mapped(attribute, count, read_u8))),
        DataType::Int16 => Some(Values::I16(read_mapped(attribute, count, read_i16))),
        DataType::Uint16 => Some(Values::U16(read_mapped(attribute, count, read_u16))),
        DataType::Int32 => Some(Values::I32(read_mapped(attribute, count, read_i32))),
        DataType::Uint32 => Some(Values::U32(read_mapped(attribute, count, read_u32))),
    }
}

fn read_mapped<T: Copy + Default>(attribute: &PointAttribute, count: usize, read: fn(&[u8]) -> T) -> Vec<T> {
    let components = attribute.num_components() as usize;
    if components == 0 {
        return vec![T::default(); count];
    }
    let width = attribute.data_type().byte_length();
    let stride = attribute.byte_stride() as usize;
    let data = attribute.buffer().data();
    let mut out = vec![T::default(); count];
    for (element, slot) in out.iter_mut().enumerate() {
        let point = element / components;
        let component = element % components;
        let value_index = attribute.mapped_index(PointIndex(point as u32)).0 as usize;
        let Some(offset) = value_index.checked_mul(stride).and_then(|base| base.checked_add(component * width)) else {
            continue;
        };
        if let Some(bytes) = data.get(offset..offset + width) {
            *slot = read(bytes);
        }
    }
    out
}

fn read_f32(bytes: &[u8]) -> f32 {
    f32::from_le_bytes(bytes.try_into().unwrap_or([0; 4]))
}
fn read_i8(bytes: &[u8]) -> i8 {
    bytes.first().copied().unwrap_or(0) as i8
}
fn read_u8(bytes: &[u8]) -> u8 {
    bytes.first().copied().unwrap_or(0)
}
fn read_i16(bytes: &[u8]) -> i16 {
    i16::from_le_bytes(bytes.try_into().unwrap_or([0; 2]))
}
fn read_u16(bytes: &[u8]) -> u16 {
    u16::from_le_bytes(bytes.try_into().unwrap_or([0; 2]))
}
fn read_i32(bytes: &[u8]) -> i32 {
    i32::from_le_bytes(bytes.try_into().unwrap_or([0; 4]))
}
fn read_u32(bytes: &[u8]) -> u32 {
    u32::from_le_bytes(bytes.try_into().unwrap_or([0; 4]))
}

fn decode_binary(
    data: &[u8],
    ordering: Option<&[String]>,
    feature_order: Option<&[String]>,
    buffer_attributes: Option<&[String]>,
) -> DecodedGeometry {
    let mut geometry = DecodedGeometry {
        vertex_count: 0,
        vertex_count_is_array: false,
        scale_x: 1.0,
        scale_y: 1.0,
        indices: None,
        attributes: Vec::new(),
    };
    // `DataView.getUint32` throws, and the binary decoder keeps whatever it
    // wrote before that throw. A 4-byte buffer therefore has a vertex count
    // and no feature count.
    let Some(vertex_bytes) = data.get(0..4) else {
        return geometry;
    };
    geometry.vertex_count = u32::from_le_bytes(vertex_bytes.try_into().unwrap());
    let Some(feature_bytes) = data.get(4..8) else {
        return geometry;
    };
    let feature_count = u32::from_le_bytes(feature_bytes.try_into().unwrap());
    let mut offset = 8usize;
    if let Some(attributes) = buffer_attributes {
        for name in attributes {
            if !is_known(name) {
                continue;
            }
            let Ok(next) = decode_attribute(name, &mut geometry, feature_count, data, offset) else {
                break;
            };
            offset = next;
        }
    } else if let (Some(ordering), Some(feature_order)) = (ordering, feature_order) {
        if walk(ordering, &mut geometry, feature_count, data, &mut offset) {
            let _ = walk(feature_order, &mut geometry, feature_count, data, &mut offset);
        }
    }
    geometry
}

fn walk(
    names: &[String],
    geometry: &mut DecodedGeometry,
    feature_count: u32,
    data: &[u8],
    offset: &mut usize,
) -> bool {
    for name in names {
        let Ok(next) = decode_attribute(name, geometry, feature_count, data, *offset) else {
            return false;
        };
        *offset = next;
    }
    true
}

fn is_known(name: &str) -> bool {
    matches!(
        name,
        "position" | "normal" | "uv0" | "color" | "featureId" | "id" | "faceRange" | "uvRegion" | "region"
    )
}

fn decode_attribute(
    name: &str,
    geometry: &mut DecodedGeometry,
    feature_count: u32,
    data: &[u8],
    offset: usize,
) -> Result<usize, ()> {
    let vertices = geometry.vertex_count as usize;
    let features = feature_count as usize;
    match name {
        "position" => push_view(geometry, "positions", data, offset, vertices, 3, 4, Kind::F32),
        "normal" => push_view(geometry, "normals", data, offset, vertices, 3, 4, Kind::F32),
        "uv0" => push_view(geometry, "uv0s", data, offset, vertices, 2, 4, Kind::F32),
        "color" => push_view(geometry, "colors", data, offset, vertices, 4, 1, Kind::U8),
        "featureId" | "id" => offset.checked_add(features.checked_mul(8).ok_or(())?).ok_or(()),
        "faceRange" => push_view(geometry, "faceRange", data, offset, features, 2, 4, Kind::U32),
        "uvRegion" | "region" => push_view(geometry, "uv-region", data, offset, vertices, 4, 2, Kind::U16),
        _ => Err(()),
    }
}

#[derive(Clone, Copy)]
enum Kind {
    F32,
    U8,
    U16,
    U32,
}

fn push_view(
    geometry: &mut DecodedGeometry,
    name: &str,
    data: &[u8],
    offset: usize,
    major: usize,
    components: usize,
    align: usize,
    kind: Kind,
) -> Result<usize, ()> {
    let count = major.checked_mul(components).ok_or(())?;
    let (values, next) = read_view(data, offset, count, align, kind)?;
    if let Some((_, existing)) = geometry.attributes.iter_mut().find(|(current, _)| current == name) {
        *existing = values;
    } else {
        geometry.attributes.push((name.to_string(), values));
    }
    Ok(next)
}

fn read_view(data: &[u8], offset: usize, count: usize, align: usize, kind: Kind) -> Result<(Values, usize), ()> {
    if offset % align != 0 {
        return Err(());
    }
    let bytes = count.checked_mul(align).ok_or(())?;
    let end = offset.checked_add(bytes).ok_or(())?;
    let slice = data.get(offset..end).ok_or(())?;
    let values = match kind {
        Kind::F32 => Values::F32(slice.chunks_exact(4).map(|c| f32::from_le_bytes(c.try_into().unwrap())).collect()),
        Kind::U8 => Values::U8(slice.to_vec()),
        Kind::U16 => Values::U16(slice.chunks_exact(2).map(|c| u16::from_le_bytes(c.try_into().unwrap())).collect()),
        Kind::U32 => Values::U32(slice.chunks_exact(4).map(|c| u32::from_le_bytes(c.try_into().unwrap())).collect()),
    };
    Ok((values, end))
}
