//! Dedicated integration tests for CesiumJS WMTS time-dynamic preload and cache behavior.
#![allow(clippy::type_complexity)]

use cesium_rs::core::clock::ClockOptions;
use cesium_rs::core::image_helpers::{ImageDecodeOptions, ImageFuture};
use cesium_rs::core::{
    Clock, HttpRequest, HttpResponse, ImageHost, JulianDate, RequestErrorEvent, RequestFuture, Resource,
    ResourceClient, ResourceTransport, RgbaImage, RuntimeError, TimeInterval, TimeIntervalCollection,
};
use cesium_rs::scene::web_map_tile_service_imagery_provider::{
    DynamicData, WebMapTileServiceImageryProvider, WebMapTileServiceImageryProviderOptions,
};
use cesium_rs::scene::{ImageryImage, ImageryLoader, ImageryProvider};
use std::cell::RefCell;
use std::future::Future;
use std::rc::Rc;
use std::task::{Context as TaskContext, Poll, Waker};

fn ready<T>(future: impl Future<Output = T>) -> T {
    match std::pin::pin!(future).poll(&mut TaskContext::from_waker(Waker::noop())) {
        Poll::Ready(value) => value,
        Poll::Pending => panic!("unexpected pending request"),
    }
}

#[derive(Clone)]
struct TestImageHost {
    log: Rc<RefCell<Vec<String>>>,
}

impl ImageHost for TestImageHost {
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
    Ok(HttpResponse {
        status: 200,
        body: body.into(),
        headers: vec![("Content-Type".into(), content_type.into())],
    })
}

fn setup_loader(
    handler: impl Fn(&HttpRequest) -> Result<HttpResponse, RequestErrorEvent> + 'static,
) -> (Rc<ResourceClient>, Rc<TestImageHost>, Rc<RefCell<Vec<String>>>, ImageryLoader<TestImageHost>) {
    let log = Rc::new(RefCell::new(vec![]));
    let host = Rc::new(TestImageHost { log: log.clone() });
    let client = Rc::new(ResourceClient::new(Rc::new(ScriptedTransport {
        log: log.clone(),
        handler: Rc::new(handler),
    })));
    let loader = ImageryLoader::new(client.clone(), host.clone());
    (client, host, log, loader)
}

fn default_loader() -> (Rc<ResourceClient>, Rc<TestImageHost>, Rc<RefCell<Vec<String>>>, ImageryLoader<TestImageHost>) {
    setup_loader(|_| response(vec![0u8; 16], "image/png"))
}

fn test_three_interval_times() -> TimeIntervalCollection<DynamicData> {
    let base = JulianDate::default();
    let mut times = TimeIntervalCollection::new();
    times.add_interval(TimeInterval::new(
        base.add_seconds(0.0),
        base.add_seconds(10.0),
        true,
        false,
        Some(vec![("Time".into(), "2024-01-01".into())]),
    ));
    times.add_interval(TimeInterval::new(
        base.add_seconds(10.0),
        base.add_seconds(20.0),
        true,
        false,
        Some(vec![("Time".into(), "2024-01-02".into())]),
    ));
    times.add_interval(TimeInterval::new(
        base.add_seconds(20.0),
        base.add_seconds(30.0),
        true,
        false,
        Some(vec![("Time".into(), "2024-01-03".into())]),
    ));
    times
}

fn animating_clock(start_secs: f64, current_secs: f64, multiplier: f64) -> Rc<RefCell<Clock>> {
    let base = JulianDate::default();
    Rc::new(RefCell::new(
        Clock::new(ClockOptions {
            start_time: Some(base.add_seconds(start_secs)),
            stop_time: Some(base.add_seconds(30.0)),
            current_time: Some(base.add_seconds(current_secs)),
            multiplier: Some(multiplier),
            can_animate: Some(true),
            should_animate: Some(true),
            ..Default::default()
        })
        .unwrap(),
    ))
}

