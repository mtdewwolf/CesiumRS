//! Shared Draco decode for `decode_i3s` and `decode_draco`.
//!
//! Moved out of `workers::decode_i3s`: the geometry-type check, the decoder
//! call and its panic boundary, face-index flattening, and the mapped-index
//! attribute walk. Both workers enter through [`decode_triangular_mesh`] or
//! [`decode_point_cloud`]. A point cloud never reaches `MeshDecoder`.
//!
//! `decode_i3s` still applies semantic names and `i3s-*` metadata, and every
//! guard failure becomes its empty geometry. `decode_draco` still owns the
//! worker's geometry-type errors, unique-id lookup, and JSON. The walk is the
//! I3S one. A mapped value past the attribute buffer stays the type's zero,
//! and `short_value` records that the read was short.

use draco_core::geometry_indices::PointIndex;
use draco_core::{DataType, DecoderBuffer, Mesh, MeshDecoder, PointAttribute, PointCloud, PointCloudDecoder};
use std::panic::{catch_unwind, AssertUnwindSafe};

pub use draco_core::geometry_attribute::GeometryAttributeType;
pub use draco_core::Metadata;

/// Header geometry type `POINT_CLOUD`.
pub const POINT_CLOUD: u8 = 0;
/// Header geometry type `TRIANGULAR_MESH`.
pub const TRIANGULAR_MESH: u8 = 1;

/// Why a guarded decode produced no geometry.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DracoDecodeError {
    /// Header byte 7 is not the geometry this decoder accepts. The bitstream
    /// decoder was not called. Point clouds take this path on the mesh decoder.
    WrongGeometry,
    /// The decoder returned an error, or `catch_unwind` caught a panic.
    /// `decode_i3s` turns both into empty geometry.
    Failed(String),
}

/// Attribute values in attribute-id order, already inverse-transformed.
#[derive(Clone, Debug)]
pub struct DracoGeometry {
    pub num_points: usize,
    pub indices: Vec<u32>,
    pub attributes: Vec<DracoAttribute>,
}

/// One decoded attribute and the metadata block attached to its unique id.
#[derive(Clone, Debug)]
pub struct DracoAttribute {
    pub unique_id: u32,
    pub attribute_type: GeometryAttributeType,
    pub data_type: DataType,
    pub components: u8,
    pub normalized: bool,
    pub values: AttributeValues,
    /// A mapped element extended past the attribute buffer.
    pub short_value: bool,
    pub metadata: Option<Metadata>,
}

/// Numbers produced by the mapped-index walk.
///
/// `Bool` is stored as raw bytes in [`AttributeValues::U8`]. `Invalid` and an
/// overflowing component count carry no buffer.
#[derive(Clone, Debug)]
pub enum AttributeValues {
    F32(Vec<f32>),
    I8(Vec<i8>),
    U8(Vec<u8>),
    I16(Vec<i16>),
    U16(Vec<u16>),
    I32(Vec<i32>),
    U32(Vec<u32>),
    I64(Vec<i64>),
    U64(Vec<u64>),
    F64(Vec<f64>),
    Invalid,
    Unreadable,
}

impl DracoGeometry {
    /// Attribute with this Draco unique id, in decode order.
    pub fn attribute_by_unique_id(&self, unique_id: u32) -> Option<&DracoAttribute> {
        self.attributes.iter().find(|attribute| attribute.unique_id == unique_id)
    }

    /// First attribute of this semantic, matching `named_attribute_id`.
    pub fn named_attribute(&self, kind: GeometryAttributeType) -> Option<&DracoAttribute> {
        self.attributes.iter().find(|attribute| attribute.attribute_type == kind)
    }
}

/// Decodes a triangular mesh for both workers.
///
/// `GetEncodedGeometryType` accepts only `TRIANGULAR_MESH` (header type byte 1).
/// Point clouds and buffers shorter than the type byte stay out of
/// `MeshDecoder`. A truncated mesh makes draco3d return a failed status.
/// draco-core returns `Status::Err` for the same prefixes. `catch_unwind` maps
/// an unwind panic onto [`DracoDecodeError::Failed`], which `decode_i3s` turns
/// into empty geometry. Release and wasm use `panic = "abort"`, so an assert
/// inside the decoder still aborts the module; this catch does not.
pub fn decode_triangular_mesh(data: &[u8]) -> Result<DracoGeometry, DracoDecodeError> {
    if data.get(7).copied() != Some(TRIANGULAR_MESH) {
        return Err(DracoDecodeError::WrongGeometry);
    }
    guard(|| {
        let mut buffer = DecoderBuffer::new(data);
        let mut mesh = Mesh::new();
        MeshDecoder::new().decode(&mut buffer, &mut mesh).map_err(|error| error.message().to_string())?;
        Ok(geometry_from_mesh(&mesh))
    })
}

