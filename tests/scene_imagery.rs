//! Imagery providers (`UrlTemplateImageryProvider`, `GetFeatureInfoFormat`,
//! `OpenStreetMapImageryProvider`, `MapboxImageryProvider`,
//! `MapboxStyleImageryProvider`, `SingleTileImageryProvider`,
//! `DiscardMissingTileImagePolicy`) replay and lifecycle tests.

mod common;
use cesium_rs::core::image_helpers::ImageFuture;
use cesium_rs::core::*;
use cesium_rs::scene::*;
use common::*;
use serde_json::{json, Value};
use std::{
    cell::RefCell,
    collections::HashMap,
    future::Future,
    rc::Rc,
    sync::Arc,
    task::{Context as TaskContext, Poll, Waker},
};

fn ready<T>(future: impl Future<Output = T>) -> T {
    match std::pin::pin!(future).poll(&mut TaskContext::from_waker(Waker::noop())) {
        Poll::Ready(value) => value,
        Poll::Pending => panic!("unexpected pending request"),
    }
}

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

/// Records every request and serves text bodies to URLs that contain `plain`.
struct Transport {
    log: Rc<RefCell<Vec<String>>>,
    image: bool,
}
impl ResourceTransport for Transport {
    fn execute(&self, request: HttpRequest) -> RequestFuture<HttpResponse> {
        self.log.borrow_mut().push(request.url.clone());
        Box::pin(std::future::ready(if self.image {
            Ok(HttpResponse {
                status: 200,
                body: vec![1, 2, 3, 4],
                headers: vec![("Content-Type".into(), "image/png".into())],
            })
        } else if request.url.contains("plain") {
            Ok(HttpResponse {
                status: 200,
                body: b"<html><title>Name</title><body>Description</body></html>".to_vec(),
                headers: vec![("Content-Type".into(), "text/plain".into())],
            })
        } else {
            Err(RequestErrorEvent::new(Some(404), None, None))
        }))
    }
}

fn client(log: Rc<RefCell<Vec<String>>>, image: bool) -> Rc<ResourceClient> {
    Rc::new(ResourceClient::new(Rc::new(Transport { log, image })))
}

fn provider(
    client: Rc<ResourceClient>,
    options: UrlTemplateImageryProviderOptions,
) -> UrlTemplateImageryProvider<Images> {
    let host = Rc::new(Images { log: Rc::default() });
    UrlTemplateImageryProvider::new(ImageryLoader::new(client, host), options).unwrap()
}

fn url_options(url: &str) -> UrlTemplateImageryProviderOptions {
    UrlTemplateImageryProviderOptions::new(Resource::new(url).unwrap())
}

fn scheme(name: &str) -> Arc<dyn TilingScheme> {
    match name {
        "GeographicTilingScheme" => Arc::new(GeographicTilingScheme::default()),
        "WebMercatorTilingScheme" => Arc::new(WebMercatorTilingScheme::default()),
        other => panic!("unknown tiling scheme {other}"),
    }
}

fn rect_json(rectangle: &Rectangle) -> Value {
    json!([rectangle.west, rectangle.south, rectangle.east, rectangle.north])
}

fn metadata_options(name: &str) -> (UrlTemplateImageryProviderOptions, &'static str) {
    match name {
        "defaults" => (url_options("https://example.com/{z}/{x}/{y}.png"), "WebMercatorTilingScheme"),
        "full" => {
            let mut options = url_options("https://example.com/{z}/{x}/{y}.png");
            options.pick_features_url = Some(Resource::new("https://example.com/pick/{z}/{x}/{y}/{i}/{j}").unwrap());
            options.subdomains = Some(vec!["a".into(), "b".into()]);
            options.credit = Some(Credit::new("<a href=\"https://example.com\">Example</a>", true));
            options.minimum_level = Some(2);
            options.maximum_level = Some(12);
            options.rectangle = Some(Rectangle::from_degrees(-10.0, -20.0, 30.0, 40.0));
            options.tile_width = Some(512.0);
            options.tile_height = Some(128.0);
            options.has_alpha_channel = Some(false);
            (options, "WebMercatorTilingScheme")
        }
        "geographic" => {
            let mut options = url_options("https://example.com/{z}/{x}/{y}.png");
            options.subdomains = Some(vec!["w".into(), "x".into(), "y".into(), "z".into()]);
            options.tiling_scheme = Some(scheme("GeographicTilingScheme"));
            (options, "GeographicTilingScheme")
        }
        "levelZeroTiles" => {
            let mut options = url_options("https://example.com/{z}/{reverseY}.png");
            options.tiling_scheme = Some(Arc::new(WebMercatorTilingScheme::new(Ellipsoid::WGS84, 2, 1, None)));
            (options, "WebMercatorTilingScheme")
        }
        other => panic!("unknown metadata case {other}"),
    }
}

