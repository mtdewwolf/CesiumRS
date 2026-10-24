//! Cesium3DTileFeatureTable, B3dmParser, I3dmParser and PntsParser against
//! CesiumJS (`tools/cesium/fixtures/scene-tile-formats.mjs`).
mod common;
use cesium_rs::core::*;
use cesium_rs::scene::gltf_pipeline::BufferSource;
use cesium_rs::scene::*;
use common::*;
use serde_json::{json, Value};
use std::sync::Arc;

fn typed(a: &TypedArray) -> Value {
    json!({"type": typed_array_name(a), "values": a.to_f64_vec().enc()})
}

fn global(g: &GlobalProperty) -> Value {
    match g {
        GlobalProperty::Json(v) => v.clone(),
        GlobalProperty::Binary(a) => typed(a),
    }
}

fn bytes(v: &Value) -> Arc<Vec<u8>> {
    Arc::new(v.as_array().unwrap().iter().map(|b| b.as_u64().unwrap() as u8).collect())
}

/// A view's place in the tile buffer, or a copy.
fn view(v: &BufferSource, tile: &Arc<Vec<u8>>) -> Value {
    let copied = !Arc::ptr_eq(&v.buffer, tile);
    json!({"copied": copied, "byteOffset": if copied { Value::Null } else { json!(v.byte_offset) }, "bytes": v.bytes()})
}

fn batch_binary(b: &Option<Vec<u8>>) -> Value {
    // Always a copy, so the tile buffer can be released.
    b.as_ref().map_or(Value::Null, |b| json!({"copied": true, "bytes": b}))
}

fn warnings(w: &[TileFormatWarning]) -> Vec<&'static str> {
    w.iter().map(|w| w.id()).collect()
}

fn datatype(v: &Value) -> ComponentDatatype {
    ComponentDatatype::from_gl(u(v)).expect("component datatype")
}

#[test]
fn feature_table() {
    run("scene", "Cesium3DTileFeatureTable", |_| EXACT, |f, a| {
        (f == "sequence").then_some(())?;
        let bin = bytes(&a[1]);
        let mut buffer = vec![0u8; 8];
        buffer.extend_from_slice(&bin);
        let mut table = Cesium3DTileFeatureTable::new(a[0].clone(), BufferSource::view(Arc::new(buffer), 8, bin.len()));
        table.features_length = 2.0;
        let mut out = Vec::new();
        for op in a[2].as_array()? {
            let semantic = op.get(1).and_then(Value::as_str).unwrap_or("");
            let result = match s(&op[0]) {
                "featuresLength" => {
                    table.features_length = n(&op[1]);
                    Ok(Value::Null)
                }
                "global" => table
                    .get_global_property(semantic, op.get(2).map(datatype), op.get(3).map(|v| u(v) as usize))
                    .map(|g| g.as_ref().map_or(Value::Null, global)),
                "has" => Ok(json!(table.has_property(semantic))),
                "array" => table
                    .get_property_array(semantic, datatype(&op[2]), u(&op[3]) as usize)
                    .map(|a| a.as_deref().map_or(Value::Null, typed)),
                _ => {
                    let length = u(&op[3]) as usize;
                    table.get_property(semantic, datatype(&op[2]), length, u(&op[4]) as usize).map(|values| match values {
                        None => Value::Null,
                        // componentLength 1 returns the element itself.
                        Some(v) if length == 1 => v[0].map_or(Value::Null, |x| x.enc()),
                        Some(v) => v.iter().map(|x| x.map_or(Value::Null, |x| x.enc())).collect(),
                    })
                }
            };
            match result {
                Ok(v) => out.push(v),
                Err(e) => return Some(Out::Err(e.to_string())),
            }
        }
        ok(Value::Array(out))
    });
}

#[test]
fn b3dm_parser() {
    run("scene", "B3dmParser", |_| EXACT, |_, a| {
        let tile = bytes(&a[0]);
        Some(match b3dm_parser::parse(&tile, u(&a[1]) as usize) {
            Ok(p) => Out::Ok(json!({
                "batchLength": p.batch_length, "featureTableJson": p.feature_table_json,
                "featureTableBinary": view(&p.feature_table_binary, &tile), "batchTableJson": p.batch_table_json,
                "batchTableBinary": batch_binary(&p.batch_table_binary), "gltf": view(&p.gltf, &tile), "warnings": warnings(&p.warnings),
            })),
            Err(e) => Out::Err(e.message),
        })
    });
}

