//! Dedicated lifecycle, request state, throttling, retry, and ancestor fallback
//! tests for Scene `ImageryLayer`, `Imagery`, and `TileImagery`.
#![allow(clippy::arc_with_non_send_sync)] // Providers use single-threaded shared state.

use cesium_rs::core::*;
use cesium_rs::scene::imagery_provider::{
    ImageryErrorEvent, ImageryFuture, ImageryImage, ImageryProvider,
};
use cesium_rs::scene::*;
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;

struct MockImageryProvider {
    scheme: GeographicTilingScheme,
    error_event: ImageryErrorEvent,
    should_throttle: Arc<AtomicBool>,
    should_fail: Arc<AtomicBool>,
    request_count: Arc<AtomicUsize>,
    discard_policy: Option<Rc<dyn TileDiscardPolicy<ImageryImage>>>,
}

impl MockImageryProvider {
    fn new() -> Self {
        Self {
            scheme: GeographicTilingScheme::default(),
            error_event: Rc::new(RefCell::new(MutableErrorEvent::new())),
            should_throttle: Arc::new(AtomicBool::new(false)),
            should_fail: Arc::new(AtomicBool::new(false)),
            request_count: Arc::new(AtomicUsize::new(0)),
            discard_policy: None,
        }
    }
}

impl ImageryProvider for MockImageryProvider {
    fn rectangle(&self) -> &Rectangle {
        self.scheme.rectangle()
    }
    fn tile_width(&self) -> f64 {
        256.0
    }
    fn tile_height(&self) -> f64 {
        256.0
    }
    fn maximum_level(&self) -> Option<u32> {
        Some(10)
    }
    fn tiling_scheme(&self) -> &dyn TilingScheme {
        &self.scheme
    }
    fn error_event(&self) -> &ImageryErrorEvent {
        &self.error_event
    }
    fn tile_discard_policy(&self) -> Option<Rc<dyn TileDiscardPolicy<ImageryImage>>> {
        self.discard_policy.clone()
    }
    fn get_tile_credits(&self, x: i32, y: i32, level: u32) -> Option<Vec<Credit>> {
        Some(vec![Credit::new(
            format!("Tile-Credit-{x}-{y}-{level}"),
            false,
        )])
    }
    fn request_image(&self, _x: i32, _y: i32, _level: u32) -> Option<ImageryFuture> {
        self.request_count.fetch_add(1, Ordering::SeqCst);
        if self.should_throttle.load(Ordering::SeqCst) {
            return None;
        }
        let fail = self.should_fail.load(Ordering::SeqCst);
        Some(Box::pin(std::future::ready(if fail {
            Err(RuntimeError::new("Network timeout"))
        } else {
            let resource = FrameResource::new(vec![255u8; 256 * 256 * 4]);
            Ok(ImageryImage::Image(resource))
        })))
    }
}

struct MockDiscardPolicy {
    is_ready: Cell<bool>,
    should_discard: Cell<bool>,
}

impl TileDiscardPolicy<ImageryImage> for MockDiscardPolicy {
    fn is_ready(&self) -> bool {
        self.is_ready.get()
    }
    fn should_discard_image(&self, _image: &ImageryImage) -> bool {
        self.should_discard.get()
    }
}

#[test]
fn test_asynchronous_request_lifecycle_and_credits() {
    let provider = Arc::new(MockImageryProvider::new());
    let mut layer = ImageryLayer::new(Some(provider), ImageryLayerOptions::default());

    let imagery_cell = layer.get_imagery_from_cache(1, 1, 1, None);
    assert_eq!(imagery_cell.borrow().state, ImageryState::Unloaded);
    assert_eq!(imagery_cell.borrow().reference_count, 1);
    assert!(imagery_cell.borrow().credits.is_none());
    assert!(imagery_cell.borrow().image.is_none());

    // Process state machine without skip_loading: requests tile
    layer.process_imagery_state_machine(&imagery_cell, None, false, false);

    // Because std::future::ready resolves immediately on poll, state advances to Received
    assert_eq!(imagery_cell.borrow().state, ImageryState::Received);
    assert!(imagery_cell.borrow().image.is_some());
    assert!(imagery_cell.borrow().credits.is_some());
    let credits = imagery_cell.borrow().credits.clone().unwrap();
    assert_eq!(credits[0].html(), "Tile-Credit-1-1-1");
    assert!(!imagery_cell.borrow().has_request());
}

