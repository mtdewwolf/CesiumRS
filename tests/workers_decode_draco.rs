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
fn wasm_config_precedes_a_buffer() {
    let value = decode_draco(&serde_json::json!({
        "webAssemblyConfig": { "wasmBinaryFile": "draco_decoder.wasm" },
        "bufferView": { "byteLength": 0 },
        "array": [1, 2, 3]
    }))
    .unwrap();
    assert_eq!(value, Value::Bool(true));
}
