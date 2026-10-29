//! `combineGeometry` against the real CesiumJS worker, including packed
//! instances and Float32 bounding-sphere buffers.
mod common;
use cesium_rs::core::math::SIXTY_FOUR_KILOBYTES;
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

/// Constructor name Cesium records for an attribute's `values`.
///
/// `fitToUnsignedShortIndices` copies values into a plain array. The port
/// stores that copy as `Float64` and keeps the source component datatype, so
/// the storage kind and the datatype differ only for those copied attributes.
fn attribute_values_name(attribute: &GeometryAttribute) -> &'static str {
    if attribute.values.component_datatype() != attribute.component_datatype {
        "Array"
    } else {
        typed_array_name(&attribute.values)
    }
}

/// Constructor name Cesium records for `geometry.indices`.
///
/// The unsigned-short split stores each piece's indices in a plain array.
/// Every piece has fewer than 64K vertices, where a typed array would be
/// `Uint16Array`. The port stores those plain index lists as `Uint32`.
fn index_values_name(geometry: &Geometry, indices: &TypedArray) -> &'static str {
    let vertices = geometry.compute_number_of_vertices().unwrap_or(f64::NAN);
    if matches!(indices, TypedArray::Uint32(_)) && vertices < f64::from(SIXTY_FOUR_KILOBYTES) {
        "Array"
    } else {
        typed_array_name(indices)
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
                attribute_values_name(attribute),
                typed_values(&attribute.values),
            ]),
        );
    }
    json!({
        "attributes": attributes,
        "indices": geometry.indices.as_ref().map(|indices| {
            json!([index_values_name(geometry, indices), typed_values(indices)])
        }),
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

/// Cesium's debug build throws `DeveloperError` when instances mix
/// `primitiveType`s. Release builds strip that check, and this port combines
/// the instances under the first geometry's type.
#[test]
fn mixed_primitive_types_keep_the_first() {
    let cases = load("workers", "combineGeometry");
    let case = cases
        .iter()
        .find(|case| case.r.get("kind").and_then(Value::as_str) == Some("DeveloperError"))
        .expect("mixed primitiveType fixture");
    let result = combine(&case.a[0]).expect("mixed primitive types combine");
    let geometries = result.geometries.expect("geometries");
    assert_eq!(geometries.len(), 1);
    assert_eq!(geometries[0].primitive_type, PrimitiveType::Triangles);
    let indices = geometries[0].indices.as_ref().expect("indices");
    // BoxGeometry contributes 36 indices. The line geometry is concatenated after them.
    assert!(indices.len() > 36, "line indices were not concatenated, len {}", indices.len());
}

#[test]
fn scene_primitive_pipeline_replays_the_pinned_combine_cases() {
    use cesium_rs::scene::primitive_pipeline::{CombineGeometryParameters, PrimitivePipeline};
    let combine_scene = |script: &Value| -> Result<CombineGeometryResult, RuntimeError> {
        let packed = f64s(&script["packedInstances"]);
        let geometries = script["createGeometryResults"]
            .as_array()
            .unwrap()
            .iter()
            .map(|v| packed_results(v).unpack())
            .collect::<Result<Vec<_>, _>>()?
            .into_iter()
            .flatten()
            .collect::<Vec<_>>();
        let mut instances = Vec::new();
        for (i, data) in packed[1..].chunks_exact(19).enumerate() {
            instances.push(GeometryInstance {
                geometry: geometries.get(i).cloned().flatten(),
                model_matrix: Matrix4(data[..16].try_into().unwrap()),
                id: None,
                attributes: vec![(
                    "offset".into(),
                    OffsetGeometryInstanceAttribute { value: [data[16] as f32, data[17] as f32, data[18] as f32] }
                        .into(),
                )],
                west_hemisphere_geometry: None,
                east_hemisphere_geometry: None,
            });
        }
        let radii = f64s(&script["ellipsoid"]);
        let result = PrimitivePipeline::combine_geometry(CombineGeometryParameters {
            instances,
            ellipsoid_radii: Cartesian3::new(radii[0], radii[1], radii[2]),
            is_geographic: flag(script, "isGeographic"),
            uint_index_support: flag(script, "elementIndexUintSupported"),
            scene_3d_only: flag(script, "scene3DOnly"),
            vertex_cache_optimize: flag(script, "vertexCacheOptimize"),
            compress_vertices: flag(script, "compressVertices"),
            model_matrix: matrix(&script["modelMatrix"]),
            create_pick_offsets: flag(script, "createPickOffsets"),
        })?;
        let pack = |spheres: Vec<Option<BoundingSphere>>| {
            let mut output = vec![spheres.len() as f32];
            for s in spheres {
                match s {
                    Some(s) => {
                        output.extend([1., s.center.x as f32, s.center.y as f32, s.center.z as f32, s.radius as f32])
                    }
                    None => output.extend([0.; 5]),
                }
            }
            output
        };
        Ok(CombineGeometryResult {
            geometries: result.geometries,
            model_matrix: result.model_matrix,
            attribute_locations: result.attribute_locations,
            pick_offsets: result.pick_offsets.map(|v| {
                v.into_iter().map(|p| PickOffset { index: p.index, offset: p.offset, count: p.count }).collect()
            }),
            offset_instance_extend: result.offset_instance_extend,
            bounding_spheres: pack(result.bounding_spheres),
            bounding_spheres_cv: pack(result.bounding_spheres_cv),
        })
    };
    run(
        "workers",
        "combineGeometry",
        |_| EXACT,
        |f, a| {
            if f != "combineGeometry" {
                return None;
            }
            Some(match combine_scene(&a[0]) {
                Ok(result) => Out::Ok(encode(result)),
                Err(error) => Out::Err(error.to_string()),
            })
        },
    );
}
