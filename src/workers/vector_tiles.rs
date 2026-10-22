//! Vector tile workers: CesiumJS `createVectorTilePoints`, `createVectorTilePolylines`,
//! `createVectorTileClampedPolylines`, `createVectorTilePolygons` and
//! `createVectorTileGeometries`.
//!
//! Inputs are the typed views the workers construct over their transferred
//! buffers, together with the same Float64 `packedBuffer` layouts. Outputs own
//! their buffers in place of the transfer lists.
//!
//! The workers' JavaScript typed-array semantics are kept: reads past an input
//! yield `undefined` (NaN in arithmetic, 0 when stored in an integer array),
//! writes past an output are dropped, and integer outputs wrap. Packed buffers
//! shorter than their layout read NaN for the missing values.

use crate::core::decode_vector_polyline_positions::{decode_vector_polyline_positions, DecodeVectorPolylineOptions};
use crate::core::js_math::JsMath;
use crate::core::math::{js_max, js_min};
use crate::core::*;
use std::collections::BTreeMap;

const MAX_SHORT: f64 = 32767.0;

fn invalid(message: &str) -> RuntimeError {
    RuntimeError::new(message)
}
fn at<T: Copy + Into<f64>>(values: &[T], index: usize) -> f64 {
    values.get(index).map_or(f64::NAN, |&v| v.into())
}
fn unpack3<T: Copy + Into<f64>>(values: &[T], index: usize) -> Cartesian3 {
    Cartesian3::new(at(values, index), at(values, index + 1), at(values, index + 2))
}
fn put(values: &mut [f32], index: usize, value: f64) {
    if let Some(slot) = values.get_mut(index) {
        *slot = value as f32;
    }
}
fn put3(values: &mut [f32], index: usize, c: Cartesian3) {
    put(values, index, c.x);
    put(values, index + 1, c.y);
    put(values, index + 2, c.z);
}
fn put_u16(values: &mut [u16], index: usize, value: f64) {
    if let Some(slot) = values.get_mut(index) {
        *slot = TypedArray::from_values(ComponentDatatype::UnsignedShort, &[value]).get(0).unwrap_or(0.0) as u16;
    }
}
/// A copy of `packed` long enough for `length` values, padded with NaN.
fn packed(values: &[f64], length: usize) -> Vec<f64> {
    let mut out = values.to_vec();
    if out.len() < length {
        out.resize(length, f64::NAN);
    }
    out
}
/// `new Float32Array(n)` and friends reject negative and fractional lengths.
fn length(value: f64) -> Result<usize, RuntimeError> {
    if value >= 0.0 && value.fract() == 0.0 && value < 2f64.powi(32) {
        Ok(value as usize)
    } else {
        Err(invalid("Invalid typed array length"))
    }
}
/// `IndexDatatype.createTypedArray(numberOfVertices, values)`, converting each
/// value as a typed-array store does.
fn index_array(number_of_vertices: f64, values: &[f64]) -> TypedArray {
    let datatype =
        if number_of_vertices >= 65536.0 { ComponentDatatype::UnsignedInt } else { ComponentDatatype::UnsignedShort };
    TypedArray::from_values(datatype, values)
}
fn index_datatype(indices: &TypedArray) -> IndexDatatype {
    match indices {
        TypedArray::Uint32(_) => IndexDatatype::UnsignedInt,
        _ => IndexDatatype::UnsignedShort,
    }
}

/// The polyline workers' decoded positions and the start of each polyline in them.
#[derive(Clone, Debug, PartialEq)]
pub struct DecodedVectorPositions {
    pub positions: Vec<f64>,
    /// One more entry than there are polylines; the last is the total count.
    pub offsets: Vec<u32>,
}

fn position_offsets(counts: &[u32]) -> Vec<u32> {
    let mut offset = 0u32;
    let mut out = Vec::with_capacity(counts.len() + 1);
    for &count in counts {
        out.push(offset);
        offset = offset.wrapping_add(count);
    }
    out.push(offset);
    out
}

/// Splits `u`, `v` and height planes and zig-zag delta decodes them in place.
fn decode_planes(positions: &mut [u16], planes: usize) -> Result<usize, RuntimeError> {
    if !positions.len().is_multiple_of(planes) {
        return Err(invalid("Encoded positions must hold whole u, v and height planes."));
    }
    let n = positions.len() / planes;
    let (u, rest) = positions.split_at_mut(n);
    let (v, rest) = rest.split_at_mut(n);
    AttributeCompression::zig_zag_delta_decode(u, v, if planes == 3 { Some(&mut rest[..n]) } else { None })
        .ok_or_else(|| invalid("Encoded positions must hold whole u, v and height planes."))?;
    Ok(n)
}

fn cartographic_position(rectangle: &Rectangle, ellipsoid: &Ellipsoid, u: f64, v: f64, height: f64) -> Cartesian3 {
    let c = Cartographic::new(
        math::lerp(rectangle.west, rectangle.east, u / MAX_SHORT),
        math::lerp(rectangle.south, rectangle.north, v / MAX_SHORT),
        height,
    );
    ellipsoid.cartographic_to_cartesian(&c)
}

// ---- points -----------------------------------------------------------------

