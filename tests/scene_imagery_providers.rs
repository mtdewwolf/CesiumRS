//! Imagery provider batch: fixture replay and lifecycle tests.
//!
#![allow(clippy::type_complexity)]

pub mod core {
    pub use cesium_rs::core::*;
}

pub use cesium_rs::scene;

mod common;

use crate::core::image_helpers::ImageFuture;
use crate::core::*;
use crate::scene::arc_gis_map_server_imagery_provider::*;
use crate::scene::azure2_d_imagery_provider::*;
use crate::scene::bing_maps_imagery_provider::*;
use crate::scene::create_world_imagery::*;
use crate::scene::google2_d_imagery_provider::*;
use crate::scene::google_earth_enterprise_imagery_provider::*;
use crate::scene::google_earth_enterprise_maps_provider::*;
use crate::scene::grid_imagery_provider::*;
use crate::scene::ion_imagery_provider::*;
use crate::scene::tile_coordinates_imagery_provider::*;
use crate::scene::tile_map_service_imagery_provider::*;
use crate::scene::time_dynamic_imagery::*;
use crate::scene::web_map_service_imagery_provider::*;
use crate::scene::web_map_tile_service_imagery_provider::*;
use crate::scene::{ImageryImage, ImageryLoader, ImageryProvider};
use common::*;
use serde_json::{json, Value};
use std::{
    cell::RefCell,
    future::Future,
    rc::Rc,
    task::{Context as TaskContext, Poll, Waker},
};

fn ready<T>(future: impl Future<Output = T>) -> T {
    match std::pin::pin!(future).poll(&mut TaskContext::from_waker(Waker::noop())) {
        Poll::Ready(value) => value,
        Poll::Pending => panic!("unexpected pending request"),
    }
}

#[derive(Clone)]
struct Images {
    log: Rc<RefCell<Vec<String>>>,
}
impl ImageHost for Images {
    type Image = RgbaImage;
    fn dimensions(&self, image: &RgbaImage) -> (u32, u32) {
        (image.width, image.height)
    }
    fn image_from_rgba(&self, image: RgbaImage) -> Result<RgbaImage, RuntimeError> {
        Ok(image)
    }
    fn image_pixels(&self, image: &RgbaImage, _: u32, _: u32) -> Result<Vec<u8>, RuntimeError> {
        Ok(image.pixels.clone())
    }
    fn resize_image(&self, image: &RgbaImage, width: u32, height: u32) -> Result<RgbaImage, RuntimeError> {
        Ok(RgbaImage::new(width, height, image.pixels[..(width * height * 4) as usize].to_vec()).unwrap())
    }
    fn decode_image<'a>(
        &'a self,
        bytes: &'a [u8],
        _: &'a str,
        _: ImageDecodeOptions,
    ) -> Option<ImageFuture<'a, RgbaImage>> {
        self.log.borrow_mut().push("decode".into());
        let width = 2;
        let height = 2;
        let pixels = bytes.iter().copied().cycle().take((width * height * 4) as usize).collect();
        Some(Box::pin(std::future::ready(Ok(RgbaImage::new(width, height, pixels).unwrap()))))
    }
}

struct ScriptedTransport {
    log: Rc<RefCell<Vec<String>>>,
    handler: Rc<dyn Fn(&HttpRequest) -> Result<HttpResponse, RequestErrorEvent>>,
}
impl ResourceTransport for ScriptedTransport {
    fn execute(&self, request: HttpRequest) -> RequestFuture<HttpResponse> {
        self.log.borrow_mut().push(request.url.clone());
        Box::pin(std::future::ready((self.handler)(&request)))
    }
}

fn response(body: impl Into<Vec<u8>>, content_type: &str) -> Result<HttpResponse, RequestErrorEvent> {
    Ok(HttpResponse { status: 200, body: body.into(), headers: vec![("Content-Type".into(), content_type.into())] })
}

fn json_response(value: Value) -> Result<HttpResponse, RequestErrorEvent> {
    response(serde_json::to_vec(&value).unwrap(), "application/json")
}

fn text_response(value: impl Into<String>) -> Result<HttpResponse, RequestErrorEvent> {
    response(value.into().into_bytes(), "text/plain")
}

fn setup(
    handler: impl Fn(&HttpRequest) -> Result<HttpResponse, RequestErrorEvent> + 'static,
) -> (Rc<ResourceClient>, Rc<Images>, Rc<RefCell<Vec<String>>>, ImageryLoader<Images>) {
    let log = Rc::new(RefCell::new(vec![]));
    let host = Rc::new(Images { log: log.clone() });
    let client =
        Rc::new(ResourceClient::new(Rc::new(ScriptedTransport { log: log.clone(), handler: Rc::new(handler) })));
    let loader = ImageryLoader::new(client.clone(), host.clone());
    (client, host, log, loader)
}

fn image_loader() -> (Rc<ResourceClient>, Rc<Images>, Rc<RefCell<Vec<String>>>, ImageryLoader<Images>) {
    setup(|_| response(vec![1, 2, 3, 4], "image/png"))
}

fn rect_json(r: &Rectangle) -> Value {
    json!([r.west, r.south, r.east, r.north])
}

fn credit_json(c: &Credit) -> Value {
    json!({"html": c.html(), "showOnScreen": c.show_on_screen})
}

fn credits_json(c: Option<Vec<Credit>>) -> Value {
    match c {
        Some(c) => Value::Array(c.iter().map(credit_json).collect()),
        None => Value::Null,
    }
}

fn metadata(p: &dyn ImageryProvider) -> Value {
    json!({
        "rectangle": rect_json(p.rectangle()),
        "tilingSchemeRectangle": rect_json(p.tiling_scheme().rectangle()),
        "tileWidth": p.tile_width(),
        "tileHeight": p.tile_height(),
        "minimumLevel": p.minimum_level(),
        "maximumLevel": p.maximum_level(),
        "hasAlphaChannel": p.has_alpha_channel(),
        "credit": p.credit().map(credit_json),
        "discardPolicy": if p.tile_discard_policy().is_some() { json!("NeverTileDiscardPolicy") } else { Value::Null },
    })
}

