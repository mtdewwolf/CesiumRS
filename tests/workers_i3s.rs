//! `decodeI3S` against the real CesiumJS worker.
//! Blob URLs in the fixture are the bytes they referenced.
mod common;
use cesium_rs::core::{Cartesian3, Cartographic, Matrix3};
use cesium_rs::workers::decode_i3s::{I3sAccessor, I3sPrimitive, I3sPropertyTable};
use cesium_rs::workers::*;
use common::{m3, n, num, nums, run, Out, EXACT};
use serde_json::{json, Map, Value};

fn cartesian_obj(v: &Value) -> Cartesian3 {
    Cartesian3::new(n(&v["x"]), n(&v["y"]), n(&v["z"]))
}

fn strings(v: &Value) -> Vec<String> {
    v.as_array().unwrap().iter().map(|s| s.as_str().unwrap().to_string()).collect()
}

fn parameters(bytes: &Value, options: &Value) -> I3sDecodeParameters {
    let field = |key: &str| options.get(key).filter(|v| !v.is_null());
    I3sDecodeParameters {
        binary_data: bytes.as_array().unwrap().iter().map(|b| n(b) as u8).collect(),
        ordering: field("ordering").map(strings),
        feature_attribute_order: field("featureAttributeOrder").map(strings),
        buffer_attributes: field("bufferAttributes").map(strings),
        geoid_data_list: field("geoidDataList")
            .map(|list| list.as_array().unwrap().iter().map(geoid).collect())
            .unwrap_or_default(),
        cartographic_center: field("cartographicCenter")
            .map(|v| Cartographic::new(n(&v["longitude"]), n(&v["latitude"]), n(&v["height"]))),
        cartesian_center: field("cartesianCenter").map(cartesian_obj),
        parent_rotation: field("parentRotation").map(m3),
        ellipsoid_radii_square: field("ellipsoidRadiiSquare").map(cartesian_obj),
        url: field("url").map(|v| v.as_str().unwrap().to_string()),
        enable_features: field("enableFeatures").is_some_and(|v| v.as_bool().unwrap()),
        split_geometry_by_color_transparency: field("splitGeometryByColorTransparency")
            .is_some_and(|v| v.as_bool().unwrap()),
        calculate_normals: field("calculateNormals").is_some_and(|v| v.as_bool().unwrap()),
        symbology: field("symbologyData").map(symbology),
    }
}

fn geoid(v: &Value) -> I3sGeoid {
    let extent = &v["nativeExtent"];
    I3sGeoid {
        west: n(&extent["west"]),
        south: n(&extent["south"]),
        east: n(&extent["east"]),
        north: n(&extent["north"]),
        width: n(&v["width"]),
        height: n(&v["height"]),
        buffer: nums(&v["buffer"]),
        scale: n(&v["scale"]),
        offset: n(&v["offset"]),
        web_mercator: v["projectionType"].as_str() == Some("WebMercator"),
        web_mercator_radii: v.get("projection").map(|p| cartesian_obj(&p["_ellipsoid"]["_radii"])),
    }
}

fn symbology(v: &Value) -> I3sSymbology {
    I3sSymbology {
        entries: v
            .as_object()
            .unwrap()
            .iter()
            .map(|(name, value)| {
                let edges = value.get("edges").is_some_and(|edges| !edges.is_null());
                (name.clone(), edges)
            })
            .collect(),
    }
}

fn vec3(values: [f64; 3]) -> Value {
    Value::Array(values.into_iter().map(num).collect())
}

fn accessor_json(accessor: &I3sAccessor) -> Value {
    let mut value = json!({
        "bufferView": accessor.buffer_view,
        "byteOffset": accessor.byte_offset,
        "componentType": accessor.component_type,
        "count": accessor.count,
        "type": accessor.type_name,
    });
    if let Some(min) = accessor.min {
        value["min"] = vec3(min);
    }
    if let Some(max) = accessor.max {
        value["max"] = vec3(max);
    }
    value
}