fn metadata(provider: &UrlTemplateImageryProvider<Images>) -> Value {
    json!({
        "url": provider.url(),
        "pickFeaturesUrl": provider.pick_features_url(),
        "tileWidth": provider.tile_width(),
        "tileHeight": provider.tile_height(),
        "minimumLevel": provider.minimum_level(),
        "maximumLevel": provider.maximum_level(),
        "rectangle": rect_json(provider.rectangle()),
        "tilingSchemeRectangle": rect_json(provider.tiling_scheme().rectangle()),
        "hasAlphaChannel": provider.has_alpha_channel(),
        "credit": provider.credit().map(Credit::html),
        "discardPolicy": Value::Null,
        "tileCredits": Value::Null,
        "proxy": Value::Null,
    })
}

fn tile_provider(name: &str) -> UrlTemplateImageryProvider<Images> {
    let mut options = match name {
        "xyz" => {
            let mut options = url_options("https://example.com/{z}/{x}/{y}.png");
            options.maximum_level = Some(5);
            options
        }
        "subdomains" => {
            let mut options = url_options("https://{s}.example.com/{z}/{x}/{y}.png");
            options.subdomains = Some(vec!["a".into(), "b".into(), "c".into()]);
            options
        }
        "reverse" => {
            let mut options = url_options("https://example.com/{reverseZ}/{reverseX}/{reverseY}.png");
            options.maximum_level = Some(7);
            options.tiling_scheme = Some(scheme("GeographicTilingScheme"));
            options.url_scheme_zero_padding = HashMap::from([
                ("{reverseX}".to_owned(), "0000".to_owned()),
                ("{reverseY}".to_owned(), "00".to_owned()),
            ]);
            options
        }
        "degrees" => {
            let mut options = url_options(
                "https://example.com/{westDegrees}/{southDegrees}/{eastDegrees}/{northDegrees}/{width}/{height}.png",
            );
            options.tile_width = Some(300.0);
            options.tile_height = Some(200.0);
            options.tiling_scheme = Some(scheme("GeographicTilingScheme"));
            options
        }
        "projected" => {
            url_options("https://example.com/{westProjected},{southProjected},{eastProjected},{northProjected}.png")
        }
        "custom" => {
            let mut options = url_options("https://example.com/{Time}/{z}/{x}.png");
            options.custom_tags = HashMap::from([(
                "Time".to_owned(),
                Rc::new(|args: &CustomTagArgs| format!("t{}-{{x}}", args.x + args.y + args.level as i32))
                    as Rc<dyn Fn(&CustomTagArgs) -> String>,
            )]);
            options
        }
        other => panic!("unknown tileUrls case {other}"),
    };
    options.get_feature_info_formats.clear();
    provider(client(Rc::default(), false), options)
}

fn pick_provider(name: &str) -> UrlTemplateImageryProvider<Images> {
    let tiling_scheme = match name {
        "webmercator" => scheme("WebMercatorTilingScheme"),
        "geographic" => scheme("GeographicTilingScheme"),
        other => panic!("unknown pickUrls case {other}"),
    };
    let mut options = url_options("https://example.com/{z}/{x}/{y}.png");
    options.pick_features_url = Some(
        Resource::new(
            "https://example.com/pick/{z}/{x}/{y}/{i}/{j}/{reverseI}/{reverseJ}/{longitudeDegrees}/{latitudeDegrees}/{longitudeProjected}/{latitudeProjected}/{format}.json",
        )
        .unwrap(),
    );
    options.tiling_scheme = Some(tiling_scheme);
    options.tile_width = Some(256.0);
    options.tile_height = Some(256.0);
    options.get_feature_info_formats = vec![GetFeatureInfoFormat::json()];
    provider(client(Rc::default(), false), options)
}

