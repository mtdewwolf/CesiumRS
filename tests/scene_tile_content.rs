//! preprocess3DTileContent, TileReplacementQueue and Cesium3DContentGroup
//! against CesiumJS (`tools/cesium/fixtures/scene-tile-content.mjs`).
mod common;
use cesium_rs::scene::*;
use common::*;
use serde_json::{json, Value};
use std::sync::Arc;

#[test]
fn preprocess_tile_content() {
    run(
        "scene",
        "preprocess3DTileContent",
        |_| EXACT,
        |_, a| {
            let bytes: Vec<u8> = a[0].as_array().unwrap().iter().map(|b| b.as_u64().unwrap() as u8).collect();
            Some(match preprocess_3d_tile_content(&bytes) {
                Ok(r) => {
                    let (binary, json) = match r.payload {
                        TileContentPayload::Binary(b) => {
                            assert!(std::ptr::eq(b, bytes.as_slice()), "the binary payload is the input");
                            (json!(b.len()), Value::Null)
                        }
                        TileContentPayload::Json(v) => (Value::Null, v),
                    };
                    Out::Ok(
                        json!({ "contentType": r.content_type.as_str(), "binaryPayload": binary, "jsonPayload": json }),
                    )
                }
                Err(e) => Out::Err(e.message),
            })
        },
    );
}

#[test]
fn tile_replacement_queue() {
    run(
        "scene",
        "TileReplacementQueue",
        |_| EXACT,
        |_, a| {
            let ineligible: Vec<u64> = a[1].as_array().unwrap().iter().map(|v| v.as_u64().unwrap()).collect();
            let mut queue = TileReplacementQueue::<u64>::new();
            let steps: Vec<Value> = a[2]
                .as_array()
                .unwrap()
                .iter()
                .map(|op| {
                    let mut freed = Vec::new();
                    match s(&op[0]) {
                        "render" => queue.mark_tile_rendered(op[1].as_u64().unwrap()),
                        "start" => queue.mark_start_of_render_frame(),
                        _ => queue.trim_tiles(u(&op[1]) as usize, |tile| {
                            let eligible = !ineligible.contains(&tile);
                            if eligible {
                                freed.push(tile);
                            }
                            eligible
                        }),
                    }
                    let forward: Vec<u64> = queue.iter().collect();
                    let backward: Vec<u64> =
                        std::iter::successors(queue.tail(), |&t| queue.replacement_previous(t)).collect();
                    json!([forward, backward, queue.count(), queue.last_before_start_of_frame(), freed])
                })
                .collect();
            ok(Value::Array(steps))
        },
    );
}

#[test]
fn content_group() {
    run(
        "scene",
        "Cesium3DContentGroup",
        |_| EXACT,
        |_, a| {
            let schema = MetadataSchema::from_json(&a[0]).unwrap();
            let class: Arc<MetadataClass> = schema.classes.get("layer").unwrap().clone();
            let metadata = GroupMetadata::new(&a[1], class, a[2].as_str().map(str::to_string)).unwrap();
            let group = Cesium3DContentGroup::new(metadata.clone());
            let m = group.metadata();
            let value = |id: &str| match m.get_property(id).unwrap() {
                None => Value::Null,
                Some(MetadataValue::String(s)) => json!(s),
                Some(MetadataValue::Number(n)) => num(n),
                Some(v) => panic!("unexpected {v:?}"),
            };
            ok(json!([
                *m == metadata,
                m.id(),
                m.class().id,
                m.get_property_ids(),
                value("name"),
                value("priority"),
                m.extras().cloned().unwrap_or(Value::Null),
            ]))
        },
    );
}