fn extensions_json(primitive: &I3sPrimitive) -> Value {
    let mut extensions = Map::new();
    if let Some(count) = primitive.extensions.feature_count {
        extensions.insert(
            "EXT_mesh_features".to_string(),
            json!({"featureIds": [{"attribute": 0, "propertyTable": 0, "featureCount": num(count)}]}),
        );
    }
    if let Some(indices) = primitive.extensions.outline {
        extensions.insert("CESIUM_primitive_outline".to_string(), json!({"indices": indices}));
    }
    Value::Object(extensions)
}

fn structural_metadata(table: &I3sPropertyTable) -> Value {
    json!({
        "EXT_structural_metadata": {
            "schema": {
                "id": "i3s-metadata-schema-001",
                "name": "I3S metadata schema 001",
                "description": "The schema for I3S metadata",
                "version": "1.0",
                "classes": {
                    "feature": {
                        "name": "feature",
                        "description": "Feature metadata",
                        "properties": {
                            "index": {
                                "description": "The feature index",
                                "type": "SCALAR",
                                "componentType": "FLOAT32",
                                "required": true
                            }
                        }
                    }
                }
            },
            "propertyTables": [{
                "name": "feature-indices-mapping",
                "class": "feature",
                "count": num(table.count),
                "properties": {"index": {"values": table.values}}
            }]
        }
    })
}

fn cartesian_json(value: Cartesian3) -> Value {
    json!({"x": num(value.x), "y": num(value.y), "z": num(value.z)})
}

fn matrix_json(value: Matrix3) -> Value {
    Value::Array(value.0.into_iter().map(num).collect())
}

fn feature_json(values: &[Option<f64>]) -> Value {
    Value::Array(values.iter().map(|value| value.map(num).unwrap_or(Value::Null)).collect())
}

fn mesh_json(mesh: &I3sMeshData) -> Value {
    let mut primitives = Vec::new();
    for mesh in &mesh.meshes {
        let mut encoded = Vec::new();
        for primitive in &mesh.primitives {
            let mut attributes = Map::new();
            for (name, index) in &primitive.attributes {
                attributes.insert((*name).to_string(), json!(index));
            }
            let mut value = json!({
                "attributes": attributes,
                "indices": primitive.indices,
                "material": primitive.material,
                "extensions": extensions_json(primitive),
            });
            if primitive.transparent {
                value["extra"] = json!({"isTransparent": true});
            }
            encoded.push(value);
        }
        primitives.push(json!({"primitives": encoded}));
    }
    let mut doc = json!({
        "buffers": mesh.buffers.iter().map(|bytes| json!({"byteLength": bytes.len(), "bytes": bytes})).collect::<Vec<_>>(),
        "bufferViews": mesh.buffer_views.iter().map(|view| json!({
            "buffer": view.buffer,
            "byteOffset": view.byte_offset,
            "byteLength": view.byte_length,
            "target": view.target,
        })).collect::<Vec<_>>(),
        "accessors": mesh.accessors.iter().map(accessor_json).collect::<Vec<_>>(),
        "meshes": primitives,
        "nodes": mesh.nodes.iter().map(|node| json!({"mesh": node.mesh})).collect::<Vec<_>>(),
        "nodesInScene": mesh.nodes_in_scene,
    });
    if let Some(root) = &mesh.root_extensions {
        doc["rootExtensions"] = match &root.property_table {
            Some(table) => structural_metadata(table),
            None => json!({}),
        };
    }
    if let Some(used) = &mesh.extensions_used {
        doc["extensionsUsed"] = json!(used);
    }
    doc["_customAttributes"] = json!({
        "positions": mesh.positions.as_ref().map(|values| Value::Array(values.iter().copied().map(num).collect())),
        "indices": mesh.indices,
        "featureIndex": mesh.feature_index.as_ref().map(|values| feature_json(values)),
        "sourceURL": mesh.source_url,
        "cartesianCenter": mesh.cartesian_center.map(cartesian_json),
        "parentRotation": mesh.parent_rotation.map(matrix_json),
    });
    doc
}