/// `createVectorTilePoints`: `packed_buffer` is `[minimumHeight, maximumHeight,
/// rectangle (4), ellipsoid radii (3)]`. Returns the decoded Float64 positions.
pub fn create_vector_tile_points(positions: &[u16], packed_buffer: &[f64]) -> Result<Vec<f64>, RuntimeError> {
    let packed = packed(packed_buffer, 9);
    let (min, max) = (packed[0], packed[1]);
    let rectangle = Rectangle::unpack(&packed, 2);
    let ellipsoid = Ellipsoid::unpack(&packed, 6);
    let mut positions = positions.to_vec();
    let n = decode_planes(&mut positions, 3)?;
    let mut decoded = vec![0.0; positions.len()];
    for i in 0..n {
        let height = math::lerp(min, max, positions[2 * n + i] as f64 / MAX_SHORT);
        let p = cartographic_position(&rectangle, &ellipsoid, positions[i] as f64, positions[n + i] as f64, height);
        p.pack(&mut decoded, i * 3);
    }
    Ok(decoded)
}

// ---- polylines --------------------------------------------------------------

/// Inputs shared by both polyline workers. `packed_buffer` is `[minimumHeight,
/// maximumHeight, rectangle (4), ellipsoid radii (3), center (3)]`.
#[derive(Clone, Copy, Debug)]
pub struct VectorTilePolylinesParameters<'a> {
    pub positions: &'a [u16],
    pub widths: &'a [u16],
    pub counts: &'a [u32],
    pub batch_ids: &'a [u16],
    pub packed_buffer: &'a [f64],
    pub keep_decoded_positions: bool,
}