#[test]
fn wmts_preload_when_approaching_interval_and_consume_from_cache() {
    let (_client, _host, log, loader) = default_loader();
    let clock = animating_clock(0.0, 2.0, 1.0);
    let mut options = WebMapTileServiceImageryProviderOptions::new(
        Resource::new("https://wmts.example.com/{Time}/{TileMatrixSet}/{TileMatrix}/{TileRow}/{TileCol}.png").unwrap(),
        "layer",
        "style",
        "matrix",
    );
    options.clock = Some(clock.clone());
    options.times = Some(test_three_interval_times());
    let provider = WebMapTileServiceImageryProvider::new(loader, options).unwrap();

    let reloads = Rc::new(RefCell::new(0));
    let r = reloads.clone();
    provider.set_reload(Some(Rc::new(move || *r.borrow_mut() += 1)));

    // At t = 2.0s, interval 0 [0, 10) is active. Stop is at 10.0s (8.0s away > 5.0s threshold).
    // Request tile (1, 2, 3).
    let img = ready(provider.request_image(1, 2, 3).unwrap()).unwrap();
    assert!(matches!(img, ImageryImage::Image(_)));

    let http_urls: Vec<String> = log.borrow().iter().filter(|u| u.starts_with("http")).cloned().collect();
    assert_eq!(http_urls.len(), 1);
    assert!(http_urls[0].contains("/2024-01-01/"));
    assert_eq!(provider.pending_preload_count(), 1);
    assert_eq!(provider.cached_interval_count(), 0);

    // Advance clock to t = 7.0s (10.0 - 7.0 = 3.0s <= 5.0s, so interval 1 [10, 20) is approaching).
    clock.borrow_mut().set_current_time(JulianDate::default().add_seconds(7.0));
    provider.update_time();

    // The future is cached, but the host transport is not polled in the background.
    assert_eq!(provider.pending_preload_count(), 0);
    assert_eq!(provider.cached_interval_count(), 1);
    assert_eq!(log.borrow().iter().filter(|u| u.starts_with("http")).count(), 1);

    // Advance clock to t = 12.0s (inside interval 1 [10, 20)).
    clock.borrow_mut().set_current_time(JulianDate::default().add_seconds(12.0));

    // Consuming the cached future starts its host request; no second future is created.
    let img2 = ready(provider.request_image(1, 2, 3).unwrap()).unwrap();
    assert!(matches!(img2, ImageryImage::Image(_)));

    let http_urls_after_cache: Vec<String> =
        log.borrow().iter().filter(|u| u.starts_with("http")).cloned().collect();
    assert_eq!(http_urls_after_cache.len(), 2, "Cached future started one request when consumed");
    assert!(http_urls_after_cache[1].contains("/2024-01-02/"));
    assert!(http_urls_after_cache[1].ends_with("/matrix/3/2/1.png"));

    // Cache should now be consumed for that tile. A subsequent request for the same tile in interval 1
    // should now fall back to a fresh network load.
    let img3 = ready(provider.request_image(1, 2, 3).unwrap()).unwrap();
    assert!(matches!(img3, ImageryImage::Image(_)));

    let http_urls_after_second: Vec<String> =
        log.borrow().iter().filter(|u| u.starts_with("http")).cloned().collect();
    assert_eq!(http_urls_after_second.len(), 3, "After cache consumption, second request issues fresh network fetch");
    assert!(http_urls_after_second[2].contains("/2024-01-02/"));

    // Interval transition should have signaled reload.
    assert!(*reloads.borrow() >= 1);
}