#[test]
fn test_request_throttling_postpones_loading() {
    let provider = Arc::new(MockImageryProvider::new());
    provider.should_throttle.store(true, Ordering::SeqCst);

    let mut layer = ImageryLayer::new(Some(provider.clone()), ImageryLayerOptions::default());
    let imagery_cell = layer.get_imagery_from_cache(0, 0, 0, None);

    // Process state machine: request_image returns None
    layer.process_imagery_state_machine(&imagery_cell, None, false, false);
    assert_eq!(imagery_cell.borrow().state, ImageryState::Unloaded);
    assert!(!imagery_cell.borrow().has_request());

    // Once capacity is restored, next state machine invocation dispatches
    provider.should_throttle.store(false, Ordering::SeqCst);
    layer.process_imagery_state_machine(&imagery_cell, None, false, false);
    assert_eq!(imagery_cell.borrow().state, ImageryState::Received);
    assert!(imagery_cell.borrow().image.is_some());
}

#[test]
fn test_request_failure_error_reporting_and_retry() {
    let provider = Arc::new(MockImageryProvider::new());
    provider.should_fail.store(true, Ordering::SeqCst);

    let retry_attempted = Arc::new(AtomicBool::new(false));
    let retry_clone = retry_attempted.clone();
    let should_fail_flag = provider.should_fail.clone();

    // Register error listener on provider that requests a retry once
    provider.error_event.borrow_mut().add_event_listener(move |err| {
        assert_eq!(err.x, Some(3));
        assert_eq!(err.y, Some(2));
        assert_eq!(err.level, Some(2));
        assert!(err.message.contains("Failed to obtain image tile X: 3 Y: 2 Level: 2"));

        if err.times_retried == 0 {
            err.retry = true;
            retry_clone.store(true, Ordering::SeqCst);
            should_fail_flag.store(false, Ordering::SeqCst);
        }
    });

    let mut layer = ImageryLayer::new(Some(provider), ImageryLayerOptions::default());
    let imagery_cell = layer.get_imagery_from_cache(3, 2, 2, None);

    // Initial state machine step fails, fires event, sets retry=true, and re-requests
    layer.process_imagery_state_machine(&imagery_cell, None, false, false);

    assert!(retry_attempted.load(Ordering::SeqCst));
    // Immediately after retry is dispatched, state is Transitioning
    assert_eq!(imagery_cell.borrow().state, ImageryState::Transitioning);

    // Next state machine step polls the retried request, which now succeeds
    layer.process_imagery_state_machine(&imagery_cell, None, false, false);
    assert_eq!(imagery_cell.borrow().state, ImageryState::Received);
    assert!(imagery_cell.borrow().image.is_some());
    // Error tracking recorded success (times_retried reset to -1)
    assert_eq!(layer.request_image_error().unwrap().times_retried, -1);
}

#[test]
fn test_tile_discard_policy_lifecycle() {
    let discard_policy = Rc::new(MockDiscardPolicy {
        is_ready: Cell::new(false),
        should_discard: Cell::new(false),
    });

    let mut provider_instance = MockImageryProvider::new();
    provider_instance.discard_policy = Some(discard_policy.clone());
    let provider = Arc::new(provider_instance);

    let mut layer = ImageryLayer::new(Some(provider), ImageryLayerOptions::default());
    let imagery_cell = layer.get_imagery_from_cache(0, 0, 0, None);

    // Step 1: Discard policy not ready -> Imagery received image, stays Received
    layer.process_imagery_state_machine(&imagery_cell, None, false, false);
    assert_eq!(imagery_cell.borrow().state, ImageryState::Received);
    assert!(imagery_cell.borrow().image.is_some());

    // Step 2: Policy becomes ready, but decides to discard -> Marks Invalid and frees image
    discard_policy.is_ready.set(true);
    discard_policy.should_discard.set(true);
    layer.process_imagery_state_machine(&imagery_cell, None, false, false);
    assert_eq!(imagery_cell.borrow().state, ImageryState::Invalid);
    assert!(imagery_cell.borrow().image.is_none());
}