fn metadata_without_level(p: &dyn ImageryProvider) -> Value {
    let mut value = metadata(p);
    value["minimumLevel"] = Value::Null;
    value["maximumLevel"] = Value::Null;
    value
}

// ------------------------------------------------------------------ Bing
fn bing_metadata_json() -> Value {
    json!({
        "resourceSets": [{"resources": [{
            "imageWidth": 256,
            "imageHeight": 300,
            "zoomMax": 20,
            "imageUrlSubdomains": ["t0", "t1", "t2"],
            "imageUrl": "https://{subdomain}.tiles.example.com/tiles/{quadkey}?c={culture}",
            "imageryProviders": [
                {"attribution": "Provider A", "coverageAreas": [
                    {"bbox": [-10, 20, 10, 40], "zoomMin": 1, "zoomMax": 10},
                    {"bbox": [20, 10, 40, 30], "zoomMin": 2, "zoomMax": 20}
                ]},
                {"attribution": "Ignored", "coverageAreas": [{"zoomMin": 1, "zoomMax": 20}]}
            ]
        }]}]
    })
}

#[test]
fn bing_maps_imagery_provider() {
    let (_client, _host, _log, loader) = image_loader();
    run(
        "scene",
        "BingMapsImageryProvider",
        |_| EXACT,
        |f, a| match f {
            "metadata" => {
                let name = s(&a[0]);
                let (_c, _h, _l, loader) = setup(|_| json_response(bing_metadata_json()));
                let options = bing_options(name);
                let p = ready(BingMapsImageryProvider::from_url(
                    loader,
                    Resource::new("https://dev.virtualearth.net/base").unwrap(),
                    options,
                ))
                .unwrap();
                let mut actual = metadata(&p);
                actual["url"] = json!(p.url());
                actual["key"] = json!(p.key());
                actual["mapStyle"] = json!(p.map_style().as_str());
                actual["mapLayer"] = json!(p.map_layer());
                actual["culture"] = json!(p.culture());
                Some(Out::Ok(actual))
            }
            "quadKeys" => Some(Out::Ok(json!([
                [0, 0, 0, BingMapsImageryProvider::<Images>::tile_xy_to_quad_key(0, 0, 0)],
                [3, 5, 4, BingMapsImageryProvider::<Images>::tile_xy_to_quad_key(3, 5, 4)],
                [5, 3, 4, BingMapsImageryProvider::<Images>::tile_xy_to_quad_key(5, 3, 4)],
                [1, 2, 8, BingMapsImageryProvider::<Images>::tile_xy_to_quad_key(1, 2, 8)],
                ["12021", {
                    "x": BingMapsImageryProvider::<Images>::quad_key_to_tile_xy("12021").0,
                    "y": BingMapsImageryProvider::<Images>::quad_key_to_tile_xy("12021").1,
                    "level": BingMapsImageryProvider::<Images>::quad_key_to_tile_xy("12021").2,
                }],
            ]))),
            "tileUrls" => {
                let p = bing_from_metadata(loader.clone());
                Some(Out::Ok(json!(p.tile_url(n(&a[0]) as i32, n(&a[1]) as i32, n(&a[2]) as u32))))
            }
            "credits" => {
                let p = bing_from_metadata(loader.clone());
                let tiles = [[0, 0, 1], [1, 2, 3], [5, 7, 4], [12, 9, 5]];
                let actual: Vec<Value> = tiles
                    .iter()
                    .map(|t| {
                        Value::Array(p.credits_for_tile(t[0], t[1], t[2] as u32).iter().map(credit_json).collect())
                    })
                    .collect();
                Some(Out::Ok(Value::Array(actual)))
            }
            _ => None,
        },
    );
}

fn bing_options(name: &str) -> BingMapsImageryProviderOptions {
    let mut options = BingMapsImageryProviderOptions::new("bing-key");
    options.tile_discard_policy = Some(Rc::new(crate::scene::NeverTileDiscardPolicy));
    if name == "custom" {
        options.map_style = crate::scene::BingMapsStyle::AerialWithLabels;
        options.map_layer = Some("labels".into());
        options.culture = Some("en-US".into());
        options.tile_protocol = Some("http:".into());
    }
    options
}

fn bing_from_metadata(loader: ImageryLoader<Images>) -> BingMapsImageryProvider<Images> {
    let json = bing_metadata_json();
    let metadata = bing_metadata_from_json(&json);
    BingMapsImageryProvider::from_metadata(
        loader,
        Resource::new("https://dev.virtualearth.net/base").unwrap(),
        bing_options("defaults"),
        metadata,
        None,
    )
}

fn bing_metadata_from_json(data: &Value) -> BingMetadata {
    let resource = &data["resourceSets"][0]["resources"][0];
    BingMetadata {
        tile_width: resource["imageWidth"].as_f64().unwrap(),
        tile_height: resource["imageHeight"].as_f64().unwrap(),
        maximum_level: resource["zoomMax"].as_f64().unwrap() as u32 - 1,
        image_url_subdomains: vec!["t0".into(), "t1".into(), "t2".into()],
        image_url_template: resource["imageUrl"].as_str().unwrap().into(),
        attribution_list: vec![],
    }
}

// ------------------------------------------------------------------ ArcGIS
fn arcgis_metadata() -> Value {
    json!({
        "tileInfo": {
            "rows": 256,
            "cols": 256,
            "spatialReference": {"wkid": 102100},
            "lods": [{"level": 0}, {"level": 1}, {"level": 2}, {"level": 3}]
        },
        "fullExtent": {"xmin": -20037508, "ymin": -20037508, "xmax": 20037508, "ymax": 20037508, "spatialReference": {"wkid": 102100}},
        "copyrightText": "ArcGIS Copyright"
    })
}

