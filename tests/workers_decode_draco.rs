//! `decodeDraco` against the real CesiumJS worker and the draco3d encoder.
mod common;
use cesium_rs::workers::decode_draco;
use common::*;
use serde_json::Value;

#[test]
fn decode_draco_reference() {
    run(
        "workers",
        "decodeDraco",
        |_| EXACT,
        |_, a| {
            Some(match decode_draco(&a[0]) {
                Ok(value) => Out::Ok(value),
                Err(error) => Out::Err(error.to_string()),
            })
        },
    );
}

#[test]
fn truncated_payloads_do_not_panic() {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/workers/decodeDraco.json");
    let fixture: Value = serde_json::from_str(&std::fs::read_to_string(path).unwrap()).unwrap();
    let buffers = fixture["cases"].as_array().unwrap().iter().filter_map(|case| {
        let values = case["a"][0].get("array").or_else(|| case["a"][0].get("buffer"))?.as_array()?;
        let bytes: Vec<u8> = values.iter().map(|value| value.as_u64().unwrap() as u8).collect();
        bytes.starts_with(b"DRACO").then_some(bytes)
    });
    let mut mesh = Vec::new();
    let mut cloud = Vec::new();
    for bytes in buffers {
        if bytes.get(7).copied() == Some(1) && bytes.len() > mesh.len() {
            mesh = bytes;
        } else if bytes.get(7).copied() == Some(0) && bytes.len() > cloud.len() {
            cloud = bytes;
        }
    }
    assert!(mesh.len() > 11 && cloud.len() > 11);
    for length in 0..=mesh.len() {
        let decoded = std::panic::catch_unwind(|| {
            decode_draco(&serde_json::json!({
                "array": mesh,
                "bufferView": { "byteLength": length },
                "compressedAttributes": {}
            }))
        });
        assert!(decoded.is_ok(), "mesh decode panicked at {length} bytes");
    }
    for length in 0..=cloud.len() {
        let decoded = std::panic::catch_unwind(|| {
            decode_draco(&serde_json::json!({
                "buffer": cloud[..length],
                "properties": {}
            }))
        });
        assert!(decoded.is_ok(), "point cloud decode panicked at {length} bytes");
    }
}

#[test]
fn point_cloud_is_rejected_as_a_mesh() {
    let mut bytes = b"DRACO".to_vec();
    bytes.extend_from_slice(&[1, 0, 0, 0, 0, 0]);
    let error = decode_draco(&serde_json::json!({
        "array": bytes,
        "bufferView": { "byteLength": 11 },
        "compressedAttributes": {}
    }))
    .unwrap_err();
    assert_eq!(error.message, "Unsupported draco mesh geometry type.");
}

#[test]
fn wasm_config_precedes_a_buffer() {
    let value = decode_draco(&serde_json::json!({
        "webAssemblyConfig": { "wasmBinaryFile": "draco_decoder.wasm" },
        "bufferView": { "byteLength": 0 },
        "array": [1, 2, 3]
    }))
    .unwrap();
    assert_eq!(value, Value::Bool(true));
}