struct PolylineTile {
    min: f64,
    max: f64,
    rectangle: Rectangle,
    ellipsoid: Ellipsoid,
    center: Cartesian3,
}
fn polyline_tile(packed_buffer: &[f64]) -> PolylineTile {
    let packed = packed(packed_buffer, 12);
    PolylineTile {
        min: packed[0],
        max: packed[1],
        rectangle: Rectangle::unpack(&packed, 2),
        ellipsoid: Ellipsoid::unpack(&packed, 6),
        center: Cartesian3::unpack(&packed, 9),
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct VectorTilePolylines {
    pub index_datatype: IndexDatatype,
    pub current_positions: Vec<f32>,
    pub previous_positions: Vec<f32>,
    pub next_positions: Vec<f32>,
    pub expand_and_width: Vec<f32>,
    pub batch_ids: Vec<u16>,
    pub indices: TypedArray,
    pub decoded: Option<DecodedVectorPositions>,
}

/// `createVectorTilePolylines`: screen-space polyline vertices, four per segment.
pub fn create_vector_tile_polylines(p: VectorTilePolylinesParameters<'_>) -> Result<VectorTilePolylines, RuntimeError> {
    let tile = polyline_tile(p.packed_buffer);
    let mut encoded = p.positions.to_vec();
    let positions = decode_vector_polyline_positions(
        &mut encoded,
        DecodeVectorPolylineOptions {
            rectangle: &tile.rectangle,
            minimum_height: tile.min,
            maximum_height: tile.max,
            ellipsoid: &tile.ellipsoid,
        },
    )
    .ok_or_else(|| invalid("Encoded positions must hold whole u, v and height planes."))?;
    let positions_length = positions.len() / 3;
    let size = positions_length as f64 * 4.0 - 4.0;
    let size_len = length(size)?;
    let mut current = vec![0f32; length(size * 3.0)?];
    let mut previous_positions = vec![0f32; current.len()];
    let mut next_positions = vec![0f32; current.len()];
    let mut expand_and_width = vec![0f32; length(size * 2.0)?];
    let mut vertex_batch_ids = vec![0u16; size_len];
    let (mut position_index, mut expand_index, mut batch_index, mut offset) = (0, 0, 0, 0usize);
    for (i, &count) in p.counts.iter().enumerate() {
        let count = count as usize;
        let width = at(p.widths, i);
        let batch_id = at(p.batch_ids, i);
        for j in 0..count {
            let previous = if j == 0 {
                let p0 = unpack3(&positions, offset * 3);
                let p1 = unpack3(&positions, (offset + 1) * 3);
                p0.add(p0.subtract(p1))
            } else {
                unpack3(&positions, (offset + j - 1) * 3)
            };
            let cur = unpack3(&positions, (offset + j) * 3);
            let next = if j == count - 1 {
                let p2 = unpack3(&positions, (offset + count - 1) * 3);
                // `count - 2` is -1 for a single position: an undefined read.
                let p3 = match (offset + count).checked_sub(2) {
                    Some(k) => unpack3(&positions, k * 3),
                    None => Cartesian3::new(f64::NAN, f64::NAN, f64::NAN),
                };
                p2.add(p2.subtract(p3))
            } else {
                unpack3(&positions, (offset + j + 1) * 3)
            };
            let (previous, cur, next) =
                (previous.subtract(tile.center), cur.subtract(tile.center), next.subtract(tile.center));
            let start_k = if j == 0 { 2 } else { 0 };
            let end_k = if j == count - 1 { 2 } else { 4 };
            for k in start_k..end_k {
                put3(&mut current, position_index, cur);
                put3(&mut previous_positions, position_index, previous);
                put3(&mut next_positions, position_index, next);
                position_index += 3;
                let direction = if k < 2 { -1.0 } else { 1.0 };
                put(&mut expand_and_width, expand_index, (2 * (k % 2)) as f64 - 1.0);
                put(&mut expand_and_width, expand_index + 1, direction * width);
                expand_index += 2;
                put_u16(&mut vertex_batch_ids, batch_index, batch_id);
                batch_index += 1;
            }
        }
        offset += count;
    }
    let index_length = length(positions_length as f64 * 6.0 - 6.0)?;
    let mut indices = vec![0.0; index_length];
    let mut index = 0.0;
    for quad in indices.as_chunks_mut::<6>().0 {
        quad.copy_from_slice(&[index, index + 2.0, index + 1.0, index + 1.0, index + 2.0, index + 3.0]);
        index += 4.0;
    }
    let indices = index_array(size, &indices);
    Ok(VectorTilePolylines {
        index_datatype: index_datatype(&indices),
        current_positions: current,
        previous_positions,
        next_positions,
        expand_and_width,
        batch_ids: vertex_batch_ids,
        indices,
        decoded: p
            .keep_decoded_positions
            .then(|| DecodedVectorPositions { positions, offsets: position_offsets(p.counts) }),
    })
}

// ---- clamped polylines ------------------------------------------------------

#[derive(Clone, Debug, PartialEq)]
pub struct VectorTileClampedPolylines {
    pub index_datatype: IndexDatatype,
    pub start_ellipsoid_normals: Vec<f32>,
    pub end_ellipsoid_normals: Vec<f32>,
    pub start_position_and_heights: Vec<f32>,
    pub start_face_normal_and_vertex_corner_ids: Vec<f32>,
    pub end_position_and_heights: Vec<f32>,
    pub end_face_normal_and_half_widths: Vec<f32>,
    pub vertex_batch_ids: Vec<u16>,
    pub indices: TypedArray,
    /// Counts are those left after consecutive duplicate positions are removed.
    pub decoded: Option<DecodedVectorPositions>,
}

// Winding is reversed so each segment's volume is inside-out.
const REFERENCE_INDICES: [u32; 36] = [
    0, 2, 6, 0, 6, 4, // right
    0, 1, 3, 0, 3, 2, // start face
    0, 4, 5, 0, 5, 1, // bottom
    5, 3, 1, 5, 7, 3, // left
    7, 5, 4, 7, 4, 6, // end face
    7, 6, 2, 7, 2, 3, // top
];

fn mitered_normal(
    previous: Cartesian3,
    position: Cartesian3,
    next: Cartesian3,
    surface_normal: Cartesian3,
) -> Result<Cartesian3, RuntimeError> {
    let miter_break = math::to_radians(150.0).js_cos();
    let toward_next = next.subtract(position).normalize();
    let mut toward_current = position.subtract(previous).normalize();
    if toward_next.dot(toward_current) < miter_break {
        toward_current = toward_current.multiply_by_scalar(-1.0);
    }
    let result = toward_next.add(toward_current);
    if result == Cartesian3::ZERO {
        // Cesium calls `Cartesian3.subtract` without a result here, which throws.
        return Err(invalid("Cannot compute the miter of a segment that doubles back on itself."));
    }
    Ok(surface_normal.cross(result.cross(surface_normal)).normalize())
}

/// `createVectorTileClampedPolylines`: one eight-vertex volume per segment.
pub fn create_vector_tile_clamped_polylines(
    p: VectorTilePolylinesParameters<'_>,
) -> Result<VectorTileClampedPolylines, RuntimeError> {
    let tile = polyline_tile(p.packed_buffer);
    let mut encoded = p.positions.to_vec();
    let mut counts = p.counts.to_vec();
    let n = decode_planes(&mut encoded, 3)?;
    let (u, rest) = encoded.split_at_mut(n);
    let (v, height) = rest.split_at_mut(n);
    // Remove consecutive duplicate u/v pairs and compact the buffers.
    let mut remove = vec![false; n];
    let mut offset = 0usize;
    for count in counts.iter_mut() {
        let original = *count;
        for j in 1..original as usize {
            // Reads past the buffers are `undefined`, and `undefined === undefined`.
            let index = offset + j;
            let current = (u.get(index), v.get(index));
            let previous = (u.get(index - 1), v.get(index - 1));
            if current == previous {
                *count -= 1;
                if let Some(mark) = remove.get_mut(index - 1) {
                    *mark = true;
                }
            }
        }
        offset += original as usize;
    }
    let mut next = 0;
    for k in 0..n {
        if !remove[k] {
            u[next] = u[k];
            v[next] = v[k];
            height[next] = height[k];
            next += 1;
        }
    }
    let volumes: i64 = counts.iter().map(|&c| c as i64 - 1).sum();
    let volumes = length(volumes as f64)?;
    let vertex_count = volumes * 8;
    let mut start_ellipsoid_normals = vec![0f32; vertex_count * 3];
    let mut end_ellipsoid_normals = vec![0f32; vertex_count * 3];
    let mut start_position_and_heights = vec![0f32; vertex_count * 4];
    let mut start_face = vec![0f32; vertex_count * 4];
    let mut end_position_and_heights = vec![0f32; vertex_count * 4];
    let mut end_face = vec![0f32; vertex_count * 4];
    let mut vertex_batch_ids = vec![0u16; vertex_count];
    let mut indices = vec![0.0; 36 * volumes];

    let mut positions = vec![0.0; n * 3];
    for i in 0..n {
        let h = math::lerp(tile.min, tile.max, height[i] as f64 / MAX_SHORT);
        cartographic_position(&tile.rectangle, &tile.ellipsoid, u[i] as f64, v[i] as f64, h)
            .pack(&mut positions, i * 3);
    }
    let rtc: Vec<f32> = positions
        .as_chunks::<3>()
        .0
        .iter()
        .flat_map(|p| [(p[0] - tile.center.x) as f32, (p[1] - tile.center.y) as f32, (p[2] - tile.center.z) as f32])
        .collect();

    let (mut vec3, mut vec4, mut batch_offset, mut index_offset, mut volume_start) = (0, 0, 0, 0, 0u32);
    let (mut position_index, mut height_index) = (0usize, 0usize);
    for (i, &count) in counts.iter().enumerate() {
        let volume_count = count as i64 - 1;
        let half_width = at(p.widths, i) * 0.5;
        let batch_id = at(p.batch_ids, i);
        let first = position_index;
        for j in 0..volume_count.max(0) {
            let start = unpack3(&rtc, position_index);
            let end = unpack3(&rtc, position_index + 3);
            let start_height = math::lerp(tile.min, tile.max, at(height, height_index) / MAX_SHORT);
            let end_height = math::lerp(tile.min, tile.max, at(height, height_index + 1) / MAX_SHORT);
            height_index += 1;
            let pre_start = if j == 0 {
                let final_index = first + volume_count as usize * 3;
                if unpack3(&rtc, final_index) == start {
                    unpack3(&rtc, final_index - 3)
                } else {
                    start.subtract(end).add(start)
                }
            } else {
                unpack3(&rtc, position_index - 3)
            };
            let post_end = if j == volume_count - 1 {
                if unpack3(&rtc, first) == end {
                    unpack3(&rtc, first + 3)
                } else {
                    end.subtract(start).add(end)
                }
            } else {
                unpack3(&rtc, position_index + 6)
            };
            let missing = || invalid("A clamped polyline position is at the ellipsoid center.");
            let start_normal = tile.ellipsoid.geodetic_surface_normal(start.add(tile.center)).ok_or_else(missing)?;
            let end_normal = tile.ellipsoid.geodetic_surface_normal(end.add(tile.center)).ok_or_else(missing)?;
            let start_face_normal = mitered_normal(pre_start, start, end, start_normal)?;
            let end_face_normal = mitered_normal(post_end, end, start, end_normal)?;
            for corner in 0..8 {
                put3(&mut start_ellipsoid_normals, vec3, start_normal);
                put3(&mut end_ellipsoid_normals, vec3, end_normal);
                put3(&mut start_position_and_heights, vec4, start);
                put(&mut start_position_and_heights, vec4 + 3, start_height);
                put3(&mut end_position_and_heights, vec4, end);
                put(&mut end_position_and_heights, vec4 + 3, end_height);
                put3(&mut start_face, vec4, start_face_normal);
                put(&mut start_face, vec4 + 3, corner as f64);
                put3(&mut end_face, vec4, end_face_normal);
                put(&mut end_face, vec4 + 3, half_width);
                put_u16(&mut vertex_batch_ids, batch_offset, batch_id);
                batch_offset += 1;
                vec3 += 3;
                vec4 += 4;
            }
            for (k, &reference) in REFERENCE_INDICES.iter().enumerate() {
                if let Some(slot) = indices.get_mut(index_offset + k) {
                    *slot = (reference + volume_start) as f64;
                }
            }
            volume_start += 8;
            index_offset += 36;
            position_index += 3;
        }
        position_index += 3;
        height_index += 1;
    }
    let indices = index_array(vertex_count as f64, &indices);
    Ok(VectorTileClampedPolylines {
        index_datatype: index_datatype(&indices),
        start_ellipsoid_normals,
        end_ellipsoid_normals,
        start_position_and_heights,
        start_face_normal_and_vertex_corner_ids: start_face,
        end_position_and_heights,
        end_face_normal_and_half_widths: end_face,
        vertex_batch_ids,
        indices,
        decoded: p
            .keep_decoded_positions
            .then(|| DecodedVectorPositions { positions, offsets: position_offsets(&counts) }),
    })
}

// ---- batched draw calls -----------------------------------------------------

/// Scene `Vector3DTileBatch`: one draw call's index range, color and features.
#[derive(Clone, Debug, PartialEq)]
pub struct Vector3DTileBatch {
    pub offset: f64,
    pub count: f64,
    pub color: Color,
    pub batch_ids: Vec<f64>,
}

fn pack_batches(out: &mut Vec<f64>, batches: &[Vector3DTileBatch]) {
    out.push(batches.len() as f64);
    for batch in batches {
        let start = out.len();
        out.resize(start + Color::PACKED_LENGTH, 0.0);
        batch.color.pack(out, start);
        out.extend([batch.offset, batch.count, batch.batch_ids.len() as f64]);
        out.extend(&batch.batch_ids);
    }
}

// ---- polygons ---------------------------------------------------------------

/// `packed_buffer` is `[indexBytesPerElement, minimumHeight, maximumHeight,
/// center (3), ellipsoid radii (3), rectangle (4)]`. `indices` is reinterpreted
/// with the packed element size, as the worker views the transferred buffer.
#[derive(Clone, Copy, Debug)]
pub struct VectorTilePolygonsParameters<'a> {
    pub packed_buffer: &'a [f64],
    pub indices: &'a TypedArray,
    pub positions: &'a [u16],
    pub counts: &'a [u32],
    pub index_counts: &'a [u32],
    pub batch_ids: &'a [u32],
    pub batch_table_colors: &'a [u32],
    /// Per-polygon extrusion heights; both must be present to replace the tile's.
    pub minimum_heights: Option<&'a [f32]>,
    pub maximum_heights: Option<&'a [f32]>,
}

#[derive(Clone, Debug, PartialEq)]
pub struct VectorTilePolygons {
    pub positions: Vec<f32>,
    pub indices: TypedArray,
    pub index_offsets: Vec<u32>,
    pub index_counts: Vec<u32>,
    pub batch_ids: Vec<u16>,
    /// `[indexDatatype, n, n oriented bounding boxes, batch count, batches...]`.
    pub packed_buffer: Vec<f64>,
}

fn reinterpret(indices: &TypedArray, bytes_per_element: f64) -> Result<Vec<f64>, RuntimeError> {
    let bytes: Vec<u8> = match indices {
        TypedArray::Uint16(v) => v.iter().flat_map(|x| x.to_le_bytes()).collect(),
        TypedArray::Uint32(v) => v.iter().flat_map(|x| x.to_le_bytes()).collect(),
        TypedArray::Uint8(v) => v.clone(),
        _ => return Err(invalid("Vector tile polygon indices must be unsigned integers.")),
    };
    if bytes_per_element == 2.0 {
        Ok(bytes.as_chunks::<2>().0.iter().map(|b| u16::from_le_bytes(*b) as f64).collect())
    } else if !bytes.len().is_multiple_of(4) {
        Err(invalid("byte length of Uint32Array should be a multiple of 4"))
    } else {
        Ok(bytes.as_chunks::<4>().0.iter().map(|b| u32::from_le_bytes(*b) as f64).collect())
    }
}

/// JavaScript iterates integer-like object keys in ascending order; 2^32 - 1 is
/// not an array index, so it follows them in insertion order.
fn color_key(rgba: u32) -> u64 {
    if rgba == u32::MAX {
        u64::MAX
    } else {
        rgba as u64
    }
}

/// A JavaScript array assigned by index: gaps read `undefined`.
struct SparseArray(Vec<f64>);
impl SparseArray {
    fn set(&mut self, index: f64, value: f64) {
        if index >= 0.0 && index.fract() == 0.0 && index < 4294967295.0 {
            let index = index as usize;
            if self.0.len() <= index {
                self.0.resize(index + 1, f64::NAN);
            }
            self.0[index] = value;
        }
    }
}

/// `createVectorTilePolygons`: extruded top, bottom and wall triangles batched by color.
pub fn create_vector_tile_polygons(p: VectorTilePolygonsParameters<'_>) -> Result<VectorTilePolygons, RuntimeError> {
    let packed_in = packed(p.packed_buffer, 13);
    let index_bytes = packed_in[0];
    let (min_height, max_height) = (packed_in[1], packed_in[2]);
    let center = Cartesian3::unpack(&packed_in, 3);
    let ellipsoid = Ellipsoid::unpack(&packed_in, 6);
    let rectangle = Rectangle::unpack(&packed_in, 9);
    let indices = reinterpret(p.indices, index_bytes)?;
    let polygon_count = p.counts.len();
    if p.index_counts.len() < polygon_count || p.batch_table_colors.len() < polygon_count {
        return Err(invalid("Every polygon needs an index count and a color."));
    }
    let heights = match (p.minimum_heights, p.maximum_heights) {
        (Some(min), Some(max)) => Some((min, max)),
        _ => None,
    };

    let mut positions = p.positions.to_vec();
    let n = decode_planes(&mut positions, 2)?;
    let mut decoded = vec![0.0; n * 3];
    for i in 0..n {
        cartographic_position(&rectangle, &ellipsoid, positions[i] as f64, positions[n + i] as f64, 0.0)
            .pack(&mut decoded, i * 3);
    }
    let mut offsets = Vec::with_capacity(polygon_count);
    let mut index_offsets = Vec::with_capacity(polygon_count);
    let (mut current_offset, mut current_index_offset) = (0u64, 0u64);
    for i in 0..polygon_count {
        offsets.push(current_offset);
        index_offsets.push(current_index_offset);
        current_offset += p.counts[i] as u64;
        current_index_offset += p.index_counts[i] as u64;
    }

    let mut batched_positions = vec![0f32; n * 6];
    let mut batched_ids = vec![0u16; n * 2];
    let mut batched_index_offsets = vec![0u32; polygon_count];
    let mut batched_index_counts = vec![0u32; p.index_counts.len()];
    let mut batched_indices = SparseArray(Vec::new());

    struct ColorBuffer {
        position_length: f64,
        index_length: f64,
        offset: f64,
        index_offset: f64,
        batch_ids: Vec<f64>,
    }
    let mut buffers: BTreeMap<u64, ColorBuffer> = BTreeMap::new();
    for i in 0..polygon_count {
        let key = color_key(p.batch_table_colors[i]);
        let buffer = buffers.entry(key).or_insert(ColorBuffer {
            position_length: 0.0,
            index_length: 0.0,
            offset: 0.0,
            index_offset: 0.0,
            batch_ids: vec![],
        });
        buffer.position_length += p.counts[i] as f64;
        buffer.index_length += p.index_counts[i] as f64;
        buffer.batch_ids.push(i as f64);
    }
    let (mut by_color_position, mut by_color_index) = (0.0, 0.0);
    for buffer in buffers.values_mut() {
        buffer.offset = by_color_position;
        buffer.index_offset = by_color_index;
        let index_length = buffer.index_length * 2.0 + buffer.position_length * 6.0;
        by_color_position += buffer.position_length * 2.0;
        by_color_index += index_length;
        buffer.index_length = index_length;
    }
    let mut draw_calls: Vec<Vector3DTileBatch> = buffers
        .iter()
        .map(|(&key, b)| Vector3DTileBatch {
            color: Color::from_rgba(if key == u64::MAX { u32::MAX } else { key as u32 }),
            offset: b.index_offset,
            count: b.index_length,
            batch_ids: b.batch_ids.clone(),
        })
        .collect();

    let mut bounding_volumes = Vec::with_capacity(polygon_count);
    for i in 0..polygon_count {
        let buffer = buffers.get_mut(&color_key(p.batch_table_colors[i])).unwrap();
        let position_offset = buffer.offset;
        let mut position_index = position_offset * 3.0;
        let mut batch_index = position_offset;
        let polygon_offset = offsets[i] as usize;
        let polygon_points = p.counts[i] as usize;
        let batch_id = at(p.batch_ids, i);
        let (polygon_min, polygon_max) = match heights {
            Some((min, max)) => (at(min, i), at(max, i)),
            None => (min_height, max_height),
        };
        let (mut min_lat, mut max_lat, mut min_lon, mut max_lon) =
            (f64::INFINITY, f64::NEG_INFINITY, f64::INFINITY, f64::NEG_INFINITY);
        for j in 0..polygon_points {
            let mut position = unpack3(&decoded, polygon_offset * 3 + j * 3);
            if let Some(surface) = ellipsoid.scale_to_geodetic_surface(position) {
                position = surface;
            }
            let carto = ellipsoid
                .cartesian_to_cartographic(position)
                .ok_or_else(|| invalid("Cannot read properties of undefined (reading 'latitude')"))?;
            min_lat = js_min(carto.latitude, min_lat);
            max_lat = js_max(carto.latitude, max_lat);
            min_lon = js_min(carto.longitude, min_lon);
            max_lon = js_max(carto.longitude, max_lon);
            let normal = ellipsoid
                .geodetic_surface_normal(position)
                .ok_or_else(|| invalid("Cannot compute a surface normal at the ellipsoid center."))?;
            let min_position = position.add(normal.multiply_by_scalar(polygon_min)).subtract(center);
            let max_position = position.add(normal.multiply_by_scalar(polygon_max)).subtract(center);
            let index = position_index as usize;
            put3(&mut batched_positions, index, max_position);
            put3(&mut batched_positions, index + 3, min_position);
            put_u16(&mut batched_ids, batch_index as usize, batch_id);
            put_u16(&mut batched_ids, batch_index as usize + 1, batch_id);
            position_index += 6.0;
            batch_index += 2.0;
        }
        let bounds = Rectangle::new(min_lon, min_lat, max_lon, max_lat);
        bounding_volumes.push(
            OrientedBoundingBox::from_rectangle(&bounds, min_height, max_height, &ellipsoid)
                .ok_or_else(|| invalid("Polygon bounds are not a valid rectangle."))?,
        );

        let mut indices_index = buffer.index_offset;
        let index_offset = index_offsets[i] as usize;
        let index_count = p.index_counts[i] as usize;
        batched_index_offsets[i] =
            TypedArray::from_values(ComponentDatatype::UnsignedInt, &[indices_index]).get(0).unwrap_or(0.0) as u32;
        let local = |k: usize| at(&indices, k) - polygon_offset as f64;
        let mut push = |value: f64, at: &mut f64| {
            batched_indices.set(*at, value);
            *at += 1.0;
        };
        for j in (0..index_count).step_by(3) {
            let (i0, i1, i2) = (local(index_offset + j), local(index_offset + j + 1), local(index_offset + j + 2));
            push(i0 * 2.0 + position_offset, &mut indices_index);
            push(i1 * 2.0 + position_offset, &mut indices_index);
            push(i2 * 2.0 + position_offset, &mut indices_index);
            push(i2 * 2.0 + 1.0 + position_offset, &mut indices_index);
            push(i1 * 2.0 + 1.0 + position_offset, &mut indices_index);
            push(i0 * 2.0 + 1.0 + position_offset, &mut indices_index);
        }
        for j in 0..polygon_points {
            let v0 = j as f64;
            let v1 = ((j + 1) % polygon_points) as f64;
            push(v0 * 2.0 + 1.0 + position_offset, &mut indices_index);
            push(v1 * 2.0 + position_offset, &mut indices_index);
            push(v0 * 2.0 + position_offset, &mut indices_index);
            push(v0 * 2.0 + 1.0 + position_offset, &mut indices_index);
            push(v1 * 2.0 + 1.0 + position_offset, &mut indices_index);
            push(v1 * 2.0 + position_offset, &mut indices_index);
        }
        buffer.offset += polygon_points as f64 * 2.0;
        buffer.index_offset = indices_index;
        if let Some(slot) = batched_index_counts.get_mut(i) {
            *slot = TypedArray::from_values(
                ComponentDatatype::UnsignedInt,
                &[indices_index - batched_index_offsets[i] as f64],
            )
            .get(0)
            .unwrap_or(0.0) as u32;
        }
    }
    let indices = index_array((batched_positions.len() / 3) as f64, &batched_indices.0);
    for call in &mut draw_calls {
        call.count = call.batch_ids.iter().map(|&id| at(&batched_index_counts, id as usize)).sum();
    }
    let datatype = index_datatype(&indices);
    let mut packed_buffer = vec![datatype as u32 as f64, bounding_volumes.len() as f64];
    for volume in &bounding_volumes {
        let start = packed_buffer.len();
        packed_buffer.resize(start + OrientedBoundingBox::PACKED_LENGTH, 0.0);
        volume.pack(&mut packed_buffer, start);
    }
    pack_batches(&mut packed_buffer, &draw_calls);
    Ok(VectorTilePolygons {
        positions: batched_positions,
        indices,
        index_offsets: batched_index_offsets,
        index_counts: batched_index_counts,
        batch_ids: batched_ids,
        packed_buffer,
    })
}

// ---- geometries -------------------------------------------------------------

/// Packed boxes are `[dimensions (3), model matrix (16)]`, cylinders `[radius,
/// length, model matrix (16)]`, ellipsoids `[radii (3), model matrix (16)]` and
/// spheres `[radius, center (3)]`. Each kind is used only with its batch ids.
/// `packed_buffer` is `[center (3), tile model matrix (16)]`.
#[derive(Clone, Copy, Debug, Default)]
pub struct VectorTileGeometriesParameters<'a> {
    pub boxes: Option<&'a [f32]>,
    pub box_batch_ids: Option<&'a [u16]>,
    pub cylinders: Option<&'a [f32]>,
    pub cylinder_batch_ids: Option<&'a [u16]>,
    pub ellipsoids: Option<&'a [f32]>,
    pub ellipsoid_batch_ids: Option<&'a [u16]>,
    pub spheres: Option<&'a [f32]>,
    pub sphere_batch_ids: Option<&'a [u16]>,
    pub batch_table_colors: &'a [u32],
    pub packed_buffer: &'a [f64],
}

