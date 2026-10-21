//! Terrain and packet workers against their real CesiumJS message handlers.
mod common;
use cesium_rs::core::cesium_3d_tiles_terrain_geometry_processor::{
    TilesTerrainMeshOptions, TilesTerrainUpsampleOptions,
};
use cesium_rs::core::terrain_provider::TerrainEdgeIndices;
use cesium_rs::{core::*, workers::*};
use common::*;
use serde_json::{json, Value};

// Float32 vertex data can differ by one unit in the last place.
const TERRAIN: Tol = Tol { rel: 2e-7, abs: 2e-7, vector: false };

fn encoding(e: &TerrainEncoding) -> Value {
    json!({
        "quantization": e.quantization as u8,
        "minimumHeight": e.minimum_height.enc(),
        "maximumHeight": e.maximum_height.enc(),
        "center": e.center.enc(),
        "toScaledENU": e.to_scaled_enu.enc(),
        "fromScaledENU": e.from_scaled_enu.enc(),
        "matrix": e.matrix.enc(),
        "hasVertexNormals": e.has_vertex_normals,
        "hasWebMercatorT": e.has_web_mercator_t,
        "hasGeodeticSurfaceNormals": e.has_geodetic_surface_normals,
        "exaggeration": e.exaggeration,
        "exaggerationRelativeHeight": e.exaggeration_relative_height,
        "stride": e.stride,
    })
}
fn edges(e: &TerrainEdgeIndices) -> Value {
    json!([
        e.west_indices_south_to_north,
        e.south_indices_east_to_west,
        e.east_indices_north_to_south,
        e.north_indices_west_to_east
    ])
}
fn obb(b: Option<OrientedBoundingBox>) -> Value {
    b.map_or(Value::Null, |b| json!([b.center.enc(), b.half_axes.enc()]))
}
fn f32s(v: &[f32]) -> Value {
    v.iter().map(|&x| num(x as f64)).collect::<Vec<_>>().into()
}
fn proxy(p: &TerrainMeshProxy) -> serde_json::Map<String, Value> {
    let Value::Object(map) = json!({
        "vertices": f32s(&p.vertices),
        "indexBytes": p.indices.len() * p.index_bytes_per_element(),
        "indices": p.indices.to_f64_vec().enc(),
        "vertexCountWithoutSkirts": p.vertex_count_without_skirts,
        "indexCountWithoutSkirts": p.index_count_without_skirts,
        "encoding": encoding(&p.encoding),
        "edges": edges(&p.edges),
        "minimumHeight": null,
        "maximumHeight": null,
        "boundingSphere": null,
        "orientedBoundingBox": null,
        "horizonOcclusionPoint": null,
        "transferCount": 6,
    }) else {
        unreachable!()
    };
    map
}
fn gltf(a: &[Value]) -> TerrainGltf {
    TerrainGltf {
        json: a[0].clone(),
        buffers: a[1].as_array().unwrap().iter().map(|v| nums(v).into_iter().map(|n| n as u8).collect()).collect(),
    }
}
fn mesh_options<'a>(g: &'a TerrainGltf, o: &Value) -> TilesTerrainMeshOptions<'a> {
    let r = rect(&o["rectangle"]);
    let (min, max) = (n(&o["minimumHeight"]), n(&o["maximumHeight"]));
    TilesTerrainMeshOptions {
        gltf: g,
        ellipsoid: Ellipsoid::WGS84,
        rectangle: r,
        has_vertex_normals: o["normals"].as_bool().unwrap(),
        has_web_mercator_t: o["webMercatorT"].as_bool().unwrap(),
        minimum_height: min,
        maximum_height: max,
        bounding_sphere: BoundingSphere::new(
            Ellipsoid::WGS84.cartographic_to_cartesian(&Cartographic::new(r.west, r.south, 0.)),
            9000.,
        ),
        oriented_bounding_box: OrientedBoundingBox::from_rectangle(&r, min, max, &Ellipsoid::WGS84).unwrap(),
        horizon_occlusion_point: Cartesian3::new(0.5, 0.25, 0.1),
        skirt_height: n(&o["skirtHeight"]),
        exaggeration: n(&o["exaggeration"]),
        exaggeration_relative_height: n(&o["exaggerationRelativeHeight"]),
    }
}

#[test]
fn create_vertices_from_cesium_3d_tiles_terrain_reference() {
    run(
        "workers",
        "createVerticesFromCesium3DTilesTerrain",
        |_| TERRAIN,
        |_, a| {
            let g = gltf(a);
            Some(match create_vertices_from_cesium_3d_tiles_terrain(mesh_options(&g, &a[2])) {
                Ok(p) => Out::Ok(Value::Object(proxy(&p))),
                Err(e) => Out::Err(e.to_string()),
            })
        },
    );
}