fn feature_json(feature: &ImageryLayerFeatureInfo) -> Value {
    json!({
        "name": feature.name,
        "description": feature.description,
        "position": feature.position.map(|p| json!([p.longitude, p.latitude, p.height])),
        "properties": feature.properties,
        "data": feature.data,
    })
}

#[test]
fn url_template_imagery_provider() {
    run(
        "scene",
        "UrlTemplateImageryProvider",
        |_| EXACT,
        |f, a| match f {
            "metadata" => {
                let name = s(&a[0]);
                let (options, scheme_name) = metadata_options(name);
                let provider = provider(client(Rc::default(), false), options);
                let mut actual = metadata(&provider);
                actual["tilingScheme"] = json!(scheme_name);
                Some(Out::Ok(actual))
            }
            "tileUrls" => {
                let name = s(&a[0]);
                let provider = tile_provider(name);
                let tiles = list(&a[1], |tile| list(tile, |v| n(v) as i32));
                let actual: Vec<String> =
                    tiles.iter().map(|tile| provider.tile_url(tile[0], tile[1], tile[2] as u32)).collect();
                Some(Out::Ok(json!(actual)))
            }
            "pickUrls" => {
                let name = s(&a[0]);
                let provider = pick_provider(name);
                let actual = provider.pick_features_tile_url(1, 2, 3, 0.5, -0.25, "application/json");
                Some(Out::Ok(json!(vec![actual])))
            }
            "pickWithoutUrl" => {
                let mut options = url_options("https://example.com/{z}/{x}/{y}.png");
                options.get_feature_info_formats = vec![GetFeatureInfoFormat::json()];
                let p = provider(client(Rc::default(), false), options);
                let pick = p.pick_features(1, 2, 3, 0.5, -0.25);
                Some(Out::Ok(match pick {
                    None => Value::Null,
                    Some(_) => panic!("expected None"),
                }))
            }
            "pickDisabled" => {
                let mut options = url_options("https://example.com/{z}/{x}/{y}.png");
                options.pick_features_url = Some(Resource::new("https://example.com/pick/{x}").unwrap());
                options.enable_pick_features = Some(false);
                options.get_feature_info_formats = vec![GetFeatureInfoFormat::json()];
                let p = provider(client(Rc::default(), false), options);
                let pick = p.pick_features(1, 2, 3, 0.5, -0.25);
                Some(Out::Ok(match pick {
                    None => Value::Null,
                    Some(_) => panic!("expected None"),
                }))
            }
            "pickFallback" => {
                let mut results = Vec::new();
                // 1: XML then text
                {
                    let log = Rc::new(RefCell::new(vec![]));
                    let mut options = url_options("https://example.com/{z}/{x}/{y}.png");
                    options.pick_features_url = Some(Resource::new("https://example.com/pick/{x}/{format}").unwrap());
                    options.get_feature_info_formats = vec![GetFeatureInfoFormat::xml(), GetFeatureInfoFormat::text()];
                    let xml_then_text = provider(client(log.clone(), false), options);
                    let features = ready(xml_then_text.pick_features(1, 2, 3, 0.5, -0.25).unwrap()).unwrap().unwrap();
                    results.push(json!({
                        "captured": json!(log.borrow().clone()),
                        "value": json!(features.iter().map(feature_json).collect::<Vec<_>>()),
                    }));
                }
                // 2: All fail
                {
                    let log = Rc::new(RefCell::new(vec![]));
                    let mut options = url_options("https://example.com/{z}/{x}/{y}.png");
                    options.pick_features_url = Some(Resource::new("https://example.com/pick/{x}/{format}").unwrap());
                    options.get_feature_info_formats = vec![GetFeatureInfoFormat::json(), GetFeatureInfoFormat::xml()];
                    let all_fail = provider(client(log.clone(), false), options);
                    let features = ready(all_fail.pick_features(1, 2, 3, 0.5, -0.25).unwrap()).unwrap().unwrap();
                    results.push(json!({
                        "captured": json!(log.borrow().clone()),
                        "value": json!(features.iter().map(feature_json).collect::<Vec<_>>()),
                    }));
                }
                Some(Out::Ok(json!(results)))
            }
            _ => None,
        },
    );
}