#[test]
fn wmts_immediate_preload_when_requesting_tile_in_approaching_window() {
    let (_client, _host, log, loader) = default_loader();
    let clock = animating_clock(0.0, 8.0, 1.0); // 8.0s is 2.0s away from 10.0s boundary (<= 5.0s)
    let mut options = WebMapTileServiceImageryProviderOptions::new(
        Resource::new("https://wmts.example.com/{Time}/{TileMatrixSet}/{TileMatrix}/{TileRow}/{TileCol}.png").unwrap(),
        "layer",
        "style",
        "matrix",
    );
    options.clock = Some(clock.clone());
    options.times = Some(test_three_interval_times());
    let provider = WebMapTileServiceImageryProvider::new(loader, options).unwrap();

    // Requesting tile (4, 5, 6) while already in approaching window:
    // Should immediately load for current interval 0 ("2024-01-01") AND immediately preload for interval 1 ("2024-01-02").
    let img = ready(provider.request_image(4, 5, 6).unwrap()).unwrap();
    assert!(matches!(img, ImageryImage::Image(_)));

    let urls: Vec<String> = log.borrow().iter().filter(|u| u.starts_with("http")).cloned().collect();
    assert_eq!(urls.len(), 2);
    assert!(urls[0].contains("/2024-01-01/"));
    assert!(urls[0].ends_with("/matrix/6/5/4.png"));
    assert!(urls[1].contains("/2024-01-02/"));
    assert!(urls[1].ends_with("/matrix/6/5/4.png"));
    assert_eq!(provider.cached_interval_count(), 1);

    // Now advance time to interval 1 [10, 20) at t = 14.0s.
    clock.borrow_mut().set_current_time(JulianDate::default().add_seconds(14.0));
    let img2 = ready(provider.request_image(4, 5, 6).unwrap()).unwrap();
    assert!(matches!(img2, ImageryImage::Image(_)));

    let urls_after: Vec<String> = log.borrow().iter().filter(|u| u.starts_with("http")).cloned().collect();
    assert_eq!(urls_after.len(), 2, "Consumed preloaded tile from cache");
}

#[test]
fn wmts_cache_eviction_when_moving_past_interval() {
    let (_client, _host, log, loader) = default_loader();
    let clock = animating_clock(0.0, 8.0, 1.0);
    let mut options = WebMapTileServiceImageryProviderOptions::new(
        Resource::new("https://wmts.example.com/{Time}/{TileMatrixSet}/{TileMatrix}/{TileRow}/{TileCol}.png").unwrap(),
        "layer",
        "style",
        "matrix",
    );
    options.clock = Some(clock.clone());
    options.times = Some(test_three_interval_times());
    let provider = WebMapTileServiceImageryProvider::new(loader, options).unwrap();

    // Preload tile into interval 1 while in interval 0
    let _ = ready(provider.request_image(1, 2, 3).unwrap()).unwrap();
    assert_eq!(provider.cached_interval_count(), 1);

    // Step clock into interval 1 at t = 12.0s
    clock.borrow_mut().set_current_time(JulianDate::default().add_seconds(12.0));
    provider.update_time();
    // Cache for interval 1 is now active.
    assert_eq!(provider.current_interval_index(), 1);

    // Now advance time into interval 2 at t = 25.0s [20, 30).
    clock.borrow_mut().set_current_time(JulianDate::default().add_seconds(25.0));
    provider.update_time();

    // Old interval 1 cache should now be evicted.
    assert_eq!(provider.cached_interval_count(), 0);

    // A request in interval 2 should load from network for 2024-01-03.
    let _ = ready(provider.request_image(1, 2, 3).unwrap()).unwrap();
    let urls: Vec<String> = log.borrow().iter().filter(|u| u.starts_with("http")).cloned().collect();
    let last = urls.last().unwrap();
    assert!(last.contains("/2024-01-03/"));
}

