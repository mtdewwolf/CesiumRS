mod common;
use cesium_rs::core::google_earth_enterprise_terrain_provider::GeeTerrainOptions;
use cesium_rs::core::*;
use common::*;
use serde_json::{json, Value};
use std::{
    future::Future,
    pin::pin,
    rc::Rc,
    task::{Context, Poll, Waker},
};
fn complete<T>(f: impl Future<Output = T>) -> T {
    let mut f = pin!(f);
    for _ in 0..100 {
        if let Poll::Ready(v) = f.as_mut().poll(&mut Context::from_waker(Waker::noop())) {
            return v;
        }
    }
    panic!("fixture suspended")
}
struct Empty;
impl ResourceTransport for Empty {
    fn execute(&self, _: HttpRequest) -> RequestFuture<HttpResponse> {
        Box::pin(async { Err(RequestErrorEvent::new(Some(404), None, None)) })
    }
}
fn metadata() -> Rc<GoogleEarthEnterpriseMetadata> {
    GoogleEarthEnterpriseMetadata::new(Resource::new("https://gee.test/").unwrap(), ResourceClient::new(Rc::new(Empty)))
}
fn bytes(v: &Value) -> Vec<u8> {
    nums(v).into_iter().map(|v| v as u8).collect()
}
fn info(i: &GoogleEarthEnterpriseTileInformation) -> Value {
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
}
#[test]
fn gee_metadata() {
    run(
        "core",
        "GoogleEarthEnterpriseMetadata",
        |_| TIGHT,
        |f, a| {
            let m = metadata();
            match f {
                "quadkey" => {
                    let key = GoogleEarthEnterpriseMetadata::tile_xy_to_quad_key(u(&a[0]), u(&a[1]), u(&a[2]));
                    let (x, y, l) = GoogleEarthEnterpriseMetadata::quad_key_to_tile_xy(&key);
                    ok(json!([key,{"x":x as i32,"y":y as i32,"level":l}]))
                }
                "packet" => {
                    m.apply_metadata_packet(&bytes(&a[0]), "").unwrap();
                    let tiles = m
                        .tile_info
                        .borrow()
                        .iter()
                        .map(|(k, i)| (k.clone(), i.as_ref().map(info).unwrap_or(Value::Null)))
                        .collect::<serde_json::Map<_, _>>();
                    ok(json!([tiles, (["0", "00", "3", "33", "33333"].map(|k| m.is_valid(k)))]))
                }
                "dbroot" => {
                    m.apply_decoded_db_root(&bytes(&a[0])).unwrap();
                    let providers = m
                        .providers
                        .borrow()
                        .iter()
                        .map(|(id, c)| (id.to_string(), json!(c.html())))
                        .collect::<serde_json::Map<_, _>>();
                    ok(json!([
                        m.imagery_present.get(),
                        m.terrain_present.get(),
                        m.proto_imagery.get(),
                        m.quad_packet_version.get(),
                        m.negative_altitude_exponent_bias.get(),
                        m.negative_altitude_threshold.get(),
                        providers
                    ]))
                }
                _ => None,
            }
        },
    );
}
fn mesh(m: &TerrainMesh) -> Value {
    json!([
        m.center.enc(),
        m.vertices.iter().map(|v| num(*v as f64)).collect::<Vec<_>>(),
        m.indices.to_f64_vec().enc(),
        m.index_count_without_skirts,
        m.vertex_count_without_skirts,
        m.minimum_height,
        m.maximum_height,
        m.bounding_sphere_3d.enc(),
        m.occludee_point_in_scaled_space.enc(),
        m.stride,
        m.oriented_bounding_box.map(|b| json!([b.center.enc(), b.half_axes.enc()])),
        [
            &m.edges.west_indices_south_to_north,
            &m.edges.south_indices_east_to_west,
            &m.edges.east_indices_north_to_south,
            &m.edges.north_indices_west_to_east
        ],
        m.encoding.quantization as u8,
        m.encoding.exaggeration,
        m.encoding.exaggeration_relative_height
    ])
}
fn sample(d: &GoogleEarthEnterpriseTerrainData, r: &Rectangle) -> Value {
    [0., 0.25, 0.5, 0.75, 1.].map(|u| d.interpolate_height(r, r.west + r.width() * u, r.south + r.height() * u)).enc()
}
#[test]
fn gee_data() {
    run(
        "core",
        "GoogleEarthEnterpriseTerrainData",
        |_| Tol { rel: 1e-6, abs: 1e-5, vector: false },
        |_, a| {
            let s = GeographicTilingScheme::new(
                Ellipsoid::WGS84,
                Rectangle::new(
                    -std::f64::consts::PI,
                    -std::f64::consts::PI,
                    std::f64::consts::PI,
                    std::f64::consts::PI,
                ),
                2,
                2,
            );
            let l = u(&a[1]);
            let tile = TerrainTile { x: 3, y: 2, level: l };
            let r = s.tile_xy_to_rectangle(3., 2., l);
            let mut d = GoogleEarthEnterpriseTerrainData::new(
                bytes(&a[0]),
                32.,
                1e-12,
                5,
                Some(vec![Credit::new("GEE", false)]),
            );
            let initial = json!([d.child_tile_mask(), sample(&d, &r), d.was_created_by_upsampling()]);
            let mut req = TerrainMeshRequest::new(&s, tile);
            req.exaggeration = n(&a[2]);
            req.exaggeration_relative_height = 20.;
            let m = d.create_mesh_sync(req).unwrap();
            let mut children = vec![];
            for y in 0..2 {
                for x in 0..2 {
                    let c =
                        d.upsample_quantized_sync(&s, tile, TerrainTile { x: 6 + x, y: 4 + y, level: l + 1 }).unwrap();
                    children.push(json!([
                        c.quantized_vertices(),
                        c.indices().unwrap().to_f64_vec().enc(),
                        c.minimum_height(),
                        c.maximum_height(),
                        c.child_tile_mask(),
                        c.was_created_by_upsampling(),
                        c.skirt_heights()[0]
                    ]));
                }
            }
            ok(json!([initial, mesh(&m), sample(&d, &r), children]))
        },
    );
}
#[test]
fn gee_provider() {
    run(
        "core",
        "GoogleEarthEnterpriseTerrainProvider",
        |_| TIGHT,
        |_, a| {
            let m = metadata();
            let mut i=GoogleEarthEnterpriseTileInformation::new(cesium_rs::core::google_earth_enterprise_tile_information::GoogleEarthEnterpriseTileInformationOptions{bits:u(&a[0])as u8,cnode_version:5,imagery_version:6,terrain_version:7,imagery_provider:2,terrain_provider:3});
            i.ancestor_has_terrain = a[1].as_bool().unwrap();
            i.terrain_state = a[2].as_i64().map(|v| v as i32);
            m.tile_info.borrow_mut().insert("3".into(), Some(i));
            let p = GoogleEarthEnterpriseTerrainProvider::from_metadata(
                m.clone(),
                GeeTerrainOptions { credit: Some(Credit::new("GEE", false)), ..Default::default() },
                ResourceClient::new(Rc::new(Empty)),
            )
            .unwrap();
            ok(json!([
                m.resource.url(),
                p.credit().unwrap().html(),
                p.tiling_scheme().rectangle().enc(),
                ([0, 1, 15, 31, 32].map(|l| num(p.get_level_maximum_geometric_error(l)))),
                p.get_tile_data_available(0, 0, 0)
            ]))
        },
    );
}
#[test]
fn encrypted_terrain_packet_populates_parent_and_child_cache() {
    use cesium_rs::core::google_earth_enterprise_tile_information::GoogleEarthEnterpriseTileInformationOptions;
    use std::{cell::RefCell, io::Write};
    struct Transport {
        bytes: Vec<u8>,
        calls: RefCell<Vec<String>>,
    }
    impl ResourceTransport for Transport {
        fn execute(&self, r: HttpRequest) -> RequestFuture<HttpResponse> {
            self.calls.borrow_mut().push(r.url);
            let body = self.bytes.clone();
            Box::pin(async { Ok(HttpResponse { status: 200, headers: vec![], body }) })
        }
    }
    let fixture = common::load("core", "GoogleEarthEnterpriseTerrainData");
    let raw = bytes(&fixture[0].a[0]);
    let mut tiles = vec![];
    for _ in 0..5 {
        tiles.extend(&raw);
    }
    let mut zipped = flate2::write::ZlibEncoder::new(Vec::new(), flate2::Compression::default());
    zipped.write_all(&tiles).unwrap();
    let zipped = zipped.finish().unwrap();
    let mut packet = vec![];
    packet.extend(0x7468deadu32.to_le_bytes());
    packet.extend((tiles.len() as u32).to_le_bytes());
    packet.extend(zipped);
    let transport = Rc::new(Transport { bytes: packet, calls: Default::default() });
    let client = ResourceClient::new(transport.clone());
    let m = GoogleEarthEnterpriseMetadata::new(Resource::new("https://gee.test/").unwrap(), client.clone());
    let key = GoogleEarthEnterpriseMetadata::tile_xy_to_quad_key(3, 2, 3);
    let mut parent = GoogleEarthEnterpriseTileInformation::new(GoogleEarthEnterpriseTileInformationOptions {
        bits: 143,
        terrain_version: 7,
        terrain_provider: 3,
        ..Default::default()
    });
    parent.ancestor_has_terrain = true;
    m.tile_info.borrow_mut().insert(key.clone(), Some(parent));
    m.providers.borrow_mut().insert(3, Credit::new("GEE", false));
    for i in 0..4 {
        let mut child = GoogleEarthEnterpriseTileInformation::new(Default::default());
        child.ancestor_has_terrain = true;
        m.tile_info.borrow_mut().insert(format!("{key}{i}"), Some(child));
    }
    let p =
        GoogleEarthEnterpriseTerrainProvider::from_metadata(m.clone(), GeeTerrainOptions::default(), client).unwrap();
    let parent = complete(p.request_tile_geometry(3, 2, 3, None).unwrap()).unwrap();
    assert_eq!(parent.credits().unwrap()[0].html(), "GEE");
    for i in 0..4 {
        let (x, y, l) = GoogleEarthEnterpriseMetadata::quad_key_to_tile_xy(&format!("{key}{i}"));
        let child = complete(p.request_tile_geometry(x, y, l as u32, None).unwrap()).unwrap();
        let NetworkTerrainData::GoogleEarthEnterprise(data) = child else { panic!("Expected GEE terrain") };
        assert_eq!(data.buffer(), Some(raw.as_slice()));
        assert_eq!(data.credits().unwrap()[0].html(), "GEE");
        assert_eq!(data.child_tile_mask(), 0);
    }
    assert_eq!(*transport.calls.borrow(), vec![format!("https://gee.test/flatfile?f1c-0{key}-t.7")]);
    assert_eq!(m.get_tile_information_from_quad_key(&key).flatten().unwrap().terrain_state, Some(2));
}