fn text_feature(feature: &ImageryLayerFeatureInfo) -> Value {
    json!({
        "name": feature.name,
        "description": feature.description,
        "position": feature.position.map(|p| json!([p.longitude, p.latitude, p.height])),
        "properties": feature.properties,
        "data": feature.data,
    })
}

fn xml_samples() -> Vec<(&'static str, &'static str)> {
    vec![
        ("esri", "<?xml version=\"1.0\"?><FeatureInfoResponse xmlns=\"http://www.esri.com/wms\"><FIELDS OBJECTID=\"1\" NAME=\"Alpha\" VALUE=\"3.5\"/><FIELDS OBJECTID=\"2\" NAME=\"Beta\" VALUE=\"4\"/></FeatureInfoResponse>"),
        ("thredds", "<?xml version=\"1.0\"?><FeatureInfoResponse xmlns=\"http://www.esri.com/wms\"><FeatureInfo><name>Gamma</name><value>7</value><nested><inner>deep</inner></nested></FeatureInfo></FeatureInfoResponse>"),
        ("mapinfo", "<?xml version=\"1.0\"?><MultiFeatureCollection xmlns=\"http://www.mapinfo.com/mxp\"><Feature><Val ref=\"name\"> Delta </Val><Val ref=\"count\">5</Val><Val>ignored</Val></Feature></MultiFeatureCollection>"),
        ("gml", "<?xml version=\"1.0\"?><wfs:FeatureCollection xmlns:wfs=\"http://www.opengis.net/wfs\" xmlns:gml=\"http://www.opengis.net/gml\"><gml:featureMember><layer:city xmlns:layer=\"urn:test\"><layer:name>Epsilon</layer:name><layer:point><gml:Point><gml:coordinates>1,2</gml:coordinates></gml:Point></layer:point></layer:city></gml:featureMember></wfs:FeatureCollection>"),
        ("msGml", "<?xml version=\"1.0\"?><msGMLOutput><cities_layer><city><name>Zeta</name><population>1000</population></city><city><name>Eta</name></city></cities_layer></msGMLOutput>"),
        ("unknown", "<?xml version=\"1.0\"?><Something><a x=\"1\">text &amp; more</a><b>2</b></Something>"),
        ("exception", "<?xml version=\"1.0\"?><ServiceExceptionReport version=\"1.3.0\"><ServiceException>bad request</ServiceException></ServiceExceptionReport>"),
    ]
}

fn text_samples() -> Vec<(&'static str, &'static str)> {
    vec![
        ("simple", "<html><title>Title</title><body>Hello</body></html>"),
        ("emptyBody", "<html><body>   </body></html>"),
        ("exception", "<ServiceExceptionReport><ServiceException>bad</ServiceException></ServiceExceptionReport>"),
        ("plain", "just text"),
        ("noTitle", "<html><body>No title</body></html>"),
    ]
}