#[test]
fn wmts_backwards_animation_preloading() {
    let (_client, _host, log, loader) = default_loader();
    // Clock animating backwards (multiplier: -1.0) starting at t = 12.0s in interval 1 [10, 20).
    // Start of interval 1 is 10.0s. Distance is (10.0 - 12.0) / -1.0 = 2.0s <= 5.0s.
    // Approaching interval is interval 0 [0, 10) ("2024-01-01").
    let clock = animating_clock(0.0, 12.0, -1.0);
    let mut options = WebMapTileServiceImageryProviderOptions::new(
        Resource::new("https://wmts.example.com/{Time}/{TileMatrixSet}/{TileMatrix}/{TileRow}/{TileCol}.png").unwrap(),
        "layer",
        "style",
        "matrix",
    );
    options.clock = Some(clock.clone());
    options.times = Some(test_three_interval_times());
    let provider = WebMapTileServiceImageryProvider::new(loader, options).unwrap();

    let _ = ready(provider.request_image(1, 2, 3).unwrap()).unwrap();

    let urls: Vec<String> = log.borrow().iter().filter(|u| u.starts_with("http")).cloned().collect();
    assert_eq!(urls.len(), 2);
    assert!(urls[0].contains("/2024-01-02/"), "Current interval tile");
    assert!(urls[1].contains("/2024-01-01/"), "Preloaded backward approaching interval tile");
    assert_eq!(provider.cached_interval_count(), 1);

    // Advance clock backwards into interval 0 at t = 8.0s.
    clock.borrow_mut().set_current_time(JulianDate::default().add_seconds(8.0));
    let _ = ready(provider.request_image(1, 2, 3).unwrap()).unwrap();

    let urls_after: Vec<String> = log.borrow().iter().filter(|u| u.starts_with("http")).cloned().collect();
    assert_eq!(urls_after.len(), 2, "Backward preloaded tile was consumed from cache");
}

#[test]
fn wmts_kvp_time_dynamic_preload_and_cache() {
    let (_client, _host, log, loader) = default_loader();
    let clock = animating_clock(0.0, 8.0, 1.0);
    let mut options = WebMapTileServiceImageryProviderOptions::new(
        Resource::new("https://wmts.example.com/service").unwrap(),
        "my_layer",
        "my_style",
        "my_set",
    );
    options.clock = Some(clock.clone());
    options.times = Some(test_three_interval_times());
    let provider = WebMapTileServiceImageryProvider::new(loader, options).unwrap();
    assert!(provider.use_kvp());

    let _ = ready(provider.request_image(1, 2, 3).unwrap()).unwrap();

    let urls: Vec<String> = log.borrow().iter().filter(|u| u.starts_with("http")).cloned().collect();
    assert_eq!(urls.len(), 2);
    assert!(urls[0].contains("Time=2024-01-01"));
    assert!(urls[0].contains("service=WMTS"));
    assert!(urls[1].contains("Time=2024-01-02"));
    assert!(urls[1].contains("service=WMTS"));

    // Advance into interval 1 at t = 13.0s.
    clock.borrow_mut().set_current_time(JulianDate::default().add_seconds(13.0));
    let _ = ready(provider.request_image(1, 2, 3).unwrap()).unwrap();

    let urls_after: Vec<String> = log.borrow().iter().filter(|u| u.starts_with("http")).cloned().collect();
    assert_eq!(urls_after.len(), 2, "KVP tile consumed from cache without network request");
}

#[test]
fn wmts_static_dimensions_combined_with_time_dynamic_and_mutated() {
    let (_client, _host, log, loader) = default_loader();
    let clock = animating_clock(0.0, 8.0, 1.0);
    let mut options = WebMapTileServiceImageryProviderOptions::new(
        Resource::new("https://wmts.example.com/{Elevation}/{Time}/{TileMatrixSet}/{TileMatrix}/{TileRow}/{TileCol}.png")
            .unwrap(),
        "layer",
        "style",
        "matrix",
    );
    options.clock = Some(clock.clone());
    options.times = Some(test_three_interval_times());
    options.dimensions = Some(vec![("Elevation".into(), "500".into())]);
    let mut provider = WebMapTileServiceImageryProvider::new(loader, options).unwrap();

    let reloads = Rc::new(RefCell::new(0));
    let r = reloads.clone();
    provider.set_reload(Some(Rc::new(move || *r.borrow_mut() += 1)));

    let _ = ready(provider.request_image(1, 2, 3).unwrap()).unwrap();
    let urls: Vec<String> = log.borrow().iter().filter(|u| u.starts_with("http")).cloned().collect();
    assert_eq!(urls.len(), 2);
    assert!(urls[0].contains("/500/2024-01-01/"));
    assert!(urls[1].contains("/500/2024-01-02/"));
    // First reload occurred when interval initialized on first request
    assert_eq!(*reloads.borrow(), 1);

    // Mutate static dimensions
    provider.set_dimensions(Some(vec![("Elevation".into(), "1000".into())]));
    assert_eq!(*reloads.borrow(), 2);
    assert_eq!(provider.dimensions(), Some(&[("Elevation".into(), "1000".into())][..]));

    // Request new tile (4, 5, 6)
    let _ = ready(provider.request_image(4, 5, 6).unwrap()).unwrap();
    let urls_after: Vec<String> = log.borrow().iter().filter(|u| u.starts_with("http")).cloned().collect();
    assert_eq!(urls_after.len(), 4);
    assert!(urls_after[2].contains("/1000/2024-01-01/"));
    assert!(urls_after[3].contains("/1000/2024-01-02/"));
}