#[test]
fn i3dm_parser() {
    run("scene", "I3dmParser", |_| EXACT, |_, a| {
        let tile = bytes(&a[0]);
        Some(match i3dm_parser::parse(&tile, u(&a[1]) as usize) {
            Ok(p) => Out::Ok(json!({
                "gltfFormat": p.gltf_format, "featureTableJson": p.feature_table_json,
                "featureTableBinary": view(&p.feature_table_binary, &tile), "batchTableJson": p.batch_table_json,
                "batchTableBinary": batch_binary(&p.batch_table_binary), "gltf": view(&p.gltf, &tile), "warnings": warnings(&p.warnings),
            })),
            Err(e) => Out::Err(e.message),
        })
    });
}

fn opt_enc<T: Enc>(v: Option<T>) -> Value {
    v.map_or(Value::Null, |v| v.enc())
}

fn attribute(a: &Option<PntsAttribute>) -> Value {
    let Some(a) = a else { return Value::Null };
    json!({
        "name": a.name, "semantic": a.semantic.as_str(), "setIndex": a.set_index,
        "typedArray": a.typed_array.as_deref().map_or(Value::Null, typed), "constantColor": opt_enc(a.constant_color),
        "componentDatatype": a.component_datatype as u32, "type": a.attribute_type.as_str(), "normalized": a.normalized,
        "isQuantized": a.is_quantized, "isRGB565": a.is_rgb565, "isTranslucent": a.is_translucent,
        "octEncoded": a.oct_encoded, "octEncodedZXY": a.oct_encoded_zxy, "quantizedRange": a.quantized_range,
        "quantizedVolumeOffset": opt_enc(a.quantized_volume_offset), "quantizedVolumeScale": opt_enc(a.quantized_volume_scale),
        "quantizedComponentDatatype": a.quantized_component_datatype.map(|d| d as u32), "quantizedType": a.quantized_type.map(|t| t.as_str()),
    })
}

#[test]
fn pnts_parser() {
    run("scene", "PntsParser", |_| TIGHT, |_, a| {
        let tile = bytes(&a[0]);
        Some(match pnts_parser::parse(&tile, u(&a[1]) as usize) {
            Ok(p) => Out::Ok(json!({
                "draco": p.draco.as_ref().map(|d| json!({
                    "buffer": d.buffer, "featureTableProperties": d.feature_table_properties,
                    "batchTableProperties": d.batch_table_properties, "properties": d.properties, "dequantizeInShader": d.dequantize_in_shader,
                })),
                "hasPositions": p.has_positions, "hasColors": p.has_colors, "hasNormals": p.has_normals, "hasBatchIds": p.has_batch_ids,
                "isTranslucent": p.is_translucent, "hasConstantColor": p.has_constant_color, "rtcCenter": opt_enc(p.rtc_center),
                "pointsLength": global(&p.points_length), "positions": attribute(&p.positions), "normals": attribute(&p.normals),
                "colors": attribute(&p.colors), "batchIds": attribute(&p.batch_ids), "batchLength": p.batch_length.as_ref().map(global),
                "batchTableJson": p.batch_table_json, "batchTableBinary": p.batch_table_binary,
            })),
            Err(e) => Out::Err(e.message),
        })
    });
}

#[test]
fn pnts_reports_removed_batch_table_properties() {
    // Upstream logs a one-time warning per removed name; Rust returns the names.
    let fixture: Value =
        serde_json::from_str(&std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/scene/PntsParser.json")).unwrap())
            .unwrap();
    let case = fixture["cases"].as_array().unwrap().iter().find(|c| c["f"] == "batchTableNoBinary").unwrap();
    let parsed = pnts_parser::parse(&bytes(&case["a"][0]), 0).unwrap();
    assert_eq!(parsed.removed_batch_table_properties, ["3", "a"]);
    let draco = fixture["cases"].as_array().unwrap().iter().find(|c| c["f"] == "dracoWithoutBinary").unwrap();
    assert_eq!(pnts_parser::parse(&bytes(&draco["a"][0]), 0).unwrap().removed_batch_table_properties, ["missing"]);
}