#[test]
fn get_feature_info_format() {
    run(
        "scene",
        "GetFeatureInfoFormat",
        |_| EXACT,
        |f, a| match f {
            "defaults" => {
                let formats = [
                    GetFeatureInfoFormat::json(),
                    GetFeatureInfoFormat::xml(),
                    GetFeatureInfoFormat::html(),
                    GetFeatureInfoFormat::text(),
                ];
                let actual: Vec<Value> = formats
                    .iter()
                    .map(|format| {
                        json!({
                            "type": format.response_type.as_str(),
                            "format": format.format,
                            "hasCallback": true,
                        })
                    })
                    .collect();
                Some(Out::Ok(json!(actual)))
            }
            "json" => {
                let geo_json = json!({
                    "type": "FeatureCollection",
                    "features": [
                        {
                            "type": "Feature",
                            "properties": {"name": "Alpha", "title": "A", "count": 2, "nested": {"a": 1}, "blank": null},
                            "geometry": {"type": "Point", "coordinates": [10.5, -20.25]},
                        },
                        {
                            "type": "Feature",
                            "properties": {"label": "no name", "otherName": "second"},
                            "geometry": {"type": "LineString", "coordinates": [[0.0, 0.0], [1.0, 1.0]]},
                        },
                        {"type": "Feature", "properties": null},
                    ],
                });
                let features =
                    GetFeatureInfoFormat::json().callback(FeatureInfoResponse::Json(geo_json)).unwrap().unwrap();
                let actual = json!(features.iter().map(text_feature).collect::<Vec<_>>());
                Some(Out::Ok(actual))
            }
            "xml" => {
                let name = s(&a[0]);
                let xml = xml_samples().into_iter().find(|(sample, _)| *sample == name).unwrap().1;
                let document = XmlDocument::parse(xml.to_owned()).unwrap();
                match GetFeatureInfoFormat::xml().callback(FeatureInfoResponse::Xml(document)) {
                    Ok(features) => {
                        let actual = features
                            .map(|f| json!(f.iter().map(text_feature).collect::<Vec<_>>()))
                            .unwrap_or(Value::Null);
                        Some(Out::Ok(actual))
                    }
                    Err(e) => Some(Out::Err(e.message)),
                }
            }
            "text" => {
                let name = s(&a[0]);
                let text = text_samples().into_iter().find(|(sample, _)| *sample == name).unwrap().1;
                let features =
                    GetFeatureInfoFormat::text().callback(FeatureInfoResponse::Text(text.to_owned())).unwrap();
                let actual =
                    features.map(|f| json!(f.iter().map(text_feature).collect::<Vec<_>>())).unwrap_or(Value::Null);
                Some(Out::Ok(actual))
            }
            _ => None,
        },
    );
}

#[test]
fn open_street_map_imagery_provider() {
    let c = client(Rc::default(), false);
    let host = Rc::new(Images { log: Rc::default() });

    run(
        "scene",
        "OpenStreetMapImageryProvider",
        |_| EXACT,
        |f, a| match f {
            "metadata" => {
                let name = s(&a[0]);
                let options = match name {
                    "defaults" => OpenStreetMapImageryProviderOptions::default(),
                    "custom" => OpenStreetMapImageryProviderOptions {
                        url: Some(Resource::new("https://custom.osm.org/").unwrap()),
                        file_extension: Some("jpg".into()),
                        retina_tiles: true,
                        maximum_level: Some(19),
                        credit: Some(Credit::new("<a href=\"https://example.com\">Custom OSM</a>", true)),
                        ..Default::default()
                    },
                    _ => panic!("unknown OSM case"),
                };
                let p =
                    OpenStreetMapImageryProvider::new(ImageryLoader::new(c.clone(), host.clone()), options).unwrap();
                let actual = json!({
                    "url": p.url(),
                    "pickFeaturesUrl": Value::Null,
                    "tileWidth": p.tile_width(),
                    "tileHeight": p.tile_height(),
                    "minimumLevel": p.minimum_level(),
                    "maximumLevel": p.maximum_level(),
                    "rectangle": rect_json(p.rectangle()),
                    "tilingScheme": "WebMercatorTilingScheme",
                    "tilingSchemeRectangle": rect_json(p.tiling_scheme().rectangle()),
                    "hasAlphaChannel": p.has_alpha_channel(),
                    "credit": p.credit().map(Credit::html),
                    "discardPolicy": Value::Null,
                    "tileCredits": Value::Null,
                    "proxy": Value::Null,
                });
                Some(Out::Ok(actual))
            }
            "tileUrls" => {
                let name = s(&a[0]);
                let options = match name {
                    "defaults" => OpenStreetMapImageryProviderOptions::default(),
                    "custom" => OpenStreetMapImageryProviderOptions {
                        url: Some(Resource::new("https://custom.osm.org/").unwrap()),
                        file_extension: Some("jpg".into()),
                        retina_tiles: true,
                        maximum_level: Some(19),
                        credit: Some(Credit::new("<a href=\"https://example.com\">Custom OSM</a>", true)),
                        ..Default::default()
                    },
                    _ => panic!("unknown OSM case"),
                };
                let p =
                    OpenStreetMapImageryProvider::new(ImageryLoader::new(c.clone(), host.clone()), options).unwrap();
                Some(Out::Ok(json!(vec![p.tile_url(1, 2, 3)])))
            }
            "tooManyTiles" => {
                let bad_options = OpenStreetMapImageryProviderOptions { minimum_level: Some(3), ..Default::default() };
                match OpenStreetMapImageryProvider::new(ImageryLoader::new(c.clone(), host.clone()), bad_options) {
                    Ok(_) => Some(Out::Ok(Value::Null)),
                    Err(e) => Some(Out::Err(e.message)),
                }
            }
            _ => None,
        },
    );
}

