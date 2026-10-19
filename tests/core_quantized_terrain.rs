//! Quantized terrain data and both real Cesium workers, including full buffers.
mod common;
use cesium_rs::core::*;
use cesium_rs::core::{quantized_terrain_processor as processor, quantized_terrain_upsample as upsample};
use common::*;
use serde_json::{json, Value};

fn options(o: &Value) -> QuantizedMeshTerrainDataOptions<String> {
    let vertices = nums(&o["vertices"]).into_iter().map(|n| n as u16).collect();
    let indices = nums(&o["indices"]).into_iter().map(|n| n as u32).collect::<Vec<_>>();
    let indices = if s(&o["indexType"]) == "Uint32Array" {
        TypedArray::Uint32(indices)
    } else {
        TypedArray::Uint16(indices.iter().map(|&n| n as u16).collect())
    };
    let sphere = &o["sphere"];
    let mut options = QuantizedMeshTerrainDataOptions::new(
        vertices,
        indices,
        [n(&o["minimum"]), n(&o["maximum"])],
        BoundingSphere::new(Cartesian3::new(n(&sphere[0]), n(&sphere[1]), n(&sphere[2])), n(&sphere[3])),
        c3(&o["horizon"]),
    );
    options.encoded_normals =
        (!o["normals"].is_null()).then(|| nums(&o["normals"]).into_iter().map(|v| v as u8).collect());
    let edge = |i| nums(&o["edges"][i]).into_iter().map(|n| n as u32).collect();
    options.edges = QuantizedTerrainEdges { west: edge(0), south: edge(1), east: edge(2), north: edge(3) };
    options.skirt_heights = nums(&o["skirts"]).try_into().unwrap();
    options.oriented_bounding_box =
        (!o["box"].is_null()).then(|| OrientedBoundingBox::new(c3(&o["box"][0]), m3(&o["box"][1])));
    options.child_tile_mask = u(&o["mask"]) as u8;
    options.created_by_upsampling = o["upsampled"].as_bool().unwrap();
    options.credits = o["credits"].as_array().map(|values| values.iter().map(|v| s(v).to_owned()).collect());
    options.water_mask = (!o["water"].is_null()).then(|| nums(&o["water"]).into_iter().map(|n| n as u8).collect());
    options
}
fn scheme(merc: bool) -> Box<dyn TilingScheme> {
    if merc {
        Box::new(WebMercatorTilingScheme::default())
    } else {
        Box::new(GeographicTilingScheme::default())
    }
}
fn tile(v: &Value) -> TerrainTile {
    TerrainTile { x: u(&v[0]), y: u(&v[1]), level: u(&v[2]) }
}
fn e_meta(e: &TerrainEncoding) -> Value {
    json!([
        e.quantization as u8,
        e.minimum_height,
        e.maximum_height,
        e.stride,
        e.has_vertex_normals,
        e.has_web_mercator_t,
        e.has_geodetic_surface_normals,
        e.exaggeration,
        e.exaggeration_relative_height,
        e.center.enc(),
        e.matrix.enc(),
        e.to_scaled_enu.enc(),
        e.from_scaled_enu.enc()
    ])
}
fn edge_meta(e: &TerrainEdgeIndices) -> Value {
    json!([
        e.west_indices_south_to_north,
        e.south_indices_east_to_west,
        e.east_indices_north_to_south,
        e.north_indices_west_to_east
    ])
}
fn q_edges(e: &QuantizedTerrainEdges) -> Value {
    json!([e.west, e.south, e.east, e.north])
}
fn obb(b: Option<OrientedBoundingBox>) -> Value {
    b.map(|b| json!([b.center.enc(), b.half_axes.enc()])).unwrap_or(Value::Null)
}
fn floats(v: &[f32]) -> Value {
    v.iter().map(|&v| v as f64).collect::<Vec<_>>().enc()
}
fn raw(d: &QuantizedMeshTerrainData) -> Value {
    let skirts = d.skirt_heights();
    json!([
        d.quantized_vertices(),
        d.indices().map(typed_array_name),
        d.indices().map(|v| v.to_f64_vec().enc()),
        d.encoded_normals(),
        d.edges().map(q_edges),
        d.minimum_height(),
        d.maximum_height(),
        d.bounding_sphere().enc(),
        obb(d.oriented_bounding_box()),
        d.horizon_occlusion_point().enc(),
        skirts[0],
        skirts[1],
        skirts[2],
        skirts[3],
        d.credits(),
        d.water_mask(),
        d.child_tile_mask(),
        d.was_created_by_upsampling(),
        d.can_upsample()
    ])
}
fn mesh(m: &TerrainMesh) -> Value {
    json!([
        m.center.enc(),
        floats(&m.vertices),
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
        obb(m.oriented_bounding_box),
        edge_meta(&m.edges),
        e_meta(&m.encoding)
    ])
}
fn sample(d: &QuantizedMeshTerrainData, r: &Rectangle, points: &Value) -> Value {
    points
        .as_array()
        .unwrap()
        .iter()
        .map(|p| d.interpolate_height(r, r.west + r.width() * n(&p[0]), r.south + r.height() * n(&p[1])).enc())
        .collect::<Vec<_>>()
        .into()
}