/// Decodes a point cloud for `decode_draco`.
///
/// The type byte is checked before `PointCloudDecoder`. A mesh header is
/// [`DracoDecodeError::WrongGeometry`]. Panic and status errors use the same
/// catch as the mesh path. `PointCloudDecoder::new` leaves `threads` at 1, so
/// the point-cloud decoder runs on the calling thread.
pub fn decode_point_cloud(data: &[u8]) -> Result<DracoGeometry, DracoDecodeError> {
    if data.get(7).copied() != Some(POINT_CLOUD) {
        return Err(DracoDecodeError::WrongGeometry);
    }
    guard(|| {
        let mut buffer = DecoderBuffer::new(data);
        let mut cloud = PointCloud::new();
        PointCloudDecoder::new().decode(&mut buffer, &mut cloud).map_err(|error| error.message().to_string())?;
        Ok(geometry_from_cloud(&cloud))
    })
}

fn guard(decode: impl FnOnce() -> Result<DracoGeometry, String>) -> Result<DracoGeometry, DracoDecodeError> {
    match catch_unwind(AssertUnwindSafe(decode)) {
        Ok(Ok(geometry)) => Ok(geometry),
        Ok(Err(message)) => Err(DracoDecodeError::Failed(message)),
        Err(_) => Err(DracoDecodeError::Failed(String::new())),
    }
}

/// The empty handlers for `DT_INT64`, `DT_UINT64`, and `DT_FLOAT64` return
/// undefined. `DT_INVALID` and an overflowing component count do too.
/// `decode_i3s` drops the previous attribute of that name when this is `None`.
/// `decode_draco` narrows the stored 64-bit buffers to the 32-bit view that
/// worker requests: a value that fits is kept, and a value that does not fit
/// becomes 0.
pub fn defined_for_i3s(values: AttributeValues) -> Option<AttributeValues> {
    match values {
        AttributeValues::I64(_)
        | AttributeValues::U64(_)
        | AttributeValues::F64(_)
        | AttributeValues::Invalid
        | AttributeValues::Unreadable => None,
        defined => Some(defined),
    }
}

fn geometry_from_mesh(mesh: &Mesh) -> DracoGeometry {
    let mut geometry = geometry_from_cloud(mesh);
    let mut indices = Vec::with_capacity(mesh.num_faces().saturating_mul(3));
    for face in mesh.faces() {
        indices.extend(face.iter().map(|point| point.0));
    }
    geometry.indices = indices;
    geometry
}

fn geometry_from_cloud(cloud: &PointCloud) -> DracoGeometry {
    let num_points = cloud.num_points();
    let mut attributes = Vec::new();
    for attr_index in 0..cloud.num_attributes() {
        let Ok(attribute) = cloud.try_attribute(attr_index) else {
            continue;
        };
        attributes.push(decoded_attribute(cloud, attribute, num_points));
    }
    DracoGeometry { num_points, indices: Vec::new(), attributes }
}

fn decoded_attribute(cloud: &PointCloud, attribute: &PointAttribute, num_points: usize) -> DracoAttribute {
    let metadata =
        cloud.attribute_metadata_by_unique_id(attribute.unique_id()).map(|metadata| metadata.metadata().clone());
    let (values, short_value) = read_attribute(attribute, num_points);
    DracoAttribute {
        unique_id: attribute.unique_id(),
        attribute_type: attribute.attribute_type(),
        data_type: attribute.data_type(),
        components: attribute.num_components(),
        normalized: attribute.normalized(),
        values,
        short_value,
        metadata,
    }
}