fn arcgis_identify() -> Value {
    json!({"results": [
        {"value": "Alpha", "attributes": {"name": "Alpha", "count": 2}, "geometryType": "esriGeometryPoint",
         "geometry": {"x": 10.5, "y": -20.25, "spatialReference": {"wkid": 4326}}},
        {"value": "Mercator", "attributes": {"title": "Mercator"}, "geometryType": "esriGeometryPoint",
         "geometry": {"x": 0, "y": 0, "spatialReference": {"wkid": 102100}}},
        {"attributes": {"name": "No geometry"}}
    ]})
}

#[test]
fn arc_gis_map_server_imagery_provider() {
    let (_c, _h, _l, _loader) = image_loader();
    let identify = Rc::new(RefCell::new(None::<Value>));
    let identify_for_handler = identify.clone();
    let (client, _host, log, loader2) = setup(move |request| {
        if request.url.contains("/identify") {
            let value = arcgis_identify();
            *identify_for_handler.borrow_mut() = Some(json!({"url": request.url, "features": value}));
            json_response(value)
        } else {
            json_response(arcgis_metadata())
        }
    });
    let _ = (&client, &log);
    run(
        "scene",
        "ArcGisMapServerImageryProvider",
        |_| EXACT,
        |f, a| match f {
            "metadata" => {
                let name = s(&a[0]);
                let p = ready(ArcGisMapServerImageryProvider::from_url(
                    loader2.clone(),
                    Resource::new("https://services.example.com/arcgis/rest/services/Test/MapServer").unwrap(),
                    arcgis_options(name),
                ))
                .unwrap();
                let mut actual = metadata(&p);
                actual["url"] = json!(p.url());
                actual["token"] = json!(p.token());
                actual["usingPrecachedTiles"] = json!(p.using_precached_tiles());
                actual["layers"] = json!(p.layers());
                actual["tileCredits"] = credits_json(p.get_tile_credits(0, 0, 0))
                    .as_array()
                    .cloned()
                    .map(Value::Array)
                    .unwrap_or_else(|| json!([]));
                Some(Out::Ok(actual))
            }
            "tileUrls" => {
                let name = s(&a[0]);
                let p = ready(arc_gis_for(loader2.clone(), name)).unwrap();
                Some(Out::Ok(json!(p.tile_url(n(&a[1]) as i32, n(&a[2]) as i32, n(&a[3]) as u32))))
            }
            "pickUrl" => {
                let p = ready(arc_gis_for(loader2.clone(), "tiled")).unwrap();
                let features = ready(p.pick_features(1, 2, 3, 0.5, -0.25).unwrap()).unwrap().unwrap();
                assert_eq!(features.len(), 3);
                let captured = identify.borrow().clone().unwrap();
                Some(Out::Ok(captured["url"].clone()))
            }
            "pickFeatures" => {
                let p = ready(arc_gis_for(loader2.clone(), "tiled")).unwrap();
                let features = ready(p.pick_features(1, 2, 3, 0.5, -0.25).unwrap()).unwrap().unwrap();
                let expected = arcgis_identify();
                Some(Out::Ok(json!({
                    "results": expected["results"],
                    "features": features.iter().map(feature_json).collect::<Vec<_>>(),
                })))
            }
            _ => None,
        },
    );
}

fn arcgis_options(name: &str) -> ArcGisMapServerImageryProviderOptions {
    let mut options = ArcGisMapServerImageryProviderOptions {
        token: Some("arc-token".into()),
        use_pre_cached_tiles_if_available: name != "export",
        tile_discard_policy: Some(Rc::new(crate::scene::NeverTileDiscardPolicy)),
        ..Default::default()
    };
    if name == "export" {
        options.layers = Some("0,1".into());
        options.tile_width = Some(512.0);
        options.tile_height = Some(256.0);
        options.maximum_level = Some(7);
    }
    options
}

fn arc_gis_for(
    loader: ImageryLoader<Images>,
    name: &str,
) -> impl Future<Output = Result<ArcGisMapServerImageryProvider<Images>, RuntimeError>> {
    ArcGisMapServerImageryProvider::from_url(
        loader,
        Resource::new("https://services.example.com/arcgis/rest/services/Test/MapServer").unwrap(),
        arcgis_options(name),
    )
}

fn feature_json(f: &crate::scene::ImageryLayerFeatureInfo) -> Value {
    let mut value = json!({
        "description": f.description,
        "position": f.position.map(|p| json!([p.longitude, p.latitude, p.height])),
        "properties": f.properties,
    });
    if let Some(name) = &f.name {
        value["name"] = name.clone();
    }
    value
}

fn wms_feature_json(f: &crate::scene::ImageryLayerFeatureInfo) -> Value {
    let mut value = json!({
        "description": f.description,
        "position": f.position.map(|p| json!([p.longitude, p.latitude, p.height])),
    });
    if let Some(name) = &f.name {
        value["name"] = name.clone();
    }
    value
}

// ------------------------------------------------------------------ WMS
#[test]
fn web_map_service_imagery_provider() {
    let (_c, _h, _l, loader) = image_loader();
    run(
        "scene",
        "WebMapServiceImageryProvider",
        |_| EXACT,
        |f, a| match f {
            "metadata" => {
                let name = s(&a[0]);
                let p = WebMapServiceImageryProvider::new(loader.clone(), wms_options(name)).unwrap();
                let mut actual = metadata(&p);
                actual["url"] = json!(p.url());
                actual["layers"] = json!(p.layers());
                actual["getFeatureInfoUrl"] = json!(p.get_feature_info_url().url());
                Some(Out::Ok(actual))
            }
            "tileUrls" => {
                let name = s(&a[0]);
                let p = WebMapServiceImageryProvider::new(loader.clone(), wms_options(name)).unwrap();
                Some(Out::Ok(json!(p.tile_url(n(&a[1]) as i32, n(&a[2]) as i32, n(&a[3]) as u32))))
            }
            "pickUrl" | "pickFeatures" => {
                let log = Rc::new(RefCell::new(vec![]));
                let host = Rc::new(Images { log: log.clone() });
                let client = Rc::new(ResourceClient::new(Rc::new(ScriptedTransport {
                    log: log.clone(),
                    handler: Rc::new(|request| {
                        if request.url.contains("GetFeatureInfo") {
                            json_response(json!({
                                "type": "FeatureCollection",
                                "features": [{"type": "Feature", "properties": {"name": "WMS feature"}, "geometry": {"type": "Point", "coordinates": [0.5, -0.25]}}]
                            }))
                        } else {
                            response(vec![1, 2, 3, 4], "image/png")
                        }
                    }),
                })));
                let p = WebMapServiceImageryProvider::new(ImageryLoader::new(client, host), wms_options("defaults"))
                    .unwrap();
                let features = ready(p.pick_features(1, 2, 3, 0.5, -0.25).unwrap()).unwrap().unwrap();
                if f == "pickUrl" {
                    let url = log.borrow().iter().find(|url| url.contains("GetFeatureInfo")).cloned().unwrap();
                    Some(Out::Ok(json!(url)))
                } else {
                    Some(Out::Ok(json!(features.iter().map(wms_feature_json).collect::<Vec<_>>())))
                }
            }
            _ => None,
        },
    );
}