#[derive(Clone, Debug, PartialEq)]
pub struct VectorTileGeometries {
    pub positions: Vec<f32>,
    pub vertex_batch_ids: Vec<u16>,
    pub indices: TypedArray,
    pub index_offsets: Vec<u32>,
    pub index_counts: Vec<u32>,
    pub batch_ids: Vec<u16>,
    /// `[bytesPerIndex, n, n bounding spheres, batch count, batches...]`.
    pub packed_buffer: Vec<f64>,
}

fn matrix_at(values: &[f32], index: usize) -> Matrix4 {
    let m: Vec<f64> = (0..16).map(|k| at(values, index + k)).collect();
    Matrix4::unpack(&m, 0)
}

type ModelMatrixAndRadius = fn(&[f32], usize) -> (Matrix4, f64);
type Primitive<'a> = (Option<&'a [f32]>, Option<&'a [u16]>, ModelMatrixAndRadius);

fn box_matrix(boxes: &[f32], index: usize) -> (Matrix4, f64) {
    let i = index * 19;
    (matrix_at(boxes, i + 3).multiply_by_scale(unpack3(boxes, i)), 3f64.sqrt())
}
fn cylinder_matrix(cylinders: &[f32], index: usize) -> (Matrix4, f64) {
    let i = index * 18;
    let (radius, length) = (at(cylinders, i), at(cylinders, i + 1));
    (matrix_at(cylinders, i + 2).multiply_by_scale(Cartesian3::new(radius, radius, length)), 2f64.sqrt())
}
fn ellipsoid_matrix(ellipsoids: &[f32], index: usize) -> (Matrix4, f64) {
    let i = index * 19;
    (matrix_at(ellipsoids, i + 3).multiply_by_scale(unpack3(ellipsoids, i)), 1.0)
}
fn sphere_matrix(spheres: &[f32], index: usize) -> (Matrix4, f64) {
    let i = index * 4;
    (Matrix4::from_translation(unpack3(spheres, i + 1)).multiply_by_uniform_scale(at(spheres, i)), 1.0)
}