#[test]
fn upsample_vertices_from_cesium_3d_tiles_terrain_reference() {
    run(
        "workers",
        "upsampleVerticesFromCesium3DTilesTerrain",
        |_| TERRAIN,
        |_, a| {
            // The parent is the creation worker's own output, as in Cesium3DTilesTerrainData.
            let g = gltf(a);
            let parent = create_vertices_from_cesium_3d_tiles_terrain(mesh_options(&g, &a[2])).unwrap();
            let o = &a[2];
            let child = upsample_vertices_from_cesium_3d_tiles_terrain(TilesTerrainUpsampleOptions {
                is_east_child: a[3].as_bool().unwrap(),
                is_north_child: a[4].as_bool().unwrap(),
                parent_vertices: &parent.vertices,
                parent_indices: &parent.indices,
                parent_vertex_count_without_skirts: parent.vertex_count_without_skirts,
                parent_index_count_without_skirts: parent.index_count_without_skirts,
                parent_minimum_height: n(&o["minimumHeight"]),
                parent_maximum_height: n(&o["maximumHeight"]),
                parent_encoding: &parent.encoding,
                rectangle: rect(&a[5]),
                ellipsoid: Ellipsoid::WGS84,
                skirt_height: n(&o["skirtHeight"]) * 0.5,
            });
            Some(match child {
                Ok(c) => {
                    let mut map = proxy(&c.mesh);
                    map.insert("minimumHeight".into(), c.minimum_height.enc());
                    map.insert("maximumHeight".into(), c.maximum_height.enc());
                    map.insert("boundingSphere".into(), c.bounding_sphere.enc());
                    map.insert("orientedBoundingBox".into(), obb(c.oriented_bounding_box));
                    map.insert("horizonOcclusionPoint".into(), c.horizon_occlusion_point.enc());
                    Out::Ok(Value::Object(map))
                }
                Err(e) => Out::Err(e.to_string()),
            })
        },
    );
}

#[test]
fn create_vertices_from_google_earth_enterprise_buffer_reference() {
    let pi = std::f64::consts::PI;
    let scheme = GeographicTilingScheme::new(Ellipsoid::WGS84, Rectangle::new(-pi, -pi, pi, pi), 2, 2);
    run(
        "workers",
        "createVerticesFromGoogleEarthEnterpriseBuffer",
        |_| TERRAIN,
        |_, a| {
            let bytes = nums(&a[0]).into_iter().map(|v| v as u8).collect::<Vec<_>>();
            let level = u(&a[1]);
            let o = &a[2];
            let r = scheme.tile_xy_to_rectangle(3., 2., level);
            let result = create_vertices_from_google_earth_enterprise_buffer(GoogleEarthEnterpriseBufferRequest {
                buffer: &bytes,
                relative_to_center: Ellipsoid::WGS84.cartographic_to_cartesian(&r.center()),
                ellipsoid: Ellipsoid::WGS84,
                rectangle: r,
                exaggeration: n(&o["exaggeration"]),
                exaggeration_relative_height: n(&o["exaggerationRelativeHeight"]),
                skirt_height: (40075.16 / (1u64 << level) as f64 * 8.).min(1000.),
                include_web_mercator_t: o["webMercatorT"].as_bool().unwrap(),
                negative_altitude_exponent_bias: 32.,
                negative_elevation_threshold: n(&o["threshold"]),
            });
            Some(match result {
                Ok(m) => Out::Ok(json!({
                    "vertices": f32s(&m.vertices),
                    "indices": m.indices.iter().map(|&i| i as u32).collect::<Vec<_>>(),
                    "numberOfAttributes": m.number_of_attributes,
                    "minimumHeight": m.minimum_height.enc(),
                    "maximumHeight": m.maximum_height.enc(),
                    "boundingSphere3D": m.bounding_sphere_3d.enc(),
                    "orientedBoundingBox": obb(m.oriented_bounding_box),
                    "occludeePointInScaledSpace": m.occludee_point_in_scaled_space.enc(),
                    "encoding": encoding(&m.encoding),
                    "vertexCountWithoutSkirts": m.vertex_count_without_skirts,
                    "indexCountWithoutSkirts": m.index_count_without_skirts,
                    "edges": edges(&m.edges),
                    "transferCount": 2,
                })),
                Err(e) => Out::Err(e.to_string()),
            })
        },
    );
}

fn tile(i: &Option<GoogleEarthEnterpriseTileInformation>) -> Value {
    i.as_ref().map_or(Value::Null, |i| {
        json!([
            i.bits,
            i.cnode_version,
            i.imagery_version,
            i.terrain_version,
            i.imagery_provider,
            i.terrain_provider,
            i.ancestor_has_terrain,
            i.terrain_state
        ])
    })
}

#[test]
fn decode_google_earth_enterprise_packet_reference() {
    run(
        "workers",
        "decodeGoogleEarthEnterprisePacket",
        |_| EXACT,
        |f, a| {
            let bytes = |v: &Value| nums(v).into_iter().map(|v| v as u8).collect::<Vec<_>>();
            let packet_type = GoogleEarthEnterprisePacketType::from_name(f);
            Some(match decode_google_earth_enterprise_packet(packet_type, &bytes(&a[0]), &bytes(&a[1]), s(&a[2])) {
                Ok(Some(GoogleEarthEnterprisePacket::Metadata(tiles))) => Out::Ok(json!({
                    "tiles": tiles.iter().map(|(k, v)| (k.clone(), tile(v))).collect::<serde_json::Map<_, _>>(),
                    "transferCount": 0,
                })),
                Ok(Some(GoogleEarthEnterprisePacket::Terrain(meshes))) => {
                    Out::Ok(json!({"meshes": meshes, "transferCount": meshes.len()}))
                }
                Ok(Some(GoogleEarthEnterprisePacket::DbRoot(buffer))) => {
                    Out::Ok(json!({"buffer": buffer, "transferCount": 1}))
                }
                Ok(None) => Out::Ok(Value::Null),
                Err(e) => Out::Err(e.to_string()),
            })
        },
    );
}

#[test]
fn unknown_packet_type_inflates_without_a_result() {
    let mut packet = vec![0xad, 0xde, 0x68, 0x74, 3, 0, 0, 0];
    let mut z = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    std::io::Write::write_all(&mut z, &[1, 2, 3]).unwrap();
    packet.extend(z.finish().unwrap());
    assert_eq!(decode_google_earth_enterprise_packet(None, &packet, &[0; 4], ""), Ok(None));
    assert!(decode_google_earth_enterprise_packet(None, &packet[..9], &[0; 4], "").is_err());
}