fn wms_options(name: &str) -> WebMapServiceImageryProviderOptions {
    let mut options = WebMapServiceImageryProviderOptions::new(
        Resource::new("https://wms.example.com/service?token=abc").unwrap(),
        "layer-a,layer-b",
    );
    options.tile_discard_policy = Some(Rc::new(crate::scene::NeverTileDiscardPolicy));
    if name == "version13" {
        options.parameters = vec![
            ("VERSION".into(), "1.3.0".into()),
            ("Format".into(), "image/png".into()),
            ("Transparent".into(), "TRUE".into()),
        ];
        options.tiling_scheme = Some(std::sync::Arc::new(WebMercatorTilingScheme::default()));
    } else if name == "reverseAxis" {
        options.parameters = vec![("version".into(), "1.3.0".into())];
        options.crs = Some("EPSG:3043".into());
    }
    options
}

// ------------------------------------------------------------------ WMTS
#[test]
fn web_map_tile_service_imagery_provider() {
    let (_c, _h, _l, loader) = image_loader();
    run(
        "scene",
        "WebMapTileServiceImageryProvider",
        |_| EXACT,
        |f, a| match f {
            "metadata" => {
                let name = s(&a[0]);
                let p = WebMapTileServiceImageryProvider::new(loader.clone(), wmts_options(name)).unwrap();
                let mut actual = metadata(&p);
                actual["url"] = json!(p.url());
                actual["format"] = json!(p.format());
                Some(Out::Ok(actual))
            }
            "tileUrls" => {
                let p = WebMapTileServiceImageryProvider::new(loader.clone(), wmts_options(s(&a[0]))).unwrap();
                Some(Out::Ok(json!(p.tile_url(n(&a[1]) as i32, n(&a[2]) as i32, n(&a[3]) as u32).unwrap())))
            }
            _ => None,
        },
    );
}

fn wmts_options(name: &str) -> WebMapTileServiceImageryProviderOptions {
    if name == "kvp" {
        let mut options = WebMapTileServiceImageryProviderOptions::new(
            Resource::new("https://wmts.example.com/service").unwrap(),
            "layer",
            "default",
            "matrix",
        );
        options.format = Some("image/png".into());
        options.maximum_level = Some(6);
        options.tile_discard_policy = Some(Rc::new(crate::scene::NeverTileDiscardPolicy));
        options
    } else {
        let mut options = WebMapTileServiceImageryProviderOptions::new(
            Resource::new("https://wmts.example.com/{TileMatrixSet}/{TileMatrix}/{TileRow}/{TileCol}.png").unwrap(),
            "layer",
            "dark",
            "matrix",
        );
        options.tile_matrix_labels = Some(vec!["m0".into(), "m1".into(), "m2".into()]);
        options.dimensions = Some(vec![("Time".into(), "2020-01-01".into())]);
        options.maximum_level = Some(6);
        options.minimum_level = Some(1);
        options.tile_discard_policy = Some(Rc::new(crate::scene::NeverTileDiscardPolicy));
        options
    }
}

// ------------------------------------------------------------------ TMS
const TMS_XML: &str = r#"<TileMap version="1.0.0" tilemapservice="http://example.com">
  <Title>Test</Title><Abstract></Abstract><SRS>EPSG:4326</SRS>
  <BoundingBox minx="-120" miny="20" maxx="-60" maxy="40"/>
  <Origin x="-180" y="90"/>
  <TileFormat width="256" height="300" mime-type="image/jpeg" extension="jpg"/>
  <TileSets profile="global-geodetic">
    <TileSet href="0" units-per-pixel="0.7" order="0"/>
    <TileSet href="1" units-per-pixel="0.35" order="1"/>
    <TileSet href="2" units-per-pixel="0.17" order="2"/>
  </TileSets>
</TileMap>"#;

#[test]
fn tile_map_service_imagery_provider() {
    run(
        "scene",
        "TileMapServiceImageryProvider",
        |_| EXACT,
        |f, a| match f {
            "metadata" => {
                let name = s(&a[0]);
                let (_c, _h, _l, loader) = tms_loader(name);
                let p = ready(TileMapServiceImageryProvider::from_url(
                    loader,
                    Resource::new("https://tms.example.com/tiles").unwrap(),
                    tms_options(name),
                ))
                .unwrap();
                let mut actual = metadata(&p);
                actual["url"] = json!(p.url());
                Some(Out::Ok(actual))
            }
            "tileUrls" => {
                let name = s(&a[0]);
                let (_c, _h, _l, loader) = tms_loader(name);
                let p = ready(TileMapServiceImageryProvider::from_url(
                    loader,
                    Resource::new("https://tms.example.com/tiles").unwrap(),
                    tms_options(name),
                ))
                .unwrap();
                Some(Out::Ok(json!(p.tile_url(n(&a[1]) as i32, n(&a[2]) as i32, n(&a[3]) as u32))))
            }
            _ => None,
        },
    );
}