/// `createVectorTileGeometries`: instanced unit boxes, cylinders, ellipsoids and spheres.
pub fn create_vector_tile_geometries(
    p: VectorTileGeometriesParameters<'_>,
) -> Result<VectorTileGeometries, RuntimeError> {
    let count = |values: Option<&[f32]>, ids: Option<&[u16]>| -> Result<usize, RuntimeError> {
        match (values, ids) {
            (Some(_), Some(ids)) => Ok(ids.len()),
            (Some(_), None) => Err(invalid("Cannot read properties of undefined (reading 'length')")),
            (None, _) => Ok(0),
        }
    };
    let counts = [
        count(p.boxes, p.box_batch_ids)?,
        count(p.cylinders, p.cylinder_batch_ids)?,
        count(p.ellipsoids, p.ellipsoid_batch_ids)?,
        count(p.spheres, p.sphere_batch_ids)?,
    ];
    let unit_box = BoxGeometry::get_unit_box();
    let unit_cylinder = CylinderGeometry::get_unit_cylinder();
    let unit_ellipsoid = EllipsoidGeometry::get_unit_ellipsoid();
    let geometries = [&unit_box, &unit_cylinder, &unit_ellipsoid, &unit_ellipsoid];
    let positions_of = |g: &Geometry| g.attributes.get("position").map(|a| a.values.to_f64_vec()).unwrap_or_default();
    let indices_of = |g: &Geometry| g.indices.as_ref().map(TypedArray::to_f64_vec).unwrap_or_default();
    let unit_positions: Vec<Vec<f64>> = geometries.iter().map(|g| positions_of(g)).collect();
    let unit_indices: Vec<Vec<f64>> = geometries.iter().map(|g| indices_of(g)).collect();
    let number_of_positions: usize = (0..4).map(|k| unit_positions[k].len() * counts[k]).sum();
    let number_of_indices: usize = (0..4).map(|k| unit_indices[k].len() * counts[k]).sum();
    let number_of_geometries: usize = counts.iter().sum();

    let mut positions = vec![0f32; number_of_positions];
    let mut vertex_batch_ids = vec![0u16; number_of_positions / 3];
    let mut indices = vec![0.0; number_of_indices];
    let mut batch_ids = vec![0u16; number_of_geometries];
    let mut batches: Vec<Option<Vector3DTileBatch>> = vec![None; number_of_geometries];
    let mut index_offsets = vec![0u32; number_of_geometries];
    let mut index_counts = vec![0u32; number_of_geometries];
    let mut bounding_volumes: Vec<Option<BoundingSphere>> = vec![None; number_of_geometries];

    let packed_in = packed(p.packed_buffer, 19);
    let center = Cartesian3::unpack(&packed_in, 0);
    let model_matrix = Matrix4::unpack(&packed_in, 3);
    let primitives: [Primitive<'_>; 4] = [
        (p.boxes, p.box_batch_ids, box_matrix),
        (p.cylinders, p.cylinder_batch_ids, cylinder_matrix),
        (p.ellipsoids, p.ellipsoid_batch_ids, ellipsoid_matrix),
        (p.spheres, p.sphere_batch_ids, sphere_matrix),
    ];
    let (mut position_offset, mut batch_index, mut index_offset, mut batched_offset) = (0usize, 0usize, 0usize, 0usize);
    for (k, (values, ids, matrix_and_radius)) in primitives.into_iter().enumerate() {
        let (Some(values), Some(ids)) = (values, ids) else { continue };
        let geometry_positions = &unit_positions[k];
        let geometry_indices = &unit_indices[k];
        for (i, &batch_id) in ids.iter().enumerate() {
            let (primitive_matrix, radius) = matrix_and_radius(values, i);
            let primitive_matrix = model_matrix.multiply(&primitive_matrix);
            for j in (0..geometry_positions.len()).step_by(3) {
                let position = primitive_matrix.multiply_by_point(unpack3(geometry_positions, j)).subtract(center);
                put3(&mut positions, position_offset * 3 + j, position);
                put_u16(&mut vertex_batch_ids, batch_index, batch_id as f64);
                batch_index += 1;
            }
            for (m, &index) in geometry_indices.iter().enumerate() {
                if let Some(slot) = indices.get_mut(index_offset + m) {
                    *slot = index + position_offset as f64;
                }
            }
            let offset = i + batched_offset;
            batches[offset] = Some(Vector3DTileBatch {
                offset: index_offset as f64,
                count: geometry_indices.len() as f64,
                color: Color::from_rgba(p.batch_table_colors.get(batch_id as usize).copied().unwrap_or(0)),
                batch_ids: vec![batch_id as f64],
            });
            batch_ids[offset] = batch_id;
            index_offsets[offset] = index_offset as u32;
            index_counts[offset] = geometry_indices.len() as u32;
            bounding_volumes[offset] = Some(BoundingSphere::new(Cartesian3::ZERO, radius).transform(&primitive_matrix));
            position_offset += geometry_positions.len() / 3;
            index_offset += geometry_indices.len();
        }
        batched_offset += ids.len();
    }
    let indices = index_array((number_of_positions / 3) as f64, &indices);
    let bytes_per_index = if matches!(indices, TypedArray::Uint32(_)) { 4.0 } else { 2.0 };
    let mut packed_buffer = vec![bytes_per_index, number_of_geometries as f64];
    for volume in &bounding_volumes {
        let volume = volume.ok_or_else(|| invalid("Cannot read properties of undefined (reading 'center')"))?;
        let start = packed_buffer.len();
        packed_buffer.resize(start + BoundingSphere::PACKED_LENGTH, 0.0);
        volume.pack(&mut packed_buffer, start);
    }
    let batches = batches.into_iter().collect::<Option<Vec<_>>>().unwrap_or_default();
    pack_batches(&mut packed_buffer, &batches);
    Ok(VectorTileGeometries {
        positions,
        vertex_batch_ids,
        indices,
        index_offsets,
        index_counts,
        batch_ids,
        packed_buffer,
    })
}