#[test]
fn wmts_no_preload_when_clock_is_paused_or_stopped() {
    let (_client, _host, log, loader) = default_loader();
    let base = JulianDate::default();
    let clock = Rc::new(RefCell::new(
        Clock::new(ClockOptions {
            start_time: Some(base),
            stop_time: Some(base.add_seconds(30.0)),
            current_time: Some(base.add_seconds(8.0)), // within 5s of boundary, but paused
            multiplier: Some(1.0),
            can_animate: Some(false),
            should_animate: Some(false),
            ..Default::default()
        })
        .unwrap(),
    ));
    let mut options = WebMapTileServiceImageryProviderOptions::new(
        Resource::new("https://wmts.example.com/{Time}/{TileMatrixSet}/{TileMatrix}/{TileRow}/{TileCol}.png").unwrap(),
        "layer",
        "style",
        "matrix",
    );
    options.clock = Some(clock.clone());
    options.times = Some(test_three_interval_times());
    let provider = WebMapTileServiceImageryProvider::new(loader, options).unwrap();

    let _ = ready(provider.request_image(1, 2, 3).unwrap()).unwrap();

    let urls: Vec<String> = log.borrow().iter().filter(|u| u.starts_with("http")).cloned().collect();
    assert_eq!(urls.len(), 1, "Paused clock should not trigger preloading");
    assert_eq!(provider.cached_interval_count(), 0);
    assert_eq!(provider.pending_preload_count(), 1);
    // Keep the tile in the queue while paused so resuming can preload it.
    clock.borrow_mut().can_animate = true;
    clock.borrow_mut().set_should_animate(true);
    provider.update_time();
    assert_eq!(provider.pending_preload_count(), 0);
    assert_eq!(provider.cached_interval_count(), 1);
}

#[test]
fn wmts_set_times_updates_intervals_and_signals_reload() {
    let (_client, _host, _log, loader) = default_loader();
    let clock = animating_clock(0.0, 5.0, 1.0);
    let mut options = WebMapTileServiceImageryProviderOptions::new(
        Resource::new("https://wmts.example.com/{Time}/{TileMatrixSet}/{TileMatrix}/{TileRow}/{TileCol}.png").unwrap(),
        "layer",
        "style",
        "matrix",
    );
    options.clock = Some(clock.clone());
    options.times = Some(test_three_interval_times());
    let mut provider = WebMapTileServiceImageryProvider::new(loader, options).unwrap();

    let reloads = Rc::new(RefCell::new(0));
    let r = reloads.clone();
    provider.set_reload(Some(Rc::new(move || *r.borrow_mut() += 1)));

    // Initialize interval state from clock
    provider.update_time();
    assert_eq!(*reloads.borrow(), 1);
    assert_eq!(provider.current_interval_data().unwrap()[0].1, "2024-01-01");

    // Replace with new interval set
    let base = JulianDate::default();
    let mut new_times = TimeIntervalCollection::new();
    new_times.add_interval(TimeInterval::new(
        base.add_seconds(0.0),
        base.add_seconds(20.0),
        true,
        false,
        Some(vec![("Time".into(), "2025-12-31".into())]),
    ));
    provider.set_times(new_times);

    assert_eq!(*reloads.borrow(), 2);
    assert_eq!(provider.current_interval_data().unwrap()[0].1, "2025-12-31");
}
