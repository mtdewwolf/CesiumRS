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