#[test]
fn mapbox_imagery_provider() {
    let c = client(Rc::default(), false);
    let host = Rc::new(Images { log: Rc::default() });

    run(
        "scene",
        "MapboxImageryProvider",
        |_| EXACT,
        |f, a| match f {
            "metadata" => {
                let name = s(&a[0]);
                let options = match name {
                    "defaults" => MapboxImageryProviderOptions::new("mapbox.streets", "token123"),
                    "custom" => {
                        let mut opt = MapboxImageryProviderOptions::new("custom.map", "token456");
                        opt.url = Some(Resource::new("https://custom.mapbox.com/v4/").unwrap());
                        opt.format = Some(".jpg".into());
                        opt.minimum_level = Some(1);
                        opt.maximum_level = Some(10);
                        opt.credit = Some(Credit::new("Custom Mapbox Credit", true));
                        opt
                    }
                    _ => panic!("unknown Mapbox case"),
                };
                let p = MapboxImageryProvider::new(ImageryLoader::new(c.clone(), host.clone()), options).unwrap();
                let actual = json!({
                    "url": p.url(),
                    "pickFeaturesUrl": Value::Null,
                    "tileWidth": p.tile_width(),
                    "tileHeight": p.tile_height(),
                    "minimumLevel": p.minimum_level(),
                    "maximumLevel": p.maximum_level(),
                    "rectangle": rect_json(p.rectangle()),
                    "tilingScheme": "WebMercatorTilingScheme",
                    "tilingSchemeRectangle": rect_json(p.tiling_scheme().rectangle()),
                    "hasAlphaChannel": p.has_alpha_channel(),
                    "credit": p.credit().map(Credit::html),
                    "discardPolicy": Value::Null,
                    "tileCredits": Value::Null,
                    "proxy": Value::Null,
                });
                Some(Out::Ok(actual))
            }
            "tileUrls" => {
                let name = s(&a[0]);
                let options = match name {
                    "defaults" => MapboxImageryProviderOptions::new("mapbox.streets", "token123"),
                    "custom" => {
                        let mut opt = MapboxImageryProviderOptions::new("custom.map", "token456");
                        opt.url = Some(Resource::new("https://custom.mapbox.com/v4/").unwrap());
                        opt.format = Some(".jpg".into());
                        opt.minimum_level = Some(1);
                        opt.maximum_level = Some(10);
                        opt.credit = Some(Credit::new("Custom Mapbox Credit", true));
                        opt
                    }
                    _ => panic!("unknown Mapbox case"),
                };
                let p = MapboxImageryProvider::new(ImageryLoader::new(c.clone(), host.clone()), options).unwrap();
                Some(Out::Ok(json!(vec![p.tile_url(2, 3, 4)])))
            }
            _ => None,
        },
    );
}