#[test]
fn test_cancellation_and_cleanup() {
    let provider = Arc::new(MockImageryProvider::new());
    let mut layer = ImageryLayer::new(Some(provider), ImageryLayerOptions::default());

    let child = layer.get_imagery_from_cache(2, 2, 2, None);
    let parent = child.borrow().parent.clone().unwrap();

    assert_eq!(child.borrow().reference_count, 1);
    assert_eq!(parent.borrow().reference_count, 1);

    // Simulate an in-flight future
    child.borrow_mut().state = ImageryState::Transitioning;
    child.borrow_mut().in_flight_request = Some(Box::pin(std::future::pending()));
    assert!(child.borrow().has_request());

    // cancel_request resets transitioning to unloaded and aborts future
    child.borrow_mut().cancel_request();
    assert_eq!(child.borrow().state, ImageryState::Unloaded);
    assert!(!child.borrow().has_request());

    // Releasing child reference drops parent reference
    child.borrow_mut().release_reference();
    assert_eq!(child.borrow().reference_count, 0);
    assert_eq!(parent.borrow().reference_count, 0);
}

#[test]
fn test_tile_imagery_ancestor_fallback_quadtree() {
    let provider = Arc::new(MockImageryProvider::new());
    let mut layer = ImageryLayer::new(Some(provider), ImageryLayerOptions::default());

    // Create a tile imagery at level 2
    let child = layer.get_imagery_from_cache(2, 2, 2, None);
    let parent = child.borrow().parent.clone().unwrap();
    let grandparent = parent.borrow().parent.clone().unwrap();

    let terrain_rect = Rectangle::new(0.0, 0.0, 0.1, 0.1);
    let mut tile_imagery =
        TileImagery::new(child.clone(), Cartesian4::new(0.0, 0.0, 1.0, 1.0), false);

    // Give grandparent a texture and mark it Ready
    grandparent.borrow_mut().texture = Some(FrameResource::new(42u32));
    grandparent.borrow_mut().state = ImageryState::Ready;

    // Process state machine with skip_loading: child has not loaded yet,
    // so tile_imagery should fall back to using grandparent!
    let done = tile_imagery.process_state_machine(&mut layer, &terrain_rect, None, true);
    assert!(!done); // child is not done loading

    assert!(tile_imagery.has_ready_imagery());
    let ready_img = tile_imagery.ready_imagery.clone().unwrap();
    assert_eq!(ready_img.borrow().level, 0); // Grandparent is level 0
    assert!(tile_imagery.texture_translation_and_scale.is_some());

    // Now complete the child tile
    child.borrow_mut().texture = Some(FrameResource::new(99u32));
    child.borrow_mut().state = ImageryState::Ready;

    let done = tile_imagery.process_state_machine(&mut layer, &terrain_rect, None, false);
    assert!(done);
    assert!(tile_imagery.is_done_loading());
    let ready_img = tile_imagery.ready_imagery.clone().unwrap();
    assert_eq!(ready_img.borrow().level, 2); // Child is level 2
}

#[test]
fn test_tile_imagery_ancestor_fallback_when_child_fails() {
    let provider = Arc::new(MockImageryProvider::new());
    let mut layer = ImageryLayer::new(Some(provider), ImageryLayerOptions::default());

    let child = layer.get_imagery_from_cache(2, 2, 2, None);
    let parent = child.borrow().parent.clone().unwrap();
    let grandparent = parent.borrow().parent.clone().unwrap();

    let terrain_rect = Rectangle::new(0.0, 0.0, 0.1, 0.1);
    let mut tile_imagery =
        TileImagery::new(child.clone(), Cartesian4::new(0.0, 0.0, 1.0, 1.0), false);

    grandparent.borrow_mut().texture = Some(FrameResource::new(101u32));
    grandparent.borrow_mut().state = ImageryState::Ready;

    // Mark child and parent as Failed
    child.borrow_mut().state = ImageryState::Failed;
    parent.borrow_mut().state = ImageryState::Failed;

    let done = tile_imagery.process_state_machine(&mut layer, &terrain_rect, None, false);
    // Since child and parent failed and grandparent is ready, we have the best substitute
    assert!(done);
    let ready_img = tile_imagery.ready_imagery.clone().unwrap();
    assert_eq!(ready_img.borrow().level, 0);
}
