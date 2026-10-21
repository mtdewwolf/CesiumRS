//! Vector tile workers against their real CesiumJS message handlers.
mod common;
use cesium_rs::{core::*, workers::*};
use common::*;
use serde_json::{json, Map, Value};

// Float32 outputs can differ by one unit in the last place; digests sum them.
const VECTOR: Tol = Tol { rel: 2e-7, abs: 2e-7, vector: false };

fn values(v: &Value) -> Vec<f64> {
    nums(v)
}
fn u16s(v: &Value) -> Vec<u16> {
    values(v).into_iter().map(|x| x as u16).collect()
}
fn u32s(v: &Value) -> Vec<u32> {
    values(v).into_iter().map(|x| x as u32).collect()
}
fn f32s(v: &Value) -> Vec<f32> {
    values(v).into_iter().map(|x| x as f32).collect()
}
/// The fixture's encoding: full arrays, or a digest past 3000 elements.
fn array(v: Vec<f64>, large: bool) -> Value {
    if !large && v.len() <= 3000 {
        return v.enc();
    }
    let (mut sum, mut weighted) = (0.0, 0.0);
    for (i, x) in v.iter().enumerate() {
        sum += x;
        weighted += x * ((i % 7) + 1) as f64;
    }
    json!({
        "length": v.len(),
        "head": v[..v.len().min(24)].to_vec().enc(),
        "tail": v[v.len().saturating_sub(24)..].to_vec().enc(),
        "sum": sum.enc(),
        "weighted": weighted.enc(),
    })
}
fn fs(v: &[f32]) -> Vec<f64> {
    v.iter().map(|&x| x as f64).collect()
}
fn us<T: Copy + Into<f64>>(v: &[T]) -> Vec<f64> {
    v.iter().map(|&x| x.into()).collect()
}
fn decoded(d: &Option<DecodedVectorPositions>) -> (Value, Value) {
    match d {
        Some(d) => (d.positions.enc(), us(&d.offsets).enc()),
        None => (Value::Null, Value::Null),
    }
}

#[test]
fn create_vector_tile_points_reference() {
    run(
        "workers",
        "createVectorTilePoints",
        |_| VECTOR,
        |_, a| {
            res(create_vector_tile_points(&u16s(&a[0]), &values(&a[1]))
                .map(|p| json!({"positions": p.enc(), "transferCount": 1})))
        },
    );
}

type PolylineBuffers = (Vec<u16>, Vec<u16>, Vec<u32>, Vec<u16>, Vec<f64>);
fn polyline_parameters<'a>(a: &[Value], buffers: &'a PolylineBuffers) -> VectorTilePolylinesParameters<'a> {
    VectorTilePolylinesParameters {
        positions: &buffers.0,
        widths: &buffers.1,
        counts: &buffers.2,
        batch_ids: &buffers.3,
        packed_buffer: &buffers.4,
        keep_decoded_positions: a[5].as_bool().unwrap(),
    }
}
fn polyline_buffers(a: &[Value]) -> PolylineBuffers {
    (u16s(&a[0]), u16s(&a[1]), u32s(&a[2]), u16s(&a[3]), values(&a[4]))
}

#[test]
fn create_vector_tile_polylines_reference() {
    run(
        "workers",
        "createVectorTilePolylines",
        |_| VECTOR,
        |_, a| {
            let buffers = polyline_buffers(a);
            let large = buffers.2.first().is_some_and(|&c| c > 16000);
            res(create_vector_tile_polylines(polyline_parameters(a, &buffers)).map(|r| {
                let (positions, offsets) = decoded(&r.decoded);
                json!({
                    "indexDatatype": r.index_datatype as u32,
                    "currentPositions": array(fs(&r.current_positions), large),
                    "previousPositions": array(fs(&r.previous_positions), large),
                    "nextPositions": array(fs(&r.next_positions), large),
                    "expandAndWidth": array(fs(&r.expand_and_width), large),
                    "batchIds": array(us(&r.batch_ids), large),
                    "indices": array(r.indices.to_f64_vec(), large),
                    "decodedPositions": positions,
                    "decodedPositionOffsets": offsets,
                    "transferCount": if r.decoded.is_some() { 8 } else { 6 },
                })
            }))
        },
    );
}