fn read_attribute(attribute: &PointAttribute, vertex_count: usize) -> (AttributeValues, bool) {
    let components = attribute.num_components() as usize;
    let Some(count) = components.checked_mul(vertex_count) else {
        return (AttributeValues::Unreadable, false);
    };
    match attribute.data_type() {
        DataType::Invalid => (AttributeValues::Invalid, false),
        DataType::Float32 => pack(read_mapped(attribute, count, read_f32), AttributeValues::F32),
        DataType::Int8 => pack(read_mapped(attribute, count, read_i8), AttributeValues::I8),
        DataType::Uint8 | DataType::Bool => pack(read_mapped(attribute, count, read_u8), AttributeValues::U8),
        DataType::Int16 => pack(read_mapped(attribute, count, read_i16), AttributeValues::I16),
        DataType::Uint16 => pack(read_mapped(attribute, count, read_u16), AttributeValues::U16),
        DataType::Int32 => pack(read_mapped(attribute, count, read_i32), AttributeValues::I32),
        DataType::Uint32 => pack(read_mapped(attribute, count, read_u32), AttributeValues::U32),
        // Stored so decodeDraco can downcast. defined_for_i3s still returns undefined.
        DataType::Int64 => pack(read_mapped(attribute, count, read_i64), AttributeValues::I64),
        DataType::Uint64 => pack(read_mapped(attribute, count, read_u64), AttributeValues::U64),
        DataType::Float64 => pack(read_mapped(attribute, count, read_f64), AttributeValues::F64),
    }
}

fn pack<T>(read: (Vec<T>, bool), wrap: fn(Vec<T>) -> AttributeValues) -> (AttributeValues, bool) {
    (wrap(read.0), read.1)
}

fn read_mapped<T: Copy + Default>(attribute: &PointAttribute, count: usize, read: fn(&[u8]) -> T) -> (Vec<T>, bool) {
    let components = attribute.num_components() as usize;
    if components == 0 {
        return (vec![T::default(); count], false);
    }
    let width = attribute.data_type().byte_length();
    let stride = attribute.byte_stride() as usize;
    let data = attribute.buffer().data();
    let mut out = vec![T::default(); count];
    let mut short = false;
    for (element, slot) in out.iter_mut().enumerate() {
        let point = element / components;
        let component = element % components;
        let value_index = attribute.mapped_index(PointIndex(point as u32)).0 as usize;
        let Some(offset) = value_index.checked_mul(stride).and_then(|base| base.checked_add(component * width)) else {
            continue;
        };
        if let Some(bytes) = data.get(offset..offset + width) {
            *slot = read(bytes);
        } else {
            short = true;
        }
    }
    (out, short)
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
fn read_i64(bytes: &[u8]) -> i64 {
    i64::from_le_bytes(bytes.try_into().unwrap_or([0; 8]))
}
fn read_u64(bytes: &[u8]) -> u64 {
    u64::from_le_bytes(bytes.try_into().unwrap_or([0; 8]))
}
fn read_f64(bytes: &[u8]) -> f64 {
    f64::from_le_bytes(bytes.try_into().unwrap_or([0; 8]))
}

#[cfg(test)]
mod tests {
    use super::{
        decode_triangular_mesh, defined_for_i3s, AttributeValues, DracoDecodeError, POINT_CLOUD, TRIANGULAR_MESH,
    };

    #[test]
    fn point_cloud_header_never_reaches_the_mesh_decoder() {
        let mut bytes = b"DRACO".to_vec();
        bytes.extend_from_slice(&[1, 0, POINT_CLOUD, 0, 0, 0]);
        let decoded = std::panic::catch_unwind(|| decode_triangular_mesh(&bytes));
        assert!(matches!(decoded, Ok(Err(DracoDecodeError::WrongGeometry))));
    }

    #[test]
    fn short_header_never_reaches_the_mesh_decoder() {
        assert!(matches!(decode_triangular_mesh(b"DRACO"), Err(DracoDecodeError::WrongGeometry)));
    }

    #[test]
    fn truncated_triangular_mesh_does_not_panic() {
        let mut bytes = b"DRACO".to_vec();
        bytes.extend_from_slice(&[1, 0, TRIANGULAR_MESH, 0, 0, 0, 0, 0, 0, 0]);
        for length in 0..=bytes.len() {
            let decoded = std::panic::catch_unwind(|| decode_triangular_mesh(&bytes[..length]));
            assert!(decoded.is_ok(), "mesh decode panicked at {length} bytes");
        }
    }

    #[test]
    fn wide_attributes_are_undefined_for_i3s() {
        assert!(defined_for_i3s(AttributeValues::I64(vec![1])).is_none());
        assert!(defined_for_i3s(AttributeValues::U64(vec![1])).is_none());
        assert!(defined_for_i3s(AttributeValues::F64(vec![1.0])).is_none());
        assert!(defined_for_i3s(AttributeValues::Invalid).is_none());
        assert!(matches!(defined_for_i3s(AttributeValues::F32(vec![1.0])), Some(AttributeValues::F32(_))));
    }
}
