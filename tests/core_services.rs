mod common;
use cesium_rs::core::geocoder_service::*;
use cesium_rs::core::sample_terrain::*;
use cesium_rs::core::*;
use common::*;
use serde_json::{json, Value};
use std::{
    cell::{Cell, RefCell},
    future::{ready, Future, Ready},
    pin::pin,
    sync::{Arc, Mutex},
    task::{Context, Poll, Waker},
};
fn complete<T>(f: impl Future<Output = T>) -> T {
    let mut f = pin!(f);
    for _ in 0..100 {
        if let Poll::Ready(v) = f.as_mut().poll(&mut Context::from_waker(Waker::noop())) {
            return v;
        }
    }
    panic!("fixture did not complete")
}
struct JsonHost {
    response: Value,
    request: RefCell<Value>,
}
impl GeocoderHost for JsonHost {
    fn fetch_json<'a>(&'a self, r: Resource, p: Option<&'static str>) -> GeocoderFuture<'a, Value> {
        *self.request.borrow_mut() = json!([r.url(), p]);
        Box::pin(async { Ok(self.response.clone()) })
    }
}
fn geocode(name: &str, a: &[Value]) -> Option<Out> {
    let host = JsonHost { response: a[0].clone(), request: RefCell::new(Value::Null) };
    let service: Box<dyn GeocoderService> = match name {
        "bing" => Box::new(BingMapsGeocoderService::new("test key", Some("fr-FR"))),
        "google" => Box::new(GoogleGeocoderService::new("test key")),
        "pelias" => Box::new(PeliasGeocoderService::new(Resource::new("https://geo.test/v1?token=a").unwrap())),
        _ => Box::new(OpenCageGeocoderService::new(
            Resource::new("https://geo.test/v1?token=a").unwrap(),
            "test key",
            vec![("language".into(), Some(cesium_rs::core::query::QueryValue::Single("fr".into())))],
        )),
    };
    let kind = GeocodeType::from_value(u(&a[1]) as u8).unwrap();
    match complete(service.geocode("New York & Paris", kind, &host)) {
        Err(e) => Some(Out::Err(e.to_string())),
        Ok(results) => {
            let credit = service.credit().map(|c| json!([c.html(), c.show_on_screen]));
            let attrs = |a: &Attribution| {
                let mut v = json!({"html":a.html});
                if let Some(c) = a.collapsible {
                    v["collapsible"] = json!(c);
                }
                v
            };
            ok(json!([
                *host.request.borrow(),
                credit,
                results
                    .iter()
                    .map(|r| json!([
                        r.display_name,
                        match r.destination {
                            GeocoderDestination::Point(p) => p.enc(),
                            GeocoderDestination::Rectangle(r) => r.enc(),
                        },
                        r.attributions.as_ref().map(|a| a.iter().map(attrs).collect::<Vec<_>>()),
                        r.attribution.as_ref().map(attrs)
                    ]))
                    .collect::<Vec<_>>()
            ]))
        }
    }
}
#[test]
fn bing() {
    run("core", "BingMapsGeocoderService", |_| TIGHT, |_, a| geocode("bing", a));
}
#[test]
fn google() {
    run("core", "GoogleGeocoderService", |_| TIGHT, |_, a| geocode("google", a));
}
#[test]
fn pelias() {
    run("core", "PeliasGeocoderService", |_| TIGHT, |_, a| geocode("pelias", a));
}
#[test]
fn opencage() {
    run("core", "OpenCageGeocoderService", |_| TIGHT, |_, a| geocode("opencage", a));
}
#[test]
fn tile_errors() {
    run(
        "core",
        "TileProviderError",
        |_| EXACT,
        |_, a| {
            let mut previous: Option<TileProviderError<(), String>> = None;
            let mut seen = vec![];
            let states: Vec<_> = a[0]
                .as_array()
                .unwrap()
                .iter()
                .enumerate()
                .map(|(i, op)| {
                    if s(op) == "success" {
                        previous.as_mut().unwrap().report_success();
                    } else {
                        previous = Some(TileProviderError::report_error(
                            previous.take(),
                            TileProviderErrorOptions {
                                provider: (),
                                message: format!("error-{i}"),
                                x: Some(i as u32),
                                y: Some(i as u32 + 1),
                                level: Some(3),
                                error: Some("details".into()),
                            },
                            |e| {
                                seen.push(json!([e.message, e.times_retried, e.retry, e.x, e.y, e.level]));
                                e.retry = e.times_retried < 2;
                            },
                        ));
                    }
                    let e = previous.as_ref().unwrap();
                    json!([e.times_retried, e.retry])
                })
                .collect();
            ok(json!([seen, states]))
        },
    );
}
struct Video {
    ready: u8,
    paused: bool,
    time: f64,
    looping: bool,
    calls: Vec<Value>,
}
impl VideoElement for Video {
    fn ready_state(&self) -> u8 {
        self.ready
    }
    fn paused(&self) -> bool {
        self.paused
    }
    fn current_time(&self) -> f64 {
        self.time
    }
    fn duration(&self) -> f64 {
        20.
    }
    fn looping(&self) -> bool {
        self.looping
    }
    fn play(&mut self) {
        self.paused = false;
        self.calls.push(json!("play"));
    }
    fn pause(&mut self) {
        self.paused = true;
        self.calls.push(json!("pause"));
    }
    fn set_playback_rate(&mut self, v: f64) -> Result<(), RuntimeError> {
        self.calls.push(json!(["rate", v]));
        if v < 0. {
            Err(RuntimeError::new("unsupported"))
        } else {
            Ok(())
        }
    }
    fn set_current_time(&mut self, v: f64) {
        self.time = v;
    }
}
#[test]
fn video() {
    run(
        "core",
        "VideoSynchronizer",
        |_| EXACT,
        |_, a| {
            let epoch = JulianDate::from_iso8601("2020-01-01T00:00:00Z").unwrap();
            let v = Video {
                ready: u(&a[1]) as u8,
                paused: true,
                time: 0.,
                looping: a[0].as_bool().unwrap(),
                calls: vec![],
            };
            let mut sync = VideoSynchronizer::new(Some(v), Some(epoch), Some(0.5));
            ok(json!(a[2]
                .as_array()
                .unwrap()
                .iter()
                .map(|t| {
                    if t[3].as_bool().unwrap() {
                        sync.on_seeked();
                    }
                    sync.on_tick_state(&cesium_rs::core::clock::ClockTick {
                        current_time: epoch.add_seconds(n(&t[1])),
                        multiplier: n(&t[2]),
                        should_animate: t[0].as_bool().unwrap(),
                    });
                    let el = sync.element_mut().unwrap();
                    json!([el.time, el.paused, std::mem::take(&mut el.calls)])
                })
                .collect::<Vec<_>>()))
        },
    );
}
struct Provider {
    scheme: GeographicTilingScheme,
    availability: TileAvailability,
    calls: RefCell<Vec<[u32; 3]>>,
    fail: bool,
    mode: String,
    throttled: Cell<bool>,
}
impl Provider {
    fn new(fail: bool, mode: &str) -> Self {
        let a = TileAvailability::new(GeographicTilingScheme::default(), 2);
        a.add_available_tile_range(0, 0, 0, 1, 0);
        if mode != "metadata" {
            a.add_available_tile_range(2, 0, 0, 7, 3);
        }
        Self {
            scheme: Default::default(),
            availability: a,
            calls: Default::default(),
            fail,
            mode: mode.into(),
            throttled: Cell::new(false),
        }
    }
}
impl TerrainProvider for Provider {
    type Error = RuntimeError;
    type Credit = String;
    type TerrainData = HeightmapTerrainData;
    type Request = ();
    type GeometryFuture<'a> = Ready<Result<HeightmapTerrainData, RuntimeError>>;
    type AvailabilityFuture<'a> = Ready<Result<(), RuntimeError>>;
    fn error_event(&self) -> Arc<Mutex<Event<RuntimeError>>> {
        Arc::new(Mutex::new(Event::new()))
    }
    fn credit(&self) -> Option<&String> {
        None
    }
    fn tiling_scheme(&self) -> &dyn TilingScheme {
        &self.scheme
    }
    fn has_water_mask(&self) -> bool {
        false
    }
    fn has_vertex_normals(&self) -> bool {
        false
    }
    fn availability(&self) -> Option<&TileAvailability> {
        Some(&self.availability)
    }
    fn request_tile_geometry(&self, x: u32, y: u32, level: u32, _: Option<()>) -> Option<Self::GeometryFuture<'_>> {
        self.calls.borrow_mut().push([x, y, level]);
        if self.mode == "throttled" && !self.throttled.replace(true) {
            return None;
        }
        Some(ready(if self.fail && x.is_multiple_of(2) {
            Err(RuntimeError::new("tile failed"))
        } else {
            Ok(HeightmapTerrainData::new(HeightmapTerrainDataOptions::new(
                TypedArray::Float32([10., 20., 30., 40.].map(|v| v + level as f32 * 100.).to_vec()),
                2,
                2,
            ))
            .unwrap())
        }))
    }
    fn get_level_maximum_geometric_error(&self, _: u32) -> f64 {
        0.
    }
    fn get_tile_data_available(&self, _: u32, _: u32, _: u32) -> Option<bool> {
        None
    }
    fn load_tile_data_availability(&self, _: u32, _: u32, _: u32) -> Option<Self::AvailabilityFuture<'_>> {
        if self.mode == "metadata" {
            self.availability.add_available_tile_range(2, 0, 0, 7, 3);
            Some(ready(Ok(())))
        } else {
            None
        }
    }
}
struct Timer;
impl TerrainSamplingHost for Timer {
    type Delay<'a> = Ready<()>;
    fn delay(&self, _: u32) -> Ready<()> {
        ready(())
    }
}
fn sample(most: bool, a: &[Value]) -> Option<Out> {
    let p = Provider::new(a[0].as_bool().unwrap(), s(&a[2]));
    let mut points: Vec<_> = [(-160., 20.), (-150., 25.), (10., 20.), (170., -20.)]
        .map(|(x, y)| Cartographic::from_degrees(x, y, 99.))
        .to_vec();
    let reject = a[1].as_bool().unwrap();
    let result = if most {
        complete(sample_terrain_most_detailed(&p, &mut points, reject, &Timer))
    } else {
        complete(sample_terrain(
            &p,
            &mut points,
            SampleTerrainOptions { level: 2, reject_on_tile_fail: reject },
            &Timer,
        ))
    };
    let calls = p.calls.borrow().clone();
    ok(json!([points.iter().map(|p| num(p.height)).collect::<Vec<_>>(), calls, result.is_err()]))
}
#[test]
fn terrain() {
    run("core", "sampleTerrain", |_| TIGHT, |_, a| sample(false, a));
}
#[test]
fn terrain_most_detailed() {
    run("core", "sampleTerrainMostDetailed", |_| TIGHT, |_, a| sample(true, a));
}