fn tms_loader(name: &str) -> (Rc<ResourceClient>, Rc<Images>, Rc<RefCell<Vec<String>>>, ImageryLoader<Images>) {
    let fallback = name == "fallback";
    setup(move |request| {
        if request.url.ends_with("tilemapresource.xml") {
            if fallback {
                Err(RequestErrorEvent::new(Some(404), None, None))
            } else {
                text_response(TMS_XML)
            }
        } else {
            response(vec![1, 2, 3, 4], "image/png")
        }
    })
}

fn tms_options(name: &str) -> TileMapServiceImageryProviderOptions {
    let mut options = TileMapServiceImageryProviderOptions {
        tile_discard_policy: Some(Rc::new(crate::scene::NeverTileDiscardPolicy)),
        ..Default::default()
    };
    if name == "fallback" {
        options.file_extension = Some("png".into());
        options.maximum_level = Some(5);
    }
    options
}

// ------------------------------------------------------------------ Google 2D
#[test]
fn google2_d_imagery_provider() {
    let (_c, _h, _l, loader) = setup(|request| {
        if request.url.contains("viewport") {
            json_response(json!({"copyright": format!("Copyright {}", query_zoom(&request.url))}))
        } else {
            response(vec![1, 2, 3, 4], "image/png")
        }
    });
    let p = Google2DImageryProvider::new(
        loader,
        Google2DImageryProviderOptions {
            key: "google-key".into(),
            session: "session-token".into(),
            url: Some(Resource::new("https://tile.googleapis.com/").unwrap()),
            tile_width: 512.0,
            tile_height: 256.0,
            ellipsoid: None,
            minimum_level: None,
            maximum_level: Some(2),
            rectangle: None,
            credit: Some(Credit::new("Google Test", false)),
            tile_discard_policy: None,
        },
    )
    .unwrap();
    run(
        "scene",
        "Google2DImageryProvider",
        |_| EXACT,
        |f, a| match f {
            "metadata" => {
                let mut actual = metadata(&p);
                actual["url"] = json!(p.url());
                Some(Out::Ok(actual))
            }
            "tileUrls" => Some(Out::Ok(json!(p.tile_url(n(&a[0]) as i32, n(&a[1]) as i32, n(&a[2]) as u32)))),
            "credits" => {
                let map = ready(p.get_viewport_credits()).unwrap();
                let mut values = Vec::new();
                for level in 0..=2 {
                    values.push(Value::Array(map.get(&level).unwrap().iter().map(credit_json).collect()));
                }
                Some(Out::Ok(Value::Array(values)))
            }
            _ => None,
        },
    );
}

fn query_zoom(url: &str) -> String {
    Resource::new(url)
        .unwrap()
        .options()
        .query_parameters
        .iter()
        .find(|(k, _)| k == "zoom")
        .and_then(|(_, v)| match v {
            Some(crate::core::query::QueryValue::Single(v)) => Some(v.clone()),
            _ => None,
        })
        .unwrap()
}

// ------------------------------------------------------------------ Azure
#[test]
fn azure2_d_imagery_provider() {
    let (_c, _h, _l, loader) = setup(|request| {
        if request.url.contains("attribution") {
            json_response(json!({"copyrights": ["First", format!("Level {}", query_zoom(&request.url))]}))
        } else {
            response(vec![1, 2, 3, 4], "image/png")
        }
    });
    let p = Azure2DImageryProvider::new(
        loader,
        Azure2DImageryProviderOptions {
            subscription_key: "azure-key".into(),
            url: None,
            tileset_id: Some("microsoft.base.road".into()),
            ellipsoid: None,
            minimum_level: None,
            maximum_level: Some(1),
            rectangle: None,
            tile_width: None,
            tile_height: None,
            credit: Some(Credit::new("Azure Test", false)),
            tile_discard_policy: None,
        },
    )
    .unwrap();
    run(
        "scene",
        "Azure2DImageryProvider",
        |_| EXACT,
        |f, a| match f {
            "metadata" => {
                let mut actual = metadata(&p);
                actual["url"] = json!(p.url());
                Some(Out::Ok(actual))
            }
            "tileUrls" => Some(Out::Ok(json!(p.tile_url(n(&a[0]) as i32, n(&a[1]) as i32, n(&a[2]) as u32)))),
            "credits" => {
                let map = ready(p.get_viewport_credits()).unwrap();
                Some(Out::Ok(Value::Array(
                    (0..=1)
                        .map(|level| Value::Array(map.get(&level).unwrap().iter().map(credit_json).collect()))
                        .collect(),
                )))
            }
            _ => None,
        },
    );
}

// ------------------------------------------------------------------ GEE Maps
#[test]
fn google_earth_enterprise_maps_provider() {
    let (_c, _h, _l, loader) = setup(|request| {
        if request.url.contains("geeServerDefs") {
            text_response(
                r#"{
  layers: [
    {id: 1002, version: 7, requestType: "ImageryMaps"},
    {id: 1007, version: 2, requestType: "VectorMapsRaster"}
  ],
  projection: "mercator"
}"#,
            )
        } else {
            response(vec![1, 2, 3, 4], "image/png")
        }
    });
    let p = ready(GoogleEarthEnterpriseMapsProvider::from_url(
        loader,
        Resource::new("https://earth.example.com").unwrap(),
        1002,
        GoogleEarthEnterpriseMapsProviderOptions { maximum_level: Some(9), ..Default::default() },
    ))
    .unwrap();
    run(
        "scene",
        "GoogleEarthEnterpriseMapsProvider",
        |_| EXACT,
        |f, a| match f {
            "metadata" => {
                let mut actual = metadata(&p);
                actual["url"] = json!(p.url());
                actual["path"] = json!(p.path());
                actual["channel"] = json!(p.channel());
                actual["version"] = json!(p.version());
                actual["requestType"] = json!(p.request_type());
                Some(Out::Ok(actual))
            }
            "tileUrls" => Some(Out::Ok(json!(p.tile_url(n(&a[0]) as i32, n(&a[1]) as i32, n(&a[2]) as u32)))),
            _ => None,
        },
    );
}