#[test]
fn mapbox_style_imagery_provider() {
    let c = client(Rc::default(), false);
    let host = Rc::new(Images { log: Rc::default() });

    run(
        "scene",
        "MapboxStyleImageryProvider",
        |_| EXACT,
        |f, a| match f {
            "metadata" => {
                let name = s(&a[0]);
                let options = match name {
                    "defaults" => MapboxStyleImageryProviderOptions::new("streets-v11", "tok-style"),
                    "custom" => {
                        let mut opt = MapboxStyleImageryProviderOptions::new("outdoors-v11", "tok-custom");
                        opt.url = Some(Resource::new("https://custom.style.com/v1/").unwrap());
                        opt.username = Some("custom-user".into());
                        opt.tilesize = Some(256);
                        opt.scale_factor = true;
                        opt.minimum_level = Some(2);
                        opt.maximum_level = Some(15);
                        opt.credit = Some(Credit::new("Custom Style Credit", true));
                        opt
                    }
                    _ => panic!("unknown MapboxStyle case"),
                };
                let p = MapboxStyleImageryProvider::new(ImageryLoader::new(c.clone(), host.clone()), options).unwrap();
                let actual = json!({
                    "url": p.url(),
                    "pickFeaturesUrl": Value::Null,
                    "tileWidth": p.tile_width(),
                    "tileHeight": p.tile_height(),
                    "minimumLevel": p.minimum_level(),
                    "maximumLevel": p.maximum_level(),
                    "rectangle": rect_json(p.rectangle()),
                    "tilingScheme": "WebMercatorTilingScheme",
                    "tilingSchemeRectangle": rect_json(p.tiling_scheme().rectangle()),
                    "hasAlphaChannel": p.has_alpha_channel(),
                    "credit": p.credit().map(Credit::html),
                    "discardPolicy": Value::Null,
                    "tileCredits": Value::Null,
                    "proxy": Value::Null,
                });
                Some(Out::Ok(actual))
            }
            "tileUrls" => {
                let name = s(&a[0]);
                let options = match name {
                    "defaults" => MapboxStyleImageryProviderOptions::new("streets-v11", "tok-style"),
                    "custom" => {
                        let mut opt = MapboxStyleImageryProviderOptions::new("outdoors-v11", "tok-custom");
                        opt.url = Some(Resource::new("https://custom.style.com/v1/").unwrap());
                        opt.username = Some("custom-user".into());
                        opt.tilesize = Some(256);
                        opt.scale_factor = true;
                        opt.minimum_level = Some(2);
                        opt.maximum_level = Some(15);
                        opt.credit = Some(Credit::new("Custom Style Credit", true));
                        opt
                    }
                    _ => panic!("unknown MapboxStyle case"),
                };
                let p = MapboxStyleImageryProvider::new(ImageryLoader::new(c.clone(), host.clone()), options).unwrap();
                Some(Out::Ok(json!(vec![p.tile_url(2, 3, 4)])))
            }
            _ => None,
        },
    );
}

#[test]
fn single_tile_imagery_provider() {
    let c = client(Rc::default(), false);
    let host = Rc::new(Images { log: Rc::default() });

    run(
        "scene",
        "SingleTileImageryProvider",
        |_| EXACT,
        |f, _| match f {
            "metadata" => {
                let options = SingleTileImageryProviderOptions {
                    url: Resource::new("https://example.com/single.png").unwrap(),
                    tile_width: 512.0,
                    tile_height: 256.0,
                    rectangle: None,
                    credit: Some(Credit::new("Single Tile Credit", true)),
                    ellipsoid: None,
                };
                let p = SingleTileImageryProvider::new(ImageryLoader::new(c.clone(), host.clone()), options);
                let actual = json!({
                    "url": p.url(),
                    "pickFeaturesUrl": Value::Null,
                    "tileWidth": p.tile_width(),
                    "tileHeight": p.tile_height(),
                    "minimumLevel": p.minimum_level(),
                    "maximumLevel": p.maximum_level(),
                    "rectangle": rect_json(p.rectangle()),
                    "tilingScheme": "GeographicTilingScheme",
                    "tilingSchemeRectangle": rect_json(p.tiling_scheme().rectangle()),
                    "hasAlphaChannel": p.has_alpha_channel(),
                    "credit": p.credit().map(Credit::html),
                    "discardPolicy": Value::Null,
                    "tileCredits": Value::Null,
                    "proxy": Value::Null,
                });
                Some(Out::Ok(actual))
            }
            "tileUrls" => {
                let options = SingleTileImageryProviderOptions {
                    url: Resource::new("https://example.com/single.png").unwrap(),
                    tile_width: 512.0,
                    tile_height: 256.0,
                    rectangle: None,
                    credit: Some(Credit::new("Single Tile Credit", true)),
                    ellipsoid: None,
                };
                let p = SingleTileImageryProvider::new(ImageryLoader::new(c.clone(), host.clone()), options);
                Some(Out::Ok(json!(vec![p.url()])))
            }
            _ => None,
        },
    );
}

