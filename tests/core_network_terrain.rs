mod common;
use cesium_rs::core::arcgis_tiled_elevation_terrain_provider::ArcGisTerrainOptions;
use cesium_rs::core::cesium_terrain_provider::CesiumTerrainOptions;
use cesium_rs::core::heightmap_terrain_data::HeightmapTerrainStructure;
use cesium_rs::core::image_helpers::ImageFuture;
use cesium_rs::core::vr_the_world_terrain_provider::VRTheWorldOptions;
use cesium_rs::core::*;
use common::*;
use serde_json::{json, Value};
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
    metadata: Value,
    bytes: Vec<u8>,
    requests: RefCell<Vec<HttpRequest>>,
}
impl ResourceTransport for Transport {
    fn execute(&self, r: HttpRequest) -> RequestFuture<HttpResponse> {
        self.requests.borrow_mut().push(r.clone());
        let body = if r.url.contains("/endpoint") {
            serde_json::to_vec(&json!({"url":"https://terrain.test/cesium/","accessToken":"test","attributions":[]}))
                .unwrap()
        } else if r.url.contains("tilemap") {
            let dim = r.url.split('?').next().unwrap().split('/').next_back().unwrap().parse::<usize>().unwrap();
            serde_json::to_vec(&json!({"data":vec![0;dim*dim]})).unwrap()
        } else if r.url.contains(".terrain") {
            self.bytes.clone()
        } else {
            serde_json::to_vec(&self.metadata).unwrap()
        };
        Box::pin(async { Ok(HttpResponse { status: 200, body, headers: vec![] }) })
    }
}
fn transport(m: Value, bytes: Vec<u8>) -> Rc<Transport> {
    Rc::new(Transport { metadata: m, bytes, requests: Default::default() })
}
fn provider<P: TerrainProvider<Credit = Credit>>(p: &P) -> Value {
    let s = p.tiling_scheme();
    json!([
        p.credit().map(|c| json!([c.html(), c.show_on_screen])),
        s.ellipsoid().radii.enc(),
        s.rectangle().enc(),
        s.get_number_of_x_tiles_at_level(0),
        s.get_number_of_y_tiles_at_level(0),
        ([0, 1, 5, 31, 32].map(|l| num(p.get_level_maximum_geometric_error(l)))),
        p.has_water_mask(),
        p.has_vertex_normals(),
        p.availability().is_some()
    ])
}
fn structure(s: HeightmapTerrainStructure) -> Value {
    let b = s.samples;
    let mut v = json!({"heightScale":b.height_scale,"heightOffset":b.height_offset,"elementsPerHeight":b.elements_per_height,"stride":b.stride,"elementMultiplier":b.element_multiplier,"isBigEndian":b.is_big_endian});
    if let Some(x) = s.lowest_encoded_height {
        v["lowestEncodedHeight"] = json!(x);
    }
    if let Some(x) = s.highest_encoded_height {
        v["highestEncodedHeight"] = json!(x);
    }
    v
}
struct Images;
impl ImageHost for Images {
    type Image = RgbaImage;
    fn dimensions(&self, i: &RgbaImage) -> (u32, u32) {
        (i.width, i.height)
    }
    fn image_from_rgba(&self, i: RgbaImage) -> Result<RgbaImage, RuntimeError> {
        Ok(i)
    }
    fn image_pixels(&self, i: &RgbaImage, _: u32, _: u32) -> Result<Vec<u8>, RuntimeError> {
        Ok(i.pixels.clone())
    }
    fn resize_image(&self, i: &RgbaImage, _: u32, _: u32) -> Result<RgbaImage, RuntimeError> {
        Ok(i.clone())
    }
    fn decode_image<'a>(
        &'a self,
        _: &'a [u8],
        _: &'a str,
        _: ImageDecodeOptions,
    ) -> Option<ImageFuture<'a, RgbaImage>> {
        None
    }
}
#[test]
fn arcgis() {
    run(
        "core",
        "ArcGISTiledElevationTerrainProvider",
        |_| TIGHT,
        |_, a| {
            let r = Resource::new("https://terrain.test/arc/?token=token")?;
            let t = transport(a[0].clone(), vec![]);
            let p = match ArcGISTiledElevationTerrainProvider::from_metadata(
                r,
                &a[0],
                ArcGisTerrainOptions::default(),
                ResourceClient::new(t),
            ) {
                Ok(v) => v,
                Err(e) => return Some(Out::Err(e.to_string())),
            };
            ok(json!([
                provider(&p),
                p.width,
                p.height,
                p.encoding as u8,
                p.lod_count,
                json!({"elementMultiplier":p.structure.samples.element_multiplier,"lowestEncodedHeight":p.structure.lowest_encoded_height,"highestEncodedHeight":p.structure.highest_encoded_height}),
                ([0, 1, 4, 5].map(|l| p.get_tile_data_available(0, 0, l))),
                p.resource.url()
            ]))
        },
    );
}
#[test]
fn vr() {
    run(
        "core",
        "VRTheWorldTerrainProvider",
        |_| TIGHT,
        |_, a| {
            let t = transport(Value::Null, vec![]);
            let p = match VRTheWorldTerrainProvider::from_xml(
                Resource::new("https://terrain.test/vr/")?,
                s(&a[0]),
                VRTheWorldOptions { credit: Some(Credit::new("VR", false)), ..Default::default() },
                ResourceClient::new(t),
                Rc::new(Images),
            ) {
                Ok(p) => p,
                Err(e) => return Some(Out::Err(e.to_string())),
            };
            ok(json!([
                provider(&p),
                p.width,
                p.height,
                structure(p.structure()),
                p.rectangles.iter().map(|(r, l)| json!([r.enc(), l])).collect::<Vec<_>>(),
                ([(0, 0), (1, 0), (0, 1), (1, 1)].map(|(x, y)| p.tile_resource(x, y, u(&a[1]), None).unwrap().url()))
            ]))
        },
    );
}
fn quantized() -> Vec<u8> {
    let mut b = vec![];
    for v in [1., 2., 3.] {
        b.extend(f64::to_le_bytes(v));
    }
    for v in [-10f32, 100.] {
        b.extend(v.to_le_bytes());
    }
    for v in [6378000., 0., 0., 100., 1., 0., 0.] {
        b.extend(f64::to_le_bytes(v));
    }
    b.extend(4u32.to_le_bytes());
    for a in [[0i32, 32767, 0, 32767], [0, 0, 32767, 32767], [0, 10000, 20000, 32767]] {
        let mut last = 0;
        for n in a {
            let delta = n - last;
            b.extend((((delta << 1) ^ (delta >> 31)) as u16).to_le_bytes());
            last = n;
        }
    }
    b.extend(2u32.to_le_bytes());
    let mut highest = 0;
    for i in [0, 1, 2, 1, 3, 2] {
        let code = highest - i;
        b.extend((code as u16).to_le_bytes());
        if code == 0 {
            highest += 1;
        }
    }
    for edge in [[0u16, 2], [0, 1], [1, 3], [2, 3]] {
        b.extend(2u32.to_le_bytes());
        for i in edge {
            b.extend(i.to_le_bytes());
        }
    }
    for (id, data) in [(1, vec![128; 8]), (2, vec![0, 255, 128]), (255, vec![0; 4])] {
        b.push(id);
        b.extend((data.len() as u32).to_le_bytes());
        b.extend(data);
    }
    b
}
fn data(d: &NetworkTerrainData) -> Value {
    match d {
        NetworkTerrainData::GoogleEarthEnterprise(_) | NetworkTerrainData::Cesium3DTiles(_) => unreachable!(),
        NetworkTerrainData::Heightmap { data: d, .. } => json!([
            "heightmap",
            d.width(),
            d.height(),
            d.child_tile_mask(),
            0,
            (0..12).map(|i| d.buffer().unwrap().get(i).unwrap()).collect::<Vec<_>>(),
            d.water_mask()
        ]),
        NetworkTerrainData::Quantized(d) => json!([
            "quantized",
            d.quantized_vertices(),
            d.indices().map(|v| (0..v.len()).map(|i| v.get(i).unwrap()).collect::<Vec<_>>()),
            d.encoded_normals(),
            d.water_mask(),
            d.child_tile_mask(),
            d.minimum_height(),
            d.maximum_height(),
            d.skirt_heights()
        ]),
    }
}
#[test]
fn cesium() {
    run(
        "core",
        "CesiumTerrainProvider",
        |_| TIGHT,
        |_, a| {
            let bytes = if a[0]["format"] == "heightmap-1.0" {
                let mut b = vec![];
                for i in 0..65 * 65 {
                    b.extend((i as u16).to_le_bytes());
                }
                b.extend([7, 0, 255, 128]);
                b
            } else {
                quantized()
            };
            let t = transport(a[0].clone(), bytes);
            let client = ResourceClient::new(t.clone());
            let p = match complete(CesiumTerrainProvider::from_url(
                Resource::new("https://terrain.test/cesium/")?,
                CesiumTerrainOptions {
                    request_vertex_normals: a[1].as_bool().unwrap(),
                    request_water_mask: true,
                    ..Default::default()
                },
                client,
            )) {
                Ok(p) => p,
                Err(e) => return Some(Out::Err(e.to_string())),
            };
            let d = match complete(p.request_tile_geometry(1, 1, 2, None)?) {
                Ok(d) => d,
                Err(e) => return Some(Out::Err(e.to_string())),
            };
            let calls = t
                .requests
                .borrow()
                .iter()
                .filter(|r| r.url.contains(".terrain"))
                .map(|r| {
                    json!([
                        r.url,
                        r.headers.iter().map(|(k, v)| (k.clone(), json!(v))).collect::<serde_json::Map<_, _>>()
                    ])
                })
                .collect::<Vec<_>>();
            ok(json!([
                provider(&p),
                p.request_vertex_normals(),
                p.request_water_mask(),
                p.request_metadata(),
                p.has_metadata(),
                p.tile_credits().iter().map(|c| json!([c.html(), c.show_on_screen])).collect::<Vec<_>>(),
                [p.get_tile_data_available(1, 1, 2), p.get_tile_data_available(99, 99, 5)],
                data(&d),
                calls
            ]))
        },
    );
}
fn factory_case(name: &str, a: &[Value]) -> Option<Out> {
    let t = transport(
        json!({"format":"quantized-mesh-1.0","tiles":["{z}/{x}/{y}.terrain"],"extensions":["octvertexnormals","watermask"]}),
        vec![],
    );
    let client = ResourceClient::new(t.clone());
    let ion = Ion::default();
    let normal = a[0].as_bool().unwrap();
    let water = a[1].as_bool().unwrap();
    let result = if name == "createWorldTerrainAsync" {
        complete(create_world_terrain_async(&ion, client, normal, water))
    } else {
        complete(create_world_bathymetry_async(&ion, client, normal))
    };
    let p = result.unwrap();
    let calls = t.requests.borrow();
    let url = &calls.first().unwrap().url;
    let id = if url.contains("/2426648/") {
        2426648
    } else {
        assert!(url.contains("/1/"));
        1
    };
    ok(json!([id, p.request_vertex_normals(), p.request_water_mask()]))
}
#[test]
fn world_factories() {
    run("core", "createWorldTerrainAsync", |_| EXACT, |_, a| factory_case("createWorldTerrainAsync", a));
    run("core", "createWorldBathymetryAsync", |_| EXACT, |_, a| factory_case("createWorldBathymetryAsync", a));
}
