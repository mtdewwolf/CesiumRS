//! CesiumJS `WireframeIndexGenerator`: line indices for the edges of
//! triangles, triangle strips and triangle fans.

use super::index_datatype::IndexDatatype;
use super::primitive_type::PrimitiveType;
use super::typed_array::TypedArray;

/// Line-list indices (two per edge) for the triangles, from their indices or,
/// without them, from the vertex order. `None` for non-triangle primitives.
pub fn create_wireframe_indices(
    primitive_type: PrimitiveType,
    vertex_count: usize,
    original_indices: Option<&[u32]>,
) -> Option<TypedArray> {
    let mut out: Vec<u32> = Vec::new();
    match (primitive_type, original_indices) {
        (PrimitiveType::Triangles, Some(idx)) => {
            // A missing index reads as undefined in Cesium and stores as 0.
            let at = |i: usize| idx.get(i).copied().unwrap_or(0);
            for i in (0..idx.len()).step_by(3) {
                let (p0, p1, p2) = (at(i), at(i + 1), at(i + 2));
                out.extend([p0, p1, p1, p2, p2, p0]);
            }
        }
        (PrimitiveType::Triangles, None) => {
            for i in (0..vertex_count as u32).step_by(3) {
                out.extend([i, i + 1, i + 1, i + 2, i + 2, i]);
            }
        }
        (PrimitiveType::TriangleStrip, Some(idx)) => {
            out.extend([idx[0], idx[1]]);
            for i in 0..idx.len().saturating_sub(2) {
                let (p0, p1, p2) = (idx[i], idx[i + 1], idx[i + 2]);
                out.extend([p1, p2, p2, p0]);
            }
        }
        (PrimitiveType::TriangleStrip, None) => {
            out.extend([0, 1]);
            for i in 0..(vertex_count as u32).saturating_sub(2) {
                out.extend([i + 1, i + 2, i + 2, i]);
            }
        }
        (PrimitiveType::TriangleFan, Some(idx)) => {
            let first_point = idx[0];
            out.extend([first_point, idx[1]]);
            for i in 0..idx.len().saturating_sub(2) {
                out.extend([idx[i + 1], idx[i + 2], idx[i + 2], first_point]);
            }
        }
        (PrimitiveType::TriangleFan, None) => {
            out.extend([0, 1]);
            for i in 0..(vertex_count as u32).saturating_sub(2) {
                out.extend([i + 1, i + 2, i + 2, 0]);
            }
        }
        _ => return None,
    }
    // Cesium writes into a typed array of this length: extra writes are
    // dropped and unwritten slots stay 0.
    let length = match (primitive_type, original_indices) {
        (PrimitiveType::Triangles, Some(idx)) => idx.len() * 2,
        (PrimitiveType::Triangles, None) => vertex_count * 2,
        (_, Some(idx)) => 2 + idx.len().saturating_sub(2) * 4,
        (_, None) => 2 + vertex_count.saturating_sub(2) * 4,
    };
    out.resize(length, 0);
    Some(IndexDatatype::create_typed_array(vertex_count, &out))
}

pub fn get_wireframe_indices_count(primitive_type: PrimitiveType, original_count: usize) -> usize {
    match primitive_type {
        PrimitiveType::Triangles => original_count * 2,
        PrimitiveType::TriangleStrip | PrimitiveType::TriangleFan => 2 + (original_count - 2) * 4,
        _ => original_count,
    }
}