// ------------------------------------------------------------------ GEE Imagery
#[test]
fn google_earth_enterprise_imagery_provider() {
    let (client, host, log, loader) = setup(|request| {
        if request.url.contains("flatfile?f1-") {
            // The 0x7468dead magic makes the GEE XOR decode a pass-through;
            // JFIF at bytes 6..10 then selects the JPEG decoder.
            let mut bytes = vec![0xad, 0xde, 0x68, 0x74, 0, 0];
            bytes.extend_from_slice(b"JFIF");
            bytes.push(0);
            response(bytes, "application/octet-stream")
        } else {
            json_response(json!({}))
        }
    });
    let gee_metadata =
        GoogleEarthEnterpriseMetadata::new(Resource::new("https://gee.example.com/").unwrap(), (*client).clone());
    gee_metadata.providers.borrow_mut().insert(3, Credit::new("GEE Provider", false));
    gee_metadata.tile_info.borrow_mut().insert(
        GoogleEarthEnterpriseMetadata::tile_xy_to_quad_key(0, 0, 0),
        Some(GoogleEarthEnterpriseTileInformation::new(
            crate::core::google_earth_enterprise_tile_information::GoogleEarthEnterpriseTileInformationOptions {
                bits: 0x40,
                imagery_version: 2,
                imagery_provider: 3,
                ..Default::default()
            },
        )),
    );
    gee_metadata.tile_info.borrow_mut().insert(
        GoogleEarthEnterpriseMetadata::tile_xy_to_quad_key(1, 1, 1),
        Some(GoogleEarthEnterpriseTileInformation::new(Default::default())),
    );
    let p = GoogleEarthEnterpriseImageryProvider::from_metadata(
        gee_metadata,
        loader,
        GoogleEarthEnterpriseImageryProviderOptions {
            credit: Some(Credit::new("GEE test", false)),
            ..Default::default()
        },
    )
    .unwrap();
    run(
        "scene",
        "GoogleEarthEnterpriseImageryProvider",
        |_| EXACT,
        |f, a| match f {
            "metadata" => {
                let mut actual = metadata(&p);
                actual["url"] = json!(p.url());
                actual["discardPolicy"] = json!("GoogleEarthEnterpriseDiscardPolicy");
                Some(Out::Ok(actual))
            }
            "tileUrls" => {
                let x = n(&a[0]) as i32;
                let y = n(&a[1]) as i32;
                let level = n(&a[2]) as u32;
                let value =
                    p.tile_url(x, y, level).map(|url| url.trim_start_matches("https://gee.example.com/").to_owned());
                Some(Out::Ok(value.map(Value::String).unwrap_or(Value::Null)))
            }
            "credits" => {
                let first = p.get_tile_credits(0, 0, 0);
                let second = p.get_tile_credits(1, 1, 1);
                Some(Out::Ok(json!([credits_json(first), credits_json(second),])))
            }
            _ => None,
        },
    );
    // Lifecycle: encrypted-header bytes decode through the host.
    let image = ready(p.request_image(0, 0, 0).unwrap()).unwrap();
    assert!(matches!(image, ImageryImage::Image(_)));
    assert!(log.borrow().iter().any(|url| url.contains("f1-03-i.2")), "{:?}", log.borrow());
    let _ = host;
    let invalid = ready(p.request_image(1, 1, 1).unwrap()).unwrap();
    assert!(p.tile_discard_policy().unwrap().should_discard_image(&invalid));
}

// ------------------------------------------------------------------ procedural
#[derive(Clone, Default)]
struct CanvasRecorder {
    commands: RefCell<Vec<String>>,
}
impl ImageHost for CanvasRecorder {
    type Image = RgbaImage;
    fn dimensions(&self, image: &RgbaImage) -> (u32, u32) {
        (image.width, image.height)
    }
    fn image_from_rgba(&self, image: RgbaImage) -> Result<RgbaImage, RuntimeError> {
        Ok(image)
    }
    fn image_pixels(&self, image: &RgbaImage, _: u32, _: u32) -> Result<Vec<u8>, RuntimeError> {
        Ok(image.pixels.clone())
    }
    fn resize_image(&self, image: &RgbaImage, width: u32, height: u32) -> Result<RgbaImage, RuntimeError> {
        Ok(RgbaImage::new(width, height, image.pixels[..(width * height * 4) as usize].to_vec()).unwrap())
    }
    fn decode_image<'a>(
        &'a self,
        _: &'a [u8],
        _: &'a str,
        _: ImageDecodeOptions,
    ) -> Option<ImageFuture<'a, RgbaImage>> {
        Some(Box::pin(std::future::ready(Ok(RgbaImage::new(1, 1, vec![0; 4]).unwrap()))))
    }
}
impl CanvasHost for CanvasRecorder {
    fn create_canvas(&self, width: u32, height: u32) -> Result<RgbaImage, RuntimeError> {
        RgbaImage::new(width, height, vec![0; (width * height * 4) as usize])
            .ok_or_else(|| RuntimeError::new("bad canvas"))
    }
    fn resize_canvas(&self, _canvas: &RgbaImage, _width: u32, _height: u32) -> Result<(), RuntimeError> {
        Ok(())
    }
    fn command(&self, _canvas: &RgbaImage, command: CanvasCommand<'_, RgbaImage>) -> Result<(), RuntimeError> {
        self.commands.borrow_mut().push(match command {
            CanvasCommand::FillStyle(v) => format!("fillStyle:{v}"),
            CanvasCommand::StrokeStyle(v) => format!("strokeStyle:{v}"),
            CanvasCommand::LineWidth(v) => format!("lineWidth:{v}"),
            CanvasCommand::FillRect(..) => "fillRect".into(),
            CanvasCommand::MoveTo(..) => "moveTo".into(),
            CanvasCommand::LineTo(..) => "lineTo".into(),
            CanvasCommand::Stroke => "stroke".into(),
            CanvasCommand::Fill => "fill".into(),
            CanvasCommand::BeginPath => "beginPath".into(),
            CanvasCommand::ClosePath => "closePath".into(),
            CanvasCommand::Font(v) => format!("font:{v}"),
            CanvasCommand::FillText(text, x, y) => format!("fillText:{text}:{x}:{y}"),
            _ => "other".into(),
        });
        Ok(())
    }
    fn measure_text(&self, _canvas: &RgbaImage, text: &str, _font: &str) -> Result<CanvasTextMetrics, RuntimeError> {
        Ok(CanvasTextMetrics { width: text.len() as f64 * 10.0, ascent: 0.0, descent: 0.0, left: 0.0 })
    }
}