#[test]
fn quantized_mesh_terrain_data() {
    run(
        "core",
        "QuantizedMeshTerrainData",
        |_| TIGHT,
        |f, a| match f {
            "lifecycle" => {
                let mut d = QuantizedMeshTerrainData::new(options(&a[0]))?;
                let ts = scheme(a[1].as_bool().unwrap());
                let tile = tile(&a[2]);
                let r = ts.tile_xy_to_rectangle(tile.x as f64, tile.y as f64, tile.level);
                let masks: Vec<_> = (0..9)
                    .map(|j| d.is_child_available(tile.x, tile.y, tile.x * 2 + j % 3, tile.y * 2 + j / 3))
                    .collect();
                assert!(d
                    .upsample_sync(
                        ts.as_ref(),
                        tile,
                        TerrainTile { x: tile.x * 2, y: tile.y * 2, level: tile.level + 1 }
                    )
                    .is_none());
                let initial = json!([raw(&d), sample(&d, &r, &a[3]), masks, Value::Null]);
                let mut request = TerrainMeshRequest::new(ts.as_ref(), tile);
                request.exaggeration = n(&a[4]);
                request.exaggeration_relative_height = n(&a[5]);
                let m = d.create_mesh_sync(request).ok()?;
                let mut children = Vec::new();
                for y in 0..2 {
                    for x in 0..2 {
                        let child_tile = TerrainTile { x: tile.x * 2 + x, y: tile.y * 2 + y, level: tile.level + 1 };
                        let child = d.upsample_sync(ts.as_ref(), tile, child_tile)?;
                        let r = ts.tile_xy_to_rectangle(child_tile.x as f64, child_tile.y as f64, child_tile.level);
                        children.push(json!([raw(&child), sample(&child, &r, &a[3])]));
                    }
                }
                ok(json!([initial, mesh(&m), raw(&d), sample(&d, &r, &a[3]), children]))
            }
            _ => None,
        },
    );
}

#[test]
fn create_vertices_from_quantized_terrain_mesh() {
    run(
        "core",
        "createVerticesFromQuantizedTerrainMesh",
        |_| TIGHT,
        |f, a| match f {
            "largeCreate" => {
                let o = options(&a[0]);
                let result =
                    processor::create_vertices_from_quantized_terrain_mesh(processor::QuantizedTerrainMeshRequest {
                        quantized_vertices: &o.quantized_vertices,
                        indices: &o.indices,
                        encoded_normals: None,
                        edges: &o.edges,
                        skirt_heights: o.skirt_heights,
                        rectangle: rect(&a[1]),
                        center: o.bounding_sphere.center,
                        ellipsoid: Ellipsoid::WGS84,
                        minimum_height: o.minimum_height,
                        maximum_height: o.maximum_height,
                        include_web_mercator_t: true,
                        exaggeration: 1.0,
                        exaggeration_relative_height: 0.0,
                    })?;
                ok(json!([
                    result.vertices.len(),
                    floats(&result.vertices[..24]),
                    floats(&result.vertices[result.vertices.len() - 24..]),
                    typed_array_name(&result.indices),
                    result.indices.to_f64_vec().enc(),
                    e_meta(&result.encoding)
                ]))
            }
            "create" => {
                let o = options(&a[0]);
                let result =
                    processor::create_vertices_from_quantized_terrain_mesh(processor::QuantizedTerrainMeshRequest {
                        quantized_vertices: &o.quantized_vertices,
                        indices: &o.indices,
                        encoded_normals: o.encoded_normals.as_deref(),
                        edges: &o.edges,
                        skirt_heights: o.skirt_heights,
                        rectangle: rect(&a[1]),
                        center: o.bounding_sphere.center,
                        ellipsoid: Ellipsoid::WGS84,
                        minimum_height: o.minimum_height,
                        maximum_height: o.maximum_height,
                        include_web_mercator_t: a[2].as_bool().unwrap(),
                        exaggeration: n(&a[3]),
                        exaggeration_relative_height: 30.0,
                    })?;
                ok(json!([
                    floats(&result.vertices),
                    typed_array_name(&result.indices),
                    result.indices.to_f64_vec().enc(),
                    edge_meta(&result.edges),
                    result.encoding.stride,
                    o.bounding_sphere.center.enc(),
                    o.minimum_height,
                    o.maximum_height,
                    result.occludee_point_in_scaled_space.enc(),
                    e_meta(&result.encoding),
                    result.index_count_without_skirts
                ]))
            }
            _ => None,
        },
    );
}

