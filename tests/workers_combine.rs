//! `combineGeometry` against the real CesiumJS worker, including packed
//! instances and Float32 bounding-sphere buffers.
mod common;
use cesium_rs::core::*;
use cesium_rs::workers::*;
use common::*;
use serde_json::{json, Map, Value};

fn f64s(v: &Value) -> Vec<f64> {
    nums(v)
}

fn flag(script: &Value, key: &str) -> bool {
    script.get(key).and_then(Value::as_bool).unwrap_or(false)
}

fn matrix(v: &Value) -> Matrix4 {
    let mut values = [0.0; 16];
    for (i, n) in f64s(v).into_iter().take(16).enumerate() {
        values[i] = n;
    }
    Matrix4(values)
}

fn packed_results(v: &Value) -> PackedGeometryResults {
    PackedGeometryResults {
        string_table: v["stringTable"].as_array().unwrap().iter().map(|s| s.as_str().unwrap().to_string()).collect(),
        packed_data: f64s(&v["packedData"]),
    }
}

fn typed_values(values: &TypedArray) -> Value {
    Value::Array(values.to_f64_vec().into_iter().map(num).collect())
}

fn sphere_json(sphere: Option<BoundingSphere>) -> Value {
    match sphere {
        None => Value::Null,
        Some(sphere) => json!([num(sphere.center.x), num(sphere.center.y), num(sphere.center.z), num(sphere.radius)]),
    }
}

fn geometry_json(geometry: &Geometry) -> Value {
    let mut attributes = Map::new();
    for (name, attribute) in geometry.attributes.iter() {
        attributes.insert(
            name.to_string(),
            json!([
                attribute.component_datatype as u32,
                attribute.components_per_attribute,
                attribute.normalize,
                typed_array_name(&attribute.values),
                typed_values(&attribute.values),
            ]),
        );
    }
    json!({
        "attributes": attributes,
        "indices": geometry.indices.as_ref().map(|indices| json!([typed_array_name(indices), typed_values(indices)])),
        "primitiveType": geometry.primitive_type as u32,
        "boundingSphere": sphere_json(geometry.bounding_sphere),
        "boundingSphereCV": sphere_json(geometry.bounding_sphere_cv),
        "geometryType": geometry.geometry_type as u32,
        "offsetAttribute": geometry.offset_attribute.map(|offset| offset as u32),
    })
}

fn locations_json(locations: &[(String, u32)]) -> Value {
    let mut map = Map::new();
    for (name, index) in locations {
        map.insert(name.clone(), json!(index));
    }
    Value::Object(map)
}

fn encode(result: CombineGeometryResult) -> Value {
    json!({
        "geometries": result.geometries.as_ref().map(|geometries| geometries.iter().map(geometry_json).collect::<Vec<_>>()),
        "attributeLocations": result.attribute_locations.as_ref().map(|locations| locations_json(locations)),
        "modelMatrix": result.model_matrix.0.iter().copied().map(num).collect::<Vec<_>>(),
        "pickOffsets": result.pick_offsets.as_ref().map(|offsets| offsets.iter().map(|offset| json!({
            "index": offset.index,
            "offset": offset.offset,
            "count": offset.count,
        })).collect::<Vec<_>>()),
        "offsetInstanceExtend": result.offset_instance_extend.as_ref().map(|slots| slots.iter().map(|slot| slot.map(|offset| offset as u32)).collect::<Vec<_>>()),
        "boundingSpheres": result.bounding_spheres.iter().map(|v| num(f64::from(*v))).collect::<Vec<_>>(),
        "boundingSpheresCV": result.bounding_spheres_cv.iter().map(|v| num(f64::from(*v))).collect::<Vec<_>>(),
    })
}

fn combine(script: &Value) -> Result<CombineGeometryResult, RuntimeError> {
    let radii = f64s(&script["ellipsoid"]);
    combine_geometry(PackedCombineGeometry {
        create_geometry_results: script["createGeometryResults"]
            .as_array()
            .unwrap()
            .iter()
            .map(packed_results)
            .collect(),
        packed_instances: f64s(&script["packedInstances"]),
        ellipsoid_radii: Cartesian3::new(radii[0], radii[1], radii[2]),
        is_geographic: flag(script, "isGeographic"),
        element_index_uint_supported: flag(script, "elementIndexUintSupported"),
        scene_3d_only: flag(script, "scene3DOnly"),
        vertex_cache_optimize: flag(script, "vertexCacheOptimize"),
        compress_vertices: flag(script, "compressVertices"),
        model_matrix: matrix(&script["modelMatrix"]),
        create_pick_offsets: flag(script, "createPickOffsets"),
    })
}

#[test]
fn combine_geometry_reference() {
    run(
        "workers",
        "combineGeometry",
        |_| EXACT,
        |f, a| {
            if f != "combineGeometry" {
                return None;
            }
            Some(match combine(&a[0]) {
                Ok(result) => Out::Ok(encode(result)),
                Err(error) => Out::Err(error.to_string()),
            })
        },
    );
}