#[test]
fn grid_imagery_provider() {
    let host = Rc::new(CanvasRecorder::default());
    let p = GridImageryProvider::new(
        host.clone(),
        GridImageryProviderOptions {
            cells: 4,
            tile_width: 300.0,
            tile_height: 200.0,
            canvas_size: 128.0,
            ..Default::default()
        },
    )
    .unwrap();
    run(
        "scene",
        "GridImageryProvider",
        |_| EXACT,
        |f, _| match f {
            "metadata" => {
                let mut actual = metadata_without_level(&p);
                actual["canvasSize"] = json!(128);
                Some(Out::Ok(actual))
            }
            _ => None,
        },
    );
    let commands = host.commands.borrow();
    assert_eq!(commands.iter().filter(|c| *c == "stroke").count(), 6);
    // 30 grid subpaths plus one MoveTo for each of the three synthesized
    // stroked rectangles (CanvasCommand has no StrokeRect primitive).
    assert_eq!(commands.iter().filter(|c| *c == "moveTo").count(), 33);
    assert!(matches!(ready(p.request_image(1, 2, 3).unwrap()), Ok(ImageryImage::Image(_))));
}

#[test]
fn tile_coordinates_imagery_provider() {
    let host = Rc::new(CanvasRecorder::default());
    let p = TileCoordinatesImageryProvider::new(
        host.clone(),
        TileCoordinatesImageryProviderOptions { tile_width: 300.0, tile_height: 200.0, ..Default::default() },
    );
    run(
        "scene",
        "TileCoordinatesImageryProvider",
        |_| EXACT,
        |f, _| match f {
            "metadata" => Some(Out::Ok(metadata_without_level(&p))),
            _ => None,
        },
    );
    let _image = ready(p.request_image(4, 5, 6).unwrap()).unwrap();
    let commands = host.commands.borrow();
    for text in ["L: 6", "X: 4", "Y: 5"] {
        assert!(commands.iter().any(|c| c.contains(text)), "{text} not drawn: {commands:?}");
    }
}

// ------------------------------------------------------------------ time dynamic
#[test]
fn time_dynamic_imagery_interval_and_cache() {
    let mut times = TimeIntervalCollection::new();
    times.add_interval(TimeInterval::new(
        JulianDate::default().add_seconds(0.0),
        JulianDate::default().add_seconds(10.0),
        true,
        false,
        Some(vec![("Time".to_string(), "a".to_string())]),
    ));
    times.add_interval(TimeInterval::new(
        JulianDate::default().add_seconds(10.0),
        JulianDate::default().add_seconds(20.0),
        true,
        false,
        Some(vec![("Time".to_string(), "b".to_string())]),
    ));
    let reloads = Rc::new(RefCell::new(0));
    let requests = Rc::new(RefCell::new(vec![]));
    let r = reloads.clone();
    let q = requests.clone();
    let mut dynamic = TimeDynamicImagery::new(TimeDynamicImageryOptions {
        times,
        request_image_function: Rc::new(move |x, y, level, interval| {
            q.borrow_mut().push((x, y, level, interval.and_then(|i| i.data.clone())));
            Some(Box::pin(std::future::ready(Err(RuntimeError::new("test")))))
        }),
        reload_function: Rc::new(move || *r.borrow_mut() += 1),
    });
    assert!(dynamic.update(&JulianDate::default().add_seconds(5.0), false, false, 0.0));
    let interval = dynamic.current_interval().unwrap();
    assert_eq!(interval.data.as_ref().unwrap()[0].1, "a");
    dynamic.check_approaching_interval(1, 2, 3, &JulianDate::default().add_seconds(2.0), true, true, 1.0, None);
    assert_eq!(dynamic.pending_preload_count(), 1);
    assert!(dynamic.update(&JulianDate::default().add_seconds(15.0), false, false, 0.0));
    assert_eq!(dynamic.current_interval().unwrap().data.as_ref().unwrap()[0].1, "b");
    assert_eq!(dynamic.pending_preload_count(), 0);
    assert_eq!(*reloads.borrow(), 2);
    assert!(requests.borrow().is_empty());
}

// ------------------------------------------------------------------ Ion / world imagery
#[test]
fn ion_imagery_provider_uses_tms_factory_and_credits() {
    let (_c, _h, log, loader) = setup(|request| {
        if request.url.contains("/v1/assets/3812/endpoint") {
            json_response(json!({
                "type": "IMAGERY",
                "url": "https://tms.example.com/ion-tiles",
                "attributions": [{"html": "Ion attribution", "collapsible": false}]
            }))
        } else if request.url.ends_with("tilemapresource.xml") {
            Err(RequestErrorEvent::new(Some(404), None, None))
        } else {
            response(vec![1, 2, 3, 4], "image/png")
        }
    });
    let p = ready(IonImageryProvider::from_asset_id(loader, 3812, IonImageryProviderOptions::default())).unwrap();
    let credits = p.get_tile_credits(0, 0, 0).unwrap();
    assert!(credits.iter().any(|c| c.html() == "Ion attribution"));
    let image = ready(p.request_image(1, 2, 3).unwrap()).unwrap();
    assert!(matches!(image, ImageryImage::Image(_)));
    let urls = log.borrow();
    assert!(urls.iter().any(|url| url.contains("/v1/assets/3812/endpoint")));
    assert!(urls.iter().any(|url| url.contains("tilemapresource.xml")));
    assert!(urls.iter().any(|url| url.contains("ion-tiles/3/1/5.png")), "{urls:?}");
}