#[test]
fn upsample_quantized_terrain_mesh() {
    run(
        "core",
        "upsampleQuantizedTerrainMesh",
        |_| TIGHT,
        |f, a| match f {
            "upsample" => {
                let mut d = QuantizedMeshTerrainData::new(options(&a[0]))?;
                let ts = scheme(a[1].as_bool().unwrap());
                let tile = tile(&a[2]);
                let m = d.create_mesh_sync(TerrainMeshRequest::new(ts.as_ref(), tile)).ok()?;
                let east = a[3].as_bool().unwrap();
                let north = a[4].as_bool().unwrap();
                let r = ts.tile_xy_to_rectangle(
                    (tile.x * 2 + u32::from(east)) as f64,
                    (tile.y * 2 + u32::from(!north)) as f64,
                    tile.level + 1,
                );
                let result = upsample::upsample_quantized_terrain_mesh(upsample::QuantizedTerrainUpsampleRequest {
                    mesh: &m,
                    minimum_height: d.minimum_height(),
                    maximum_height: d.maximum_height(),
                    is_east_child: east,
                    is_north_child: north,
                    child_rectangle: r,
                    ellipsoid: Ellipsoid::WGS84,
                })?;
                ok(json!([
                    result.vertices,
                    typed_array_name(&result.indices),
                    result.indices.to_f64_vec().enc(),
                    result.encoded_normals,
                    result.minimum_height,
                    result.maximum_height,
                    q_edges(&result.edges),
                    result.bounding_sphere.enc(),
                    obb(Some(result.oriented_bounding_box)),
                    result.horizon_occlusion_point.enc()
                ]))
            }
            _ => None,
        },
    );
}

#[test]
fn quantized_terrain_contract_preserves_credits_and_releases_source_buffers() {
    use std::future::Future;
    use std::pin::Pin;
    use std::task::{Context, Poll, Waker};
    fn complete<F: Future + Unpin>(mut f: F) -> F::Output {
        match Pin::new(&mut f).poll(&mut Context::from_waker(Waker::noop())) {
            Poll::Ready(value) => value,
            Poll::Pending => panic!("synchronous terrain task was pending"),
        }
    }
    let ts = GeographicTilingScheme::default();
    let tile = TerrainTile { x: 1, y: 1, level: 7 };
    let center = Ellipsoid::WGS84.cartographic_to_cartesian(&ts.tile_xy_to_rectangle(1.0, 1.0, 7).center());
    let mut o = QuantizedMeshTerrainDataOptions::<u32>::new(
        vec![0, 32767, 0, 32767, 0, 0, 32767, 32767, 0, 10000, 20000, 32767],
        TypedArray::Uint16(vec![0, 1, 2, 1, 3, 2]),
        [-10.0, 900.0],
        BoundingSphere::new(center, 100000.0),
        Cartesian3::new(1.0, 2.0, 3.0),
    );
    o.credits = Some(vec![42]);
    o.water_mask = Some(vec![255]);
    let mut d = QuantizedMeshTerrainData::new(o).unwrap();
    assert_eq!(TerrainData::credits(&d), Some([42].as_slice()));
    assert_eq!(TerrainData::water_mask(&d), Some([255].as_slice()));
    assert!(TerrainData::upsample(&d, &ts, tile, TerrainTile { x: 2, y: 2, level: 8 }).is_none());
    let mesh = complete(TerrainData::create_mesh(&mut d, TerrainMeshRequest::new(&ts, tile)).unwrap()).unwrap();
    assert!(
        d.quantized_vertices().is_none()
            && d.indices().is_none()
            && d.encoded_normals().is_none()
            && d.edges().is_none()
    );
    d.mesh_mut().unwrap().encoding.exaggeration = 3.0;
    assert_eq!(mesh.encoding.exaggeration, 1.0);
    assert!(d.create_mesh_sync(TerrainMeshRequest::new(&ts, tile)).is_err());
    let child = complete(TerrainData::upsample(&d, &ts, tile, TerrainTile { x: 2, y: 2, level: 8 }).unwrap()).unwrap();
    assert_eq!(child.credits(), Some([42].as_slice()));
    assert!(child.water_mask().is_none());
    assert!(TerrainData::was_created_by_upsampling(&child));
    assert_eq!(child.child_tile_mask(), 0);
    assert!(TerrainData::is_child_available(&d, 1, 1, 2, 2));
    let r = ts.tile_xy_to_rectangle(1.0, 1.0, 7);
    assert!(TerrainData::interpolate_height(&d, &r, r.west, r.south).is_some());
}

#[test]
fn invalid_quantized_topology_is_rejected() {
    let options = QuantizedMeshTerrainDataOptions::<String>::new(
        vec![0; 12],
        TypedArray::Uint16(vec![0, 1, 9]),
        [0.0, 1.0],
        BoundingSphere::default(),
        Cartesian3::ZERO,
    );
    assert!(QuantizedMeshTerrainData::new(options.clone()).is_none());
    let mut normals = options;
    normals.indices = TypedArray::Uint16(vec![0, 1, 2]);
    normals.encoded_normals = Some(vec![0; 3]);
    assert!(QuantizedMeshTerrainData::new(normals).is_none());
}
