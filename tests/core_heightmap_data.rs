//! Heightmap terrain lifecycle, packed samples, LERC and upsampling parity.
mod common;
use cesium_rs::core::*;
use common::*;
use serde_json::{json, Value};

fn create(o: &Value) -> HeightmapTerrainData {
    let kind = match s(&o["type"]) {
        "Int8Array" => ComponentDatatype::Byte,
        "Uint8Array" => ComponentDatatype::UnsignedByte,
        "Int16Array" => ComponentDatatype::Short,
        "Uint16Array" => ComponentDatatype::UnsignedShort,
        "Int32Array" => ComponentDatatype::Int,
        "Uint32Array" => ComponentDatatype::UnsignedInt,
        "Float32Array" => ComponentDatatype::Float,
        _ => ComponentDatatype::Double,
    };
    let mut options = HeightmapTerrainDataOptions::new(
        TypedArray::from_values(kind, &nums(&o["buffer"])),
        u(&o["width"]) as usize,
        u(&o["height"]) as usize,
    );
    options.encoding = if opt(&o["encoding"]) == Some(1.0) { HeightmapEncoding::Lerc } else { HeightmapEncoding::None };
    options.child_tile_mask = opt(&o["mask"]).unwrap_or(15.0) as u8;
    options.water_mask = (!o["water"].is_null()).then(|| nums(&o["water"]).into_iter().map(|v| v as u8).collect());
    options.created_by_upsampling = o["upsampled"].as_bool().unwrap_or(false);
    let q = &o["structure"];
    options.structure = HeightmapTerrainStructure {
        samples: HeightmapStructure {
            height_scale: opt(&q["heightScale"]).unwrap_or(1.0),
            height_offset: opt(&q["heightOffset"]).unwrap_or(0.0),
            elements_per_height: opt(&q["elementsPerHeight"]).unwrap_or(1.0) as usize,
            stride: opt(&q["stride"]).unwrap_or(1.0) as usize,
            element_multiplier: opt(&q["elementMultiplier"]).unwrap_or(256.0),
            is_big_endian: q["isBigEndian"].as_bool().unwrap_or(false),
        },
        lowest_encoded_height: opt(&q["lowestEncodedHeight"]),
        highest_encoded_height: opt(&q["highestEncodedHeight"]),
    };
    HeightmapTerrainData::new(options).unwrap()
}
fn metadata(d: &HeightmapTerrainData) -> Value {
    let s = d.structure();
    let q = s.samples;
    json!([Value::Null,d.water_mask(),d.child_tile_mask(),d.was_created_by_upsampling(),{"heightScale":q.height_scale,"heightOffset":q.height_offset,"elementsPerHeight":q.elements_per_height,"stride":q.stride,"elementMultiplier":q.element_multiplier,"isBigEndian":q.is_big_endian,"lowestEncodedHeight":s.lowest_encoded_height,"highestEncodedHeight":s.highest_encoded_height},d.buffer().map(|b|b.to_f64_vec().enc()),d.mesh().map(|m|m.encoding.quantization as u8),d.skirt_height()])
}
fn mesh_metadata(m: &TerrainMesh) -> Value {
    json!([
        m.center.enc(),
        m.vertices.iter().map(|&v| v as f64).collect::<Vec<_>>().enc(),
        typed_array_name(&m.indices),
        m.indices.to_f64_vec().enc(),
        m.index_count_without_skirts,
        m.vertex_count_without_skirts,
        m.minimum_height,
        m.maximum_height,
        m.rectangle.enc(),
        m.bounding_sphere_3d.enc(),
        m.occludee_point_in_scaled_space.enc(),
        m.stride,
        m.oriented_bounding_box.map(|b| json!([b.center.enc(), b.half_axes.enc()])),
        [
            m.edges.west_indices_south_to_north,
            m.edges.south_indices_east_to_west,
            m.edges.east_indices_north_to_south,
            m.edges.north_indices_west_to_east
        ],
        m.encoding.quantization as u8,
        m.encoding.exaggeration,
        m.encoding.exaggeration_relative_height
    ])
}
fn scheme(merc: bool) -> Box<dyn TilingScheme> {
    if merc {
        Box::new(WebMercatorTilingScheme::default())
    } else {
        Box::new(GeographicTilingScheme::default())
    }
}
fn interpolate(d: &HeightmapTerrainData, r: &Rectangle, coordinates: &Value) -> Vec<Value> {
    coordinates
        .as_array()
        .unwrap()
        .iter()
        .map(|p| {
            d.interpolate_height(r, r.west + (r.east - r.west) * n(&p[0]), r.south + (r.north - r.south) * n(&p[1]))
                .enc()
        })
        .collect()
}