#[test]
fn ion_rejects_non_imagery_assets() {
    let (_c, _h, _log, loader) = setup(|_| json_response(json!({"type": "TERRAIN", "url": "https://x/"})));
    let result = ready(IonImageryProvider::from_asset_id(loader, 1, IonImageryProviderOptions::default()));
    match result {
        Ok(_) => panic!("expected a non-imagery error"),
        Err(error) => assert!(error.message.contains("not an imagery asset")),
    }
}

#[test]
fn world_imagery_uses_the_style_asset_id() {
    assert_eq!(world_imagery_asset_id(crate::scene::IonWorldImageryStyle::Aerial), 2);
    let request = WorldImageryRequest::new(WorldImageryOptions {
        style: Some(crate::scene::IonWorldImageryStyle::Road),
        ..Default::default()
    });
    assert_eq!(request.asset_id(), 4);
}

fn two_interval_times() -> TimeIntervalCollection<Vec<(String, String)>> {
    let base = JulianDate::default();
    let mut times = TimeIntervalCollection::new();
    times.add_interval(TimeInterval::new(
        base.add_seconds(0.0),
        base.add_seconds(10.0),
        true,
        false,
        Some(vec![("Time".into(), "a".into())]),
    ));
    times.add_interval(TimeInterval::new(
        base.add_seconds(10.0),
        base.add_seconds(20.0),
        true,
        false,
        Some(vec![("Time".into(), "b".into())]),
    ));
    times
}

fn test_clock() -> Rc<RefCell<Clock>> {
    let base = JulianDate::default();
    Rc::new(RefCell::new(
        Clock::new(crate::core::clock::ClockOptions {
            start_time: Some(base),
            stop_time: Some(base.add_seconds(20.0)),
            current_time: Some(base.add_seconds(5.0)),
            multiplier: Some(0.0),
            can_animate: Some(false),
            should_animate: Some(false),
            ..Default::default()
        })
        .unwrap(),
    ))
}

#[test]
fn wms_time_dynamic_dimensions_change_tile_urls() {
    let log = Rc::new(RefCell::new(vec![]));
    let host = Rc::new(Images { log: log.clone() });
    let client = Rc::new(ResourceClient::new(Rc::new(ScriptedTransport {
        log: log.clone(),
        handler: Rc::new(|_| response(vec![1, 2, 3, 4], "image/png")),
    })));
    let clock = test_clock();
    let mut options = wms_options("defaults");
    options.clock = Some(clock.clone());
    options.times = Some(two_interval_times());
    let p = WebMapServiceImageryProvider::new(ImageryLoader::new(client, host), options).unwrap();
    let _ = ready(p.request_image(1, 2, 3).unwrap()).unwrap();
    let first = log.borrow().iter().rev().find(|url| url.starts_with("http")).cloned().unwrap();
    assert!(first.contains("Time=a"), "{first}");
    clock.borrow_mut().set_current_time(JulianDate::default().add_seconds(15.0));
    let _ = ready(p.request_image(1, 2, 3).unwrap()).unwrap();
    assert_eq!(p.current_interval_data().unwrap()[0].1, "b");
    // The interval change also preloads the previous interval (Cesium's
    // multiplier-0 approaching-interval arithmetic), so scan all requests.
    assert!(log.borrow().iter().any(|url| url.contains("Time=b")), "{:?}", log.borrow());
    assert!(!first.contains("Time=b"));
}

#[test]
fn wmts_time_dynamic_dimensions_change_tile_urls() {
    let log = Rc::new(RefCell::new(vec![]));
    let host = Rc::new(Images { log: log.clone() });
    let client = Rc::new(ResourceClient::new(Rc::new(ScriptedTransport {
        log: log.clone(),
        handler: Rc::new(|_| response(vec![1, 2, 3, 4], "image/png")),
    })));
    let clock = test_clock();
    let mut options = WebMapTileServiceImageryProviderOptions::new(
        Resource::new("https://wmts.example.com/{Time}/{TileMatrixSet}/{TileMatrix}/{TileRow}/{TileCol}.png").unwrap(),
        "layer",
        "style",
        "matrix",
    );
    options.clock = Some(clock.clone());
    options.times = Some(two_interval_times());
    let p = WebMapTileServiceImageryProvider::new(ImageryLoader::new(client, host), options).unwrap();
    let _ = ready(p.request_image(1, 2, 3).unwrap()).unwrap();
    let first = log.borrow().iter().rev().find(|url| url.starts_with("http")).cloned().unwrap();
    assert!(first.contains("/a/"), "{first}");
    clock.borrow_mut().set_current_time(JulianDate::default().add_seconds(15.0));
    let _ = ready(p.request_image(1, 2, 3).unwrap()).unwrap();
    // With a zero multiplier Cesium can also prepare the previous interval,
    // so the most recent request is not necessarily the current interval.
    assert!(log.borrow().iter().any(|url| url.contains("/b/")), "{:?}", log.borrow());
}

#[test]
fn wms_interval_change_notifies_host_reload() {
    let (_, _, _, loader) = image_loader();
    let clock = test_clock();
    let mut options = wms_options("defaults");
    options.clock = Some(clock.clone());
    options.times = Some(two_interval_times());
    let provider = WebMapServiceImageryProvider::new(loader, options).unwrap();
    let calls = Rc::new(std::cell::Cell::new(0));
    let counter = calls.clone();
    provider.set_reload(Some(Rc::new(move || counter.set(counter.get() + 1))));
    provider.update_time();
    assert_eq!(calls.get(), 1);
    provider.update_time();
    assert_eq!(calls.get(), 1);
    clock.borrow_mut().set_current_time(JulianDate::default().add_seconds(15.0));
    provider.update_time();
    assert_eq!(calls.get(), 2);
    provider.set_reload(None);
    clock.borrow_mut().set_current_time(JulianDate::default().add_seconds(5.0));
    provider.update_time();
    assert_eq!(calls.get(), 2);
}
