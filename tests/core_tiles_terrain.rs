mod common;
use cesium_rs::core::cesium_3d_tiles_terrain_geometry_processor::TilesTerrainMeshOptions;
use cesium_rs::core::*;
use common::*;
use serde_json::{json, Value};
fn gltf(a: &[Value]) -> TerrainGltf {
    TerrainGltf {
        json: a[0].clone(),
        buffers: a[1].as_array().unwrap().iter().map(|v| nums(v).into_iter().map(|n| n as u8).collect()).collect(),
    }
}
fn options<'a>(
    g: &'a TerrainGltf,
    s: &dyn TilingScheme,
    l: u32,
    normals: bool,
    exag: f64,
) -> TilesTerrainMeshOptions<'a> {
    let r = s.tile_xy_to_rectangle(1., 1., l);
    let center = s.ellipsoid().cartographic_to_cartesian(&r.center());
    TilesTerrainMeshOptions {
        gltf: g,
        ellipsoid: *s.ellipsoid(),
        rectangle: r,
        has_vertex_normals: normals,
        has_web_mercator_t: true,
        minimum_height: -10.,
        maximum_height: 200.,
        bounding_sphere: BoundingSphere::new(center, 5000.),
        oriented_bounding_box: OrientedBoundingBox::from_rectangle(&r, -10., 200., s.ellipsoid()).unwrap(),
        horizon_occlusion_point: Cartesian3::new(1., 0., 0.),
        skirt_height: 30.,
        exaggeration: exag,
        exaggeration_relative_height: 20.,
    }
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
#[test]
fn processor() {
    run(
        "core",
        "Cesium3DTilesTerrainGeometryProcessor",
        |_| Tol { rel: 1e-6, abs: 1e-4, vector: false },
        |_, a| {
            let g = gltf(a);
            let s = GeographicTilingScheme::default();
            let l = u(&a[2]);
            let opts = options(&g, &s, l, a[3].as_bool().unwrap(), n(&a[4]));
            let m = match Cesium3DTilesTerrainGeometryProcessor::create_mesh(opts) {
                Ok(m) => m,
                Err(e) => return Some(Out::Err(e.to_string())),
            };
            let mut children = vec![];
            for y in 0..2 {
                for x in 0..2 {
                    let child = Cesium3DTilesTerrainGeometryProcessor::upsample_mesh(
                        &m,
                        Ellipsoid::WGS84,
                        s.tile_xy_to_rectangle((2 + x) as f64, (2 + y) as f64, l + 1),
                        x == 1,
                        y == 0,
                        15.,
                    )
                    .unwrap();
                    children.push(mesh(&child));
                }
            }
            ok(json!([mesh(&m), children]))
        },
    );
}
fn sample(d: &Cesium3DTilesTerrainData, r: &Rectangle) -> Value {
    [0., 0.5, 1.].map(|u| d.interpolate_height(r, r.west + r.width() * u, r.south + r.height() * u)).enc()
}
#[test]
fn data() {
    run(
        "core",
        "Cesium3DTilesTerrainData",
        |_| Tol { rel: 1e-6, abs: 1e-4, vector: false },
        |_, a| {
            let g = gltf(a);
            let s = GeographicTilingScheme::default();
            let l = u(&a[2]);
            let o = options(&g, &s, l, a[3].as_bool().unwrap(), n(&a[4]));
            let mut d = Cesium3DTilesTerrainData::new(Cesium3DTilesTerrainDataOptions {
                gltf: g.clone(),
                minimum_height: o.minimum_height,
                maximum_height: o.maximum_height,
                bounding_sphere: o.bounding_sphere,
                oriented_bounding_box: o.oriented_bounding_box,
                horizon_occlusion_point: o.horizon_occlusion_point,
                skirt_height: o.skirt_height,
                request_vertex_normals: o.has_vertex_normals,
                child_tile_mask: 5,
                credits: Some(vec![Credit::new("tiles", false)]),
                water_mask: Some(vec![0, 255]),
            });
            let initial = json!([
                sample(&d, &o.rectangle),
                d.was_created_by_upsampling(),
                ((0..4).map(|i| d.is_child_available(1, 1, 2 + i % 2, 2 + i / 2)).collect::<Vec<_>>())
            ]);
            let mut req = TerrainMeshRequest::new(&s, TerrainTile { x: 1, y: 1, level: l });
            req.exaggeration = o.exaggeration;
            req.exaggeration_relative_height = 20.;
            let m = d.create_mesh_sync(req).unwrap();
            let child = d
                .upsample_sync(&s, TerrainTile { x: 1, y: 1, level: l }, TerrainTile { x: 2, y: 2, level: l + 1 })
                .unwrap()
                .unwrap();
            ok(json!([
                initial,
                mesh(&m),
                sample(&d, &o.rectangle),
                child.was_created_by_upsampling(),
                child.is_child_available(2, 2, 4, 4),
                mesh(child.mesh().unwrap())
            ]))
        },
    );
}
#[test]
fn provider() {
    use cesium_rs::core::cesium_3d_tiles_terrain_provider::TilesTerrainOptions;
    use std::{
        cell::RefCell,
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
    struct Transport {
        json: Value,
        sub: Vec<u8>,
        glb: Vec<u8>,
        calls: RefCell<Vec<String>>,
    }
    impl ResourceTransport for Transport {
        fn execute(&self, r: HttpRequest) -> RequestFuture<HttpResponse> {
            let body = if r.url.ends_with(".json") {
                serde_json::to_vec(&self.json).unwrap()
            } else {
                self.calls.borrow_mut().push(r.url.clone());
                if r.url.ends_with(".subtree") {
                    self.sub.clone()
                } else {
                    self.glb.clone()
                }
            };
            Box::pin(async { Ok(HttpResponse { status: 200, headers: vec![], body }) })
        }
    }
    run(
        "core",
        "Cesium3DTilesTerrainProvider",
        |_| TIGHT,
        |_, a| {
            let bytes = |v: &Value| nums(v).iter().map(|&v| v as u8).collect();
            let t = Rc::new(Transport {
                json: a[0].clone(),
                sub: bytes(&a[1]),
                glb: bytes(&a[2]),
                calls: Default::default(),
            });
            let p = complete(Cesium3DTilesTerrainProvider::from_url(
                Resource::new("https://tiles.test/tileset.json")?,
                TilesTerrainOptions {
                    credit: Some(Credit::new("3D Tiles", false)),
                    request_vertex_normals: a[3].as_bool().unwrap(),
                    request_water_mask: a[4].as_bool().unwrap(),
                    ..Default::default()
                },
                ResourceClient::new(t.clone()),
            ))
            .unwrap();
            let available =
                || [(0, 0, 0), (1, 0, 0), (0, 0, 1), (0, 0, 2)].map(|(x, y, l)| p.get_tile_data_available(x, y, l));
            let initial = available();
            let d = complete(p.request_tile_geometry(0, 0, 1, None)?).unwrap();
            let NetworkTerrainData::Cesium3DTiles(d) = d else { panic!("wrong terrain type") };
            for longitude in [-2., 1.] {
                let position = Cartographic::new(longitude, 1., 0.);
                assert_eq!(
                    p.availability().unwrap().compute_maximum_level_at_position(&position),
                    p.compute_maximum_level_at_position(&position) as i64
                );
            }
            let calls = t.calls.borrow().clone();
            ok(json!([
                p.credit().unwrap().html(),
                p.has_vertex_normals(),
                p.has_water_mask(),
                ([0, 1, 8, 31, 32].map(|l| num(p.get_level_maximum_geometric_error(l)))),
                initial,
                [
                    d.minimum_height,
                    d.maximum_height,
                    d.bounding_sphere.enc(),
                    d.horizon_occlusion_point.enc(),
                    d.child_tile_mask,
                    d.skirt_height,
                    d.water_mask
                ],
                available(),
                ([-2., 1.].map(|lon| p.compute_maximum_level_at_position(&Cartographic::new(lon, 1., 0.)))),
                calls
            ]))
        },
    );
}
