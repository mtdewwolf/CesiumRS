//! Binary I3S geometry and Draco (`DRACO` magic) attribute decoding.
//!
//! The binary layout is a little-endian vertex count, a feature count, then the
//! attributes named by `bufferInfo` or by the schema. A read that would throw
//! from a JavaScript typed-array view stops the walk and keeps what was decoded.
//! Unknown `bufferInfo` names are skipped without moving the cursor. Draco
//! payloads go through `workers::draco`. This module still applies semantic
//! names and `i3s-attribute-type` metadata, and it still drops 64-bit values.

use super::components::Values;
use crate::workers::draco::{self, AttributeValues, GeometryAttributeType, Metadata};

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
    // Wrong geometry, a decoder error, and a caught panic are all the empty
    // geometry. The header check and `catch_unwind` live in the shared helper.
    let Ok(mesh) = draco::decode_triangular_mesh(data) else {
        return empty_draco();
    };
    let mut geometry = empty_draco();
    geometry.vertex_count = mesh.num_points as u32;
    geometry.indices = Some(Values::U32(mesh.indices));

    for attribute in mesh.attributes {
        let mut name = semantic_name(attribute.attribute_type);
        if let Some(metadata) = attribute.metadata.as_ref() {
            apply_metadata(&mut geometry, metadata, &mut name);
        }
        let Some(values) = i3s_values(attribute.values) else {
            // INT64, UINT64, and FLOAT64 handlers return undefined, and the
            // assignment still replaces any previous value of this name.
            // `featureCount++` runs when that name is `feature-index`, but
            // decodeI3S never reads that counter after the Draco loop.
            geometry.attributes.retain(|(existing, _)| existing != &name);
            continue;
        };
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

fn apply_metadata(geometry: &mut DecodedGeometry, metadata: &Metadata, name: &mut String) {
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

fn i3s_values(values: AttributeValues) -> Option<Values> {
    let values = draco::defined_for_i3s(values)?;
    match values {
        AttributeValues::F32(values) => Some(Values::F32(values)),
        AttributeValues::I8(values) => Some(Values::I8(values)),
        AttributeValues::U8(values) => Some(Values::U8(values)),
        AttributeValues::I16(values) => Some(Values::I16(values)),
        AttributeValues::U16(values) => Some(Values::U16(values)),
        AttributeValues::I32(values) => Some(Values::I32(values)),
        AttributeValues::U32(values) => Some(Values::U32(values)),
        AttributeValues::I64(_)
        | AttributeValues::U64(_)
        | AttributeValues::F64(_)
        | AttributeValues::Invalid
        | AttributeValues::Unreadable => None,
    }
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

fn walk(names: &[String], geometry: &mut DecodedGeometry, feature_count: u32, data: &[u8], offset: &mut usize) -> bool {
    for name in names {
        let Ok(next) = decode_attribute(name, geometry, feature_count, data, *offset) else {
            return false;
        };
        *offset = next;
    }
    true
}

fn is_known(name: &str) -> bool {
    matches!(name, "position" | "normal" | "uv0" | "color" | "featureId" | "id" | "faceRange" | "uvRegion" | "region")
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
        "position" => push_view(
            geometry,
            "positions",
            data,
            offset,
            ViewShape { major: vertices, components: 3, align: 4, kind: Kind::F32 },
        ),
        "normal" => push_view(
            geometry,
            "normals",
            data,
            offset,
            ViewShape { major: vertices, components: 3, align: 4, kind: Kind::F32 },
        ),
        "uv0" => push_view(
            geometry,
            "uv0s",
            data,
            offset,
            ViewShape { major: vertices, components: 2, align: 4, kind: Kind::F32 },
        ),
        "color" => push_view(
            geometry,
            "colors",
            data,
            offset,
            ViewShape { major: vertices, components: 4, align: 1, kind: Kind::U8 },
        ),
        "featureId" | "id" => offset.checked_add(features.checked_mul(8).ok_or(())?).ok_or(()),
        "faceRange" => push_view(
            geometry,
            "faceRange",
            data,
            offset,
            ViewShape { major: features, components: 2, align: 4, kind: Kind::U32 },
        ),
        "uvRegion" | "region" => push_view(
            geometry,
            "uv-region",
            data,
            offset,
            ViewShape { major: vertices, components: 4, align: 2, kind: Kind::U16 },
        ),
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

struct ViewShape {
    major: usize,
    components: usize,
    align: usize,
    kind: Kind,
}

fn push_view(
    geometry: &mut DecodedGeometry,
    name: &str,
    data: &[u8],
    offset: usize,
    shape: ViewShape,
) -> Result<usize, ()> {
    let count = shape.major.checked_mul(shape.components).ok_or(())?;
    let (values, next) = read_view(data, offset, count, shape.align, shape.kind)?;
    if let Some((_, existing)) = geometry.attributes.iter_mut().find(|(current, _)| current == name) {
        *existing = values;
    } else {
        geometry.attributes.push((name.to_string(), values));
    }
    Ok(next)
}

fn read_view(data: &[u8], offset: usize, count: usize, align: usize, kind: Kind) -> Result<(Values, usize), ()> {
    if !offset.is_multiple_of(align) {
        return Err(());
    }
    let bytes = count.checked_mul(align).ok_or(())?;
    let end = offset.checked_add(bytes).ok_or(())?;
    let slice = data.get(offset..end).ok_or(())?;
    let values = match kind {
        Kind::F32 => Values::F32(slice.as_chunks::<4>().0.iter().copied().map(f32::from_le_bytes).collect()),
        Kind::U8 => Values::U8(slice.to_vec()),
        Kind::U16 => Values::U16(slice.as_chunks::<2>().0.iter().copied().map(u16::from_le_bytes).collect()),
        Kind::U32 => Values::U32(slice.as_chunks::<4>().0.iter().copied().map(u32::from_le_bytes).collect()),
    };
    Ok((values, end))
}