#[test]
fn create_vector_tile_clamped_polylines_reference() {
    run(
        "workers",
        "createVectorTileClampedPolylines",
        |_| VECTOR,
        |_, a| {
            let buffers = polyline_buffers(a);
            let large = buffers.2.first().is_some_and(|&c| c > 8000);
            res(create_vector_tile_clamped_polylines(polyline_parameters(a, &buffers)).map(|r| {
                let (positions, offsets) = decoded(&r.decoded);
                json!({
                    "indexDatatype": r.index_datatype as u32,
                    "startEllipsoidNormals": array(fs(&r.start_ellipsoid_normals), large),
                    "endEllipsoidNormals": array(fs(&r.end_ellipsoid_normals), large),
                    "startPositionAndHeights": array(fs(&r.start_position_and_heights), large),
                    "startFaceNormalAndVertexCornerIds": array(fs(&r.start_face_normal_and_vertex_corner_ids), large),
                    "endPositionAndHeights": array(fs(&r.end_position_and_heights), large),
                    "endFaceNormalAndHalfWidths": array(fs(&r.end_face_normal_and_half_widths), large),
                    "vertexBatchIds": array(us(&r.vertex_batch_ids), large),
                    "indices": array(r.indices.to_f64_vec(), large),
                    "decodedPositions": positions,
                    "decodedPositionOffsets": offsets,
                    "transferCount": if r.decoded.is_some() { 10 } else { 8 },
                })
            }))
        },
    );
}

#[test]
fn create_vector_tile_polygons_reference() {
    run(
        "workers",
        "createVectorTilePolygons",
        |_| VECTOR,
        |_, a| {
            let packed = values(&a[0]);
            let indices =
                if packed[0] == 2.0 { TypedArray::Uint16(u16s(&a[1])) } else { TypedArray::Uint32(u32s(&a[1])) };
            let (positions, counts, index_counts, batch_ids, colors) =
                (u16s(&a[2]), u32s(&a[3]), u32s(&a[4]), u32s(&a[5]), u32s(&a[6]));
            let (min, max) = (a[7].as_array().map(|_| f32s(&a[7])), a[8].as_array().map(|_| f32s(&a[8])));
            let large = counts.first().is_some_and(|&c| c > 10000);
            res(create_vector_tile_polygons(VectorTilePolygonsParameters {
                packed_buffer: &packed,
                indices: &indices,
                positions: &positions,
                counts: &counts,
                index_counts: &index_counts,
                batch_ids: &batch_ids,
                batch_table_colors: &colors,
                minimum_heights: min.as_deref(),
                maximum_heights: max.as_deref(),
            })
            .map(|r| {
                json!({
                    "positions": array(fs(&r.positions), large),
                    "indices": array(r.indices.to_f64_vec(), large),
                    "indexOffsets": array(us(&r.index_offsets), large),
                    "indexCounts": array(us(&r.index_counts), large),
                    "batchIds": array(us(&r.batch_ids), large),
                    "packedBuffer": r.packed_buffer.enc(),
                    "transferCount": 6,
                })
            }))
        },
    );
}

#[test]
fn create_vector_tile_geometries_reference() {
    run(
        "workers",
        "createVectorTileGeometries",
        |_| VECTOR,
        |_, a| {
            let o: &Map<String, Value> = a[0].as_object().unwrap();
            let f = |k: &str| o.get(k).map(f32s);
            let ids = |k: &str| o.get(k).map(u16s);
            let (boxes, cylinders, ellipsoids, spheres) = (f("boxes"), f("cylinders"), f("ellipsoids"), f("spheres"));
            let (box_ids, cylinder_ids, ellipsoid_ids, sphere_ids) =
                (ids("boxBatchIds"), ids("cylinderBatchIds"), ids("ellipsoidBatchIds"), ids("sphereBatchIds"));
            let colors = u32s(&a[1]);
            let packed = values(&a[2]);
            let large = ellipsoid_ids.as_ref().is_some_and(|i| i.len() > 30);
            res(create_vector_tile_geometries(VectorTileGeometriesParameters {
                boxes: boxes.as_deref(),
                box_batch_ids: box_ids.as_deref(),
                cylinders: cylinders.as_deref(),
                cylinder_batch_ids: cylinder_ids.as_deref(),
                ellipsoids: ellipsoids.as_deref(),
                ellipsoid_batch_ids: ellipsoid_ids.as_deref(),
                spheres: spheres.as_deref(),
                sphere_batch_ids: sphere_ids.as_deref(),
                batch_table_colors: &colors,
                packed_buffer: &packed,
            })
            .map(|r| {
                json!({
                    "positions": array(fs(&r.positions), large),
                    "vertexBatchIds": array(us(&r.vertex_batch_ids), large),
                    "indices": array(r.indices.to_f64_vec(), large),
                    "indexOffsets": array(us(&r.index_offsets), large),
                    "indexCounts": array(us(&r.index_counts), large),
                    "batchIds": array(us(&r.batch_ids), large),
                    "packedBuffer": r.packed_buffer.enc(),
                    "transferCount": 7,
                })
            }))
        },
    );
}