#[test]
fn decode_i3s_reference() {
    run(
        "workers",
        "decodeI3S",
        |_| EXACT,
        |name, args| match name {
            "init" => Some(Out::Ok(json!(true))),
            "decode" => {
                let decoded = decode_i3s(&parameters(&args[0], &args[1]));
                Some(match decoded {
                    Ok(mesh) => Out::Ok(mesh_json(&mesh)),
                    Err(error) => Out::Err(error.to_string()),
                })
            }
            _ => None,
        },
    );
}

fn wgs84_frame(bytes: Vec<u8>, ordering: Vec<String>) -> I3sDecodeParameters {
    let ellipsoid = cesium_rs::core::Ellipsoid::WGS84;
    let center = Cartographic::new(0.0, 0.0, 0.0);
    I3sDecodeParameters {
        binary_data: bytes,
        ordering: Some(ordering),
        feature_attribute_order: Some(Vec::new()),
        buffer_attributes: None,
        geoid_data_list: Vec::new(),
        cartographic_center: Some(center),
        cartesian_center: Some(ellipsoid.cartographic_to_cartesian(&center)),
        parent_rotation: Some(Matrix3::IDENTITY),
        ellipsoid_radii_square: Some(ellipsoid.radii_squared),
        url: None,
        enable_features: true,
        split_geometry_by_color_transparency: false,
        calculate_normals: false,
        symbology: None,
    }
}

#[test]
fn huge_face_range_returns_an_error() {
    let mut bytes = Vec::new();
    bytes.extend_from_slice(&3u32.to_le_bytes());
    bytes.extend_from_slice(&1u32.to_le_bytes());
    bytes.extend_from_slice(&[0u8; 3 * 3 * 4]);
    bytes.extend_from_slice(&0u32.to_le_bytes());
    bytes.extend_from_slice(&u32::MAX.to_le_bytes());
    let decoded = decode_i3s(&wgs84_frame(bytes, vec!["position".to_string(), "faceRange".to_string()]));
    assert!(decoded.is_err());
}

#[test]
fn point_cloud_draco_payload_is_rejected() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/workers/decodeI3S.json");
    let fixture: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let bytes = fixture["cases"]
        .as_array()
        .unwrap()
        .iter()
        .filter_map(|case| {
            let values = case["a"][0].as_array()?;
            let bytes: Vec<u8> = values.iter().map(|value| n(value) as u8).collect();
            (bytes.starts_with(b"DRACO") && bytes.get(7).copied() == Some(0)).then_some(bytes)
        })
        .next()
        .expect("point-cloud draco fixture");
    let decoded = std::panic::catch_unwind(|| decode_i3s(&wgs84_frame(bytes, Vec::new())));
    let decoded = decoded.expect("point cloud must not panic").expect("point cloud stays empty geometry");
    assert!(decoded.buffers.is_empty());
    assert!(decoded.positions.is_none());
    assert!(decoded.indices.is_none());
}

#[test]
fn truncated_draco_payload_does_not_panic() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/workers/decodeI3S.json");
    let fixture: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let cases = fixture["cases"].as_array().unwrap();
    let draco = cases
        .iter()
        .filter_map(|case| {
            let bytes = case["a"][0].as_array()?;
            let bytes: Vec<u8> = bytes.iter().map(|value| n(value) as u8).collect();
            bytes.starts_with(b"DRACO").then_some(bytes)
        })
        .max_by_key(Vec::len)
        .expect("draco fixture");
    assert!(draco.len() > 8);
    for length in 0..=draco.len() {
        let parameters = wgs84_frame(draco[..length].to_vec(), Vec::new());
        let decoded = std::panic::catch_unwind(|| decode_i3s(&parameters));
        let decoded = decoded.unwrap_or_else(|_| panic!("decode panicked at {length} bytes"));
        if length < 5 {
            assert!(decoded.is_err(), "short header should throw at {length}");
        } else {
            assert!(decoded.is_ok(), "malformed draco should not throw at {length}");
        }
    }
}