#[test]
fn discard_missing_tile_image_policy_compares_checked_pixels() {
    let host = Rc::new(Images { log: Rc::default() });
    let missing = RgbaImage::new(2, 2, [10, 20, 30, 255].repeat(4)).unwrap();
    let options = DiscardMissingTileImagePolicyOptions {
        pixels_to_check: vec![Cartesian2::new(0.0, 0.0), Cartesian2::new(1.0, 1.0)],
        disable_check_if_all_pixels_are_transparent: false,
    };
    let policy = DiscardMissingTileImagePolicy::new(host.clone(), options.clone(), Some(&missing));
    assert!(policy.is_ready());
    assert!(policy.is_enabled());
    assert!(policy.should_discard_image(&missing));

    let mut different = missing.pixels.clone();
    different[7] = 0;
    assert!(!policy.should_discard_image(&RgbaImage::new(2, 2, different).unwrap()));

    // A failed missing-image load disables the check.
    let failed = DiscardMissingTileImagePolicy::new(host.clone(), options.clone(), None);
    assert!(!failed.is_enabled());
    assert!(!failed.should_discard_image(&missing));

    // All checked pixels transparent: Cesium disables the check.
    let transparent = RgbaImage::new(2, 2, [0, 0, 0, 0].repeat(4)).unwrap();
    let transparent_options =
        DiscardMissingTileImagePolicyOptions { disable_check_if_all_pixels_are_transparent: true, ..options };
    let disabled = DiscardMissingTileImagePolicy::new(host, transparent_options, Some(&transparent));
    assert!(!disabled.is_enabled());
    assert!(!disabled.should_discard_image(&transparent));
}

#[test]
fn url_template_imagery_provider_requests_and_decodes_images() {
    let log = Rc::new(RefCell::new(vec![]));
    let provider = provider(client(log.clone(), true), url_options("https://example.com/{z}/{x}/{y}.png"));
    let image = ready(provider.request_image(3, 5, 4).unwrap()).unwrap();
    assert_eq!(log.borrow().as_slice(), ["https://example.com/4/3/5.png"]);
    match image {
        ImageryImage::Image(resource) => {
            let raster = resource.get::<RgbaImage>().unwrap();
            assert_eq!((raster.width, raster.height), (2, 2));
        }
        other => panic!("expected a decoded image, got {other:?}"),
    }
}

#[test]
fn single_tile_imagery_provider_request_and_caching() {
    let host = Rc::new(Images { log: Rc::default() });
    let log = Rc::new(RefCell::new(vec![]));
    let client_img = client(log.clone(), true);
    let options = SingleTileImageryProviderOptions {
        url: Resource::new("https://example.com/single.png").unwrap(),
        tile_width: 512.0,
        tile_height: 256.0,
        rectangle: None,
        credit: Some(Credit::new("Single Tile Credit", true)),
        ellipsoid: None,
    };
    let p = SingleTileImageryProvider::new(ImageryLoader::new(client_img, host.clone()), options);
    let _img = ready(p.request_image(0, 0, 0).unwrap()).unwrap();
    assert_eq!(log.borrow().as_slice(), ["https://example.com/single.png"]);
    // requesting outside level 0 or x!=0 / y!=0 yields None
    assert!(p.request_image(1, 0, 0).is_none());
}