#[test]
fn heightmap_terrain_data() {
    run(
        "core",
        "HeightmapTerrainData",
        |_| TIGHT,
        |f, a| match f {
            "lifecycle" => {
                let mut d = create(&a[0]);
                let ts = scheme(a[1].as_bool().unwrap());
                let tile = TerrainTile { x: u(&a[2][0]), y: u(&a[2][1]), level: u(&a[2][2]) };
                let r = ts.tile_xy_to_rectangle(tile.x as f64, tile.y as f64, tile.level);
                let masks: Vec<_> = (0..9)
                    .map(|j| d.is_child_available(tile.x, tile.y, tile.x * 2 + j % 3, tile.y * 2 + j / 3))
                    .collect();
                let dest = TerrainTile { x: tile.x * 2, y: tile.y * 2, level: tile.level + 1 };
                assert!(d.upsample_sync(ts.as_ref(), tile, dest).is_none());
                let initial = json!([metadata(&d), interpolate(&d, &r, &a[3]), masks, Value::Null]);
                let mut request = TerrainMeshRequest::new(ts.as_ref(), tile);
                request.exaggeration = n(&a[4]);
                request.exaggeration_relative_height = n(&a[5]);
                request.heightmap_quality = Some(n(&a[6]));
                let mesh = d.create_mesh_sync(request).ok()?;
                let mut upsampled = Vec::new();
                for y in 0..2 {
                    for x in 0..2 {
                        let dest = TerrainTile { x: tile.x * 2 + x, y: tile.y * 2 + y, level: tile.level + 1 };
                        let child = d.upsample_sync(ts.as_ref(), tile, dest)?;
                        let r = ts.tile_xy_to_rectangle(dest.x as f64, dest.y as f64, dest.level);
                        upsampled.push(json!([metadata(&child), interpolate(&child, &r, &a[3])]));
                    }
                }
                ok(json!([initial, mesh_metadata(&mesh), metadata(&d), interpolate(&d, &r, &a[3]), upsampled]))
            }
            "lerc" => {
                let mut d = create(&a[0]);
                let ts = scheme(a[1].as_bool().unwrap());
                let tile = TerrainTile { x: 0, y: 0, level: 7 };
                let r = ts.tile_xy_to_rectangle(0.0, 0.0, 7);
                let initial = d.interpolate_height(&r, r.west, r.south).enc();
                match d.create_mesh_sync(TerrainMeshRequest::new(ts.as_ref(), tile)) {
                    Ok(mesh) => ok(json!([
                        initial,
                        mesh_metadata(&mesh),
                        metadata(&d),
                        d.interpolate_height(&r, (r.west + r.east) / 2.0, (r.south + r.north) / 2.0).enc()
                    ])),
                    Err(_) => ok(json!([initial, "RuntimeError", d.buffer().is_some()])),
                }
            }
            _ => None,
        },
    );
}

#[test]
fn terrain_data_contract_and_mesh_ownership() {
    use std::future::Future;
    use std::pin::Pin;
    use std::task::{Context, Poll, Waker};
    let mut d = HeightmapTerrainData::new(HeightmapTerrainDataOptions::new(
        TypedArray::Float32(vec![10.0, 20.0, 30.0, 40.0]),
        2,
        2,
    ))
    .unwrap();
    assert!(TerrainData::credits(&d).is_none());
    assert!(TerrainData::water_mask(&d).is_none());
    let ts = GeographicTilingScheme::default();
    let tile = TerrainTile { x: 0, y: 0, level: 6 };
    let mut future = TerrainData::create_mesh(&mut d, TerrainMeshRequest::new(&ts, tile)).unwrap();
    let mesh = match Pin::new(&mut future).poll(&mut Context::from_waker(Waker::noop())) {
        Poll::Ready(Ok(mesh)) => mesh,
        _ => panic!("native terrain task was not ready"),
    };
    drop(future);
    assert!(d.buffer().is_none());
    d.mesh_mut().unwrap().encoding.exaggeration = 3.0;
    assert_eq!(mesh.encoding.exaggeration, 1.0);
    assert_eq!(d.mesh().unwrap().encoding.exaggeration, 3.0);
    assert!(d.create_mesh_sync(TerrainMeshRequest::new(&ts, tile)).is_err());
    let child = TerrainTile { x: 0, y: 0, level: 7 };
    assert!(TerrainData::upsample(&d, &ts, tile, child).is_some());
    assert!(TerrainData::is_child_available(&d, 0, 0, 0, 0));
    assert!(!TerrainData::was_created_by_upsampling(&d));
    let r = ts.tile_xy_to_rectangle(0.0, 0.0, 6);
    assert!(TerrainData::interpolate_height(&d, &r, r.west, r.south).is_some());
    assert_eq!(cesium_rs::core::terrain_data::MAXIMUM_ASYNCHRONOUS_TASKS, 5);
}
