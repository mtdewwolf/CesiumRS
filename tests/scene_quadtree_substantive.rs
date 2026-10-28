//! Substantive behavior and regression tests for Scene `QuadtreePrimitive` and `QuadtreeTile`.
//!
//! Covers:
//! 1. Height callback registrations preserve callbacks during distribution, sample tile geometry,
//!    and invoke user callbacks with updated cartographic heights.
//! 2. Height callback removals preserve original Cartographic position (not (0, 0)) and clean up
//!    custom data from the quadtree hierarchy.
//! 3. Cache trimming frees actual tile resources (`free_resources()`) of eligible tiles beyond
//!    cache size, skips ineligible tiles, and handles borrowed descendant safety.
//! 4. Tile load queues retain length until after progress events are raised.
//! 5. Screen-space error evaluates to infinity when distance is zero (parity with CesiumJS).

pub use cesium_rs::core;
pub use cesium_rs::scene;

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use cesium_rs::core::{
    Cartesian3, Cartographic, GeographicTilingScheme, JulianDate, PerspectiveFrustum,
    TimeStandard, TilingScheme, Visibility,
};
use cesium_rs::renderer::UniformFrame;
use cesium_rs::scene::camera::{Camera, CameraFrustum, CameraOptions};
use cesium_rs::scene::frame_state::FrameState;
use cesium_rs::scene::quadtree_occluders::QuadtreeOccluders;
use cesium_rs::scene::quadtree_tile_load_state::QuadtreeTileLoadState;
use cesium_rs::scene::scene_mode::SceneMode;

use cesium_rs::scene::quadtree_tile;
use cesium_rs::scene::quadtree_primitive;

use quadtree_primitive::{
    QuadtreePrimitive, QuadtreePrimitiveOptions, QuadtreeTileProvider,
};
use quadtree_tile::{
    QuadtreeTile, QuadtreeTileData, QuadtreeTileHandle,
};
use serde_json::Value;

/// Custom tile data that can track resource freeing, eligibility, and provide height sampling.
#[derive(Debug)]
struct SubstantiveTileData {
    sample_height_value: f64,
    eligible: bool,
    freed: RefCell<bool>,
    sample_count: RefCell<usize>,
}

impl SubstantiveTileData {
    fn new(sample_height_value: f64, eligible: bool) -> Self {
        Self {
            sample_height_value,
            eligible,
            freed: RefCell::new(false),
            sample_count: RefCell::new(0),
        }
    }
}

impl QuadtreeTileData for SubstantiveTileData {
    fn eligible_for_unloading(&self) -> bool {
        self.eligible
    }

    fn free_resources(&mut self) {
        *self.freed.borrow_mut() = true;
    }

    fn has_mesh(&self) -> bool {
        true
    }

    fn sample_height(&self, _pos: &Cartographic) -> Option<f64> {
        *self.sample_count.borrow_mut() += 1;
        Some(self.sample_height_value)
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}

struct SubstantiveTileProvider {
    scheme: Arc<dyn TilingScheme>,
    max_geometric_error_level_zero: f64,
    tile_height_sample: f64,
    tile_eligible_for_unloading: bool,
    distance_to_tile: f64,
    can_refine_max_level: u32,
    loaded_tiles: RefCell<Vec<String>>,
}

impl SubstantiveTileProvider {
    fn new(scheme: Arc<dyn TilingScheme>) -> Self {
        Self {
            scheme,
            max_geometric_error_level_zero: 10000.0,
            tile_height_sample: 150.0,
            tile_eligible_for_unloading: true,
            distance_to_tile: 10_000_000.0,
            can_refine_max_level: 0,
            loaded_tiles: RefCell::new(Vec::new()),
        }
    }
}

impl QuadtreeTileProvider for SubstantiveTileProvider {
    fn tiling_scheme(&self) -> Arc<dyn TilingScheme> {
        self.scheme.clone()
    }

    fn get_level_maximum_geometric_error(&self, level: u32) -> f64 {
        self.max_geometric_error_level_zero / ((1 << level) as f64)
    }

    fn load_tile(&mut self, _frame_state: &FrameState, tile: &QuadtreeTileHandle) {
        let (level, x, y) = {
            let b = tile.borrow();
            (b.level, b.x, b.y)
        };
        self.loaded_tiles
            .borrow_mut()
            .push(format!("{level}_{x}_{y}"));
        let mut b = tile.borrow_mut();
        b.state = QuadtreeTileLoadState::Done;
        b.renderable = true;
        b.data = Some(Box::new(SubstantiveTileData::new(
            self.tile_height_sample + (level as f64 * 50.0),
            self.tile_eligible_for_unloading,
        )));
    }

    fn compute_tile_visibility(
        &self,
        _tile: &QuadtreeTile,
        _frame_state: &FrameState,
        _occluders: Option<&QuadtreeOccluders>,
    ) -> Visibility {
        Visibility::Full
    }

    fn compute_distance_to_tile(&self, _tile: &QuadtreeTile, _frame_state: &FrameState) -> f64 {
        self.distance_to_tile
    }

    fn show_tile_this_frame(
        &mut self,
        _tile: &QuadtreeTileHandle,
        _frame_state: &mut FrameState,
    ) {
    }

    fn can_refine(&self, tile: &QuadtreeTile) -> bool {
        tile.level < self.can_refine_max_level
    }
}

fn create_substantive_camera(position: Cartesian3) -> Camera {
    let mut camera = Camera::new(CameraOptions::new(800.0, 600.0));
    camera.position = position;
    camera.frustum = CameraFrustum::Perspective(PerspectiveFrustum::new(
        std::f64::consts::PI / 3.0,
        800.0 / 600.0,
    ));
    camera
}

fn create_substantive_frame_state(frame_number: u32, camera: &mut Camera) -> FrameState {
    let uniform_cam = camera.uniform_camera();
    let culling = camera
        .frustum
        .compute_culling_volume(camera.position, camera.direction, camera.up);
    let mut frame_state = FrameState::new(
        UniformFrame::new(
            uniform_cam,
            JulianDate::new(2451545.0, 0.0, TimeStandard::Tai),
        ),
        culling,
        [800, 600],
    );
    frame_state.frame_number = frame_number;
    frame_state.passes.render = true;
    frame_state.mode = SceneMode::Scene3D;
    frame_state
}

#[test]
fn test_screen_space_error_zero_distance_is_infinite_and_parity() {
    let fixture_str = include_str!("data/scene-quadtree-substantive.json");
    let fixture: Value = serde_json::from_str(fixture_str).expect("Valid substantive fixture JSON");
    let sse_fixture = &fixture["screen_space_error"];

    let scheme: Arc<dyn TilingScheme> = Arc::new(GeographicTilingScheme::default());
    let mut provider = SubstantiveTileProvider::new(scheme.clone());
    provider.max_geometric_error_level_zero = sse_fixture["level_zero_error"].as_f64().unwrap();

    let qp = QuadtreePrimitive::new(QuadtreePrimitiveOptions {
        tile_provider: provider,
        maximum_screen_space_error: Some(2.0),
        tile_cache_size: Some(10),
        loading_descendant_limit: Some(20),
        preload_ancestors: Some(true),
        preload_siblings: Some(false),
    });

    let mut camera = create_substantive_camera(Cartesian3::new(10_000_000.0, 0.0, 0.0));
    let frame_state = create_substantive_frame_state(1, &mut camera);

    let roots = QuadtreeTile::create_level_zero_tiles(scheme);
    let root0 = &roots[0];

    // 1. Distance = 0.0 must yield INFINITY (not 1.0 or clamped)
    root0.borrow_mut().distance = 0.0;
    let sse_zero = qp.screen_space_error(&frame_state, &camera, &root0.borrow());
    assert!(
        sse_zero.is_infinite(),
        "Screen-space error with distance 0 must be infinite, got {sse_zero}"
    );
    assert_eq!(
        sse_fixture["distance_zero_is_infinite"].as_bool().unwrap(),
        sse_zero.is_infinite()
    );

    // 2. Distance = 1000.0 matches pinned CesiumJS expectation
    root0.borrow_mut().distance = 1000.0;
    let sse_1000 = qp.screen_space_error(&frame_state, &camera, &root0.borrow());
    let expected_1000 = sse_fixture["distance_1000"].as_f64().unwrap();
    assert!(
        (sse_1000 - expected_1000).abs() < 1e-4,
        "SSE at dist 1000: expected {expected_1000}, got {sse_1000}"
    );

    // 3. Distance = 50000.0 matches pinned CesiumJS expectation
    root0.borrow_mut().distance = 50000.0;
    let sse_50000 = qp.screen_space_error(&frame_state, &camera, &root0.borrow());
    let expected_50000 = sse_fixture["distance_50000"].as_f64().unwrap();
    assert!(
        (sse_50000 - expected_50000).abs() < 1e-4,
        "SSE at dist 50000: expected {expected_50000}, got {sse_50000}"
    );
}

#[test]
fn test_height_callback_preserves_callback_and_invokes_on_load() {
    let scheme: Arc<dyn TilingScheme> = Arc::new(GeographicTilingScheme::default());
    let mut provider = SubstantiveTileProvider::new(scheme.clone());
    provider.tile_height_sample = 250.0;

    let mut qp = QuadtreePrimitive::new(QuadtreePrimitiveOptions {
        tile_provider: provider,
        maximum_screen_space_error: Some(2.0),
        tile_cache_size: Some(10),
        loading_descendant_limit: Some(20),
        preload_ancestors: Some(true),
        preload_siblings: Some(false),
    });

    let mut camera = create_substantive_camera(Cartesian3::new(10_000_000.0, 0.0, 0.0));

    // Register callback for location in root 0: lon = -1.0, lat = 0.2
    let invoked_carto = Rc::new(RefCell::new(None));
    let invoked_carto_clone = invoked_carto.clone();

    let id = qp.update_height(Cartographic::new(-1.0, 0.2, 0.0), move |carto| {
        *invoked_carto_clone.borrow_mut() = Some(carto);
    });

    assert_eq!(id, 1);
    assert_eq!(qp.add_height_callbacks.len(), 1);

    // Frame 1: Root tiles are created and queued for loading
    let mut frame1 = create_substantive_frame_state(1, &mut camera);
    qp.begin_frame(&frame1);
    qp.render(&mut frame1, &mut camera);

    // Callback must be distributed into root 0's custom data with callback intact!
    let root0 = qp.level_zero_tiles.as_ref().unwrap()[0].clone();
    assert!(root0.borrow().rectangle.contains(&Cartographic::new(-1.0, 0.2, 0.0)));
    assert_eq!(root0.borrow().custom_data.len(), 1);
    assert!(root0.borrow().custom_data[0].callback.is_some());

    qp.end_frame(&mut frame1);

    // Frame 2: Tiles are loaded and renderable. Height sampling executes and invokes callback!
    let mut frame2 = create_substantive_frame_state(2, &mut camera);
    qp.begin_frame(&frame2);
    qp.render(&mut frame2, &mut camera);
    qp.end_frame(&mut frame2);

    let result = *invoked_carto.borrow();
    assert!(result.is_some(), "Height callback should have been invoked");
    let result_carto = result.unwrap();
    assert!(
        (result_carto.height - 250.0).abs() < 1e-3,
        "Expected height 250.0, got {}",
        result_carto.height
    );
    assert!((result_carto.longitude - (-1.0)).abs() < 1e-6);
    assert!((result_carto.latitude - 0.2).abs() < 1e-6);

    // Verify cache hit on subsequent frame: position is in position_cache
    let cached = root0
        .borrow_mut()
        .get_position_cache_entry(&Cartographic::new(-1.0, 0.2, 0.0), 2.0);
    assert!(cached.is_some(), "Sampled position must be cached in tile LRU cache");

    // A newer, finer result must suppress stale copies retained on a parent.
    *invoked_carto.borrow_mut() = None;
    qp.active_height_callbacks.get_mut(&id).unwrap().level = 1;
    qp.tile_to_update_heights.push(root0.clone());
    qp.update_heights(&frame2);
    assert!(invoked_carto.borrow().is_none());
}

#[test]
fn test_height_callback_removal_preserves_position_and_removes_from_hierarchy() {
    let scheme: Arc<dyn TilingScheme> = Arc::new(GeographicTilingScheme::default());
    let provider = SubstantiveTileProvider::new(scheme.clone());

    let mut qp = QuadtreePrimitive::new(QuadtreePrimitiveOptions {
        tile_provider: provider,
        maximum_screen_space_error: Some(2.0),
        tile_cache_size: Some(10),
        loading_descendant_limit: Some(20),
        preload_ancestors: Some(true),
        preload_siblings: Some(false),
    });

    let mut camera = create_substantive_camera(Cartesian3::new(10_000_000.0, 0.0, 0.0));

    let invoked_count = Rc::new(RefCell::new(0));
    let count_clone = invoked_count.clone();

    let target_pos = Cartographic::new(-1.2, 0.3, 0.0);
    let id = qp.update_height(target_pos, move |_carto| {
        *count_clone.borrow_mut() += 1;
    });

    // Frame 1: distribute into tree
    let mut frame1 = create_substantive_frame_state(1, &mut camera);
    qp.begin_frame(&frame1);
    qp.render(&mut frame1, &mut camera);
    qp.end_frame(&mut frame1);

    let root0 = qp.level_zero_tiles.as_ref().unwrap()[0].clone();
    assert_eq!(root0.borrow().custom_data.len(), 1);

    // Now remove callback AFTER it was already distributed to tiles
    qp.remove_height_callback(id);

    // Critical assertion: removed registration must NOT have lost its position!
    assert_eq!(qp.remove_height_callbacks.len(), 1);
    assert_eq!(
        qp.remove_height_callbacks[0].position_cartographic, target_pos,
        "Removing registration must retain the original Cartographic position"
    );

    // Frame 2: distribution cleans up custom data from root 0
    let mut frame2 = create_substantive_frame_state(2, &mut camera);
    qp.begin_frame(&frame2);
    qp.render(&mut frame2, &mut camera);
    qp.end_frame(&mut frame2);

    assert_eq!(
        root0.borrow().custom_data.len(),
        0,
        "Custom data should be pruned from tile custom_data"
    );
}

#[test]
fn test_height_callback_invalidation_lifecycle() {
    let scheme: Arc<dyn TilingScheme> = Arc::new(GeographicTilingScheme::default());
    let mut provider = SubstantiveTileProvider::new(scheme.clone());
    provider.tile_height_sample = 320.0;

    let mut qp = QuadtreePrimitive::new(QuadtreePrimitiveOptions {
        tile_provider: provider,
        maximum_screen_space_error: Some(2.0),
        tile_cache_size: Some(10),
        loading_descendant_limit: Some(20),
        preload_ancestors: Some(true),
        preload_siblings: Some(false),
    });

    let mut camera = create_substantive_camera(Cartesian3::new(10_000_000.0, 0.0, 0.0));

    let callback_count = Rc::new(RefCell::new(0));
    let count_clone = callback_count.clone();

    let id = qp.update_height(Cartographic::new(-1.0, 0.2, 0.0), move |_carto| {
        *count_clone.borrow_mut() += 1;
    });

    // Frame 1: distribute and load
    let mut frame1 = create_substantive_frame_state(1, &mut camera);
    qp.begin_frame(&frame1);
    qp.render(&mut frame1, &mut camera);
    qp.end_frame(&mut frame1);

    // Frame 2: height sampled
    let mut frame2 = create_substantive_frame_state(2, &mut camera);
    qp.begin_frame(&frame2);
    qp.render(&mut frame2, &mut camera);
    qp.end_frame(&mut frame2);
    assert_eq!(*callback_count.borrow(), 1);

    // Invalidate all tiles: should preserve active callback and re-queue it
    qp.invalidate_all_tiles();

    // Frame 3: begin_frame runs invalidate_all_tiles_now
    let frame3 = create_substantive_frame_state(3, &mut camera);
    qp.begin_frame(&frame3);
    assert_eq!(
        qp.add_height_callbacks.len(),
        1,
        "Invalidation must preserve and re-queue active height callback"
    );
    assert!(
        qp.add_height_callbacks[0].callback.is_some(),
        "Callback must remain intact after tile invalidation"
    );
    assert_eq!(qp.add_height_callbacks[0].level, -1);
    assert_eq!(qp.active_height_callbacks[&id].level, -1);
    let mut frame3 = frame3;
    qp.render(&mut frame3, &mut camera);
    qp.end_frame(&mut frame3);
    let mut frame4 = create_substantive_frame_state(4, &mut camera);
    qp.begin_frame(&frame4);
    qp.render(&mut frame4, &mut camera);
    qp.end_frame(&mut frame4);
    assert_eq!(*callback_count.borrow(), 2, "replacement root triggers the callback again");
}

#[test]
fn test_cache_trim_frees_eligible_tiles_and_preserves_ineligible() {
    let scheme: Arc<dyn TilingScheme> = Arc::new(GeographicTilingScheme::default());
    let mut provider = SubstantiveTileProvider::new(scheme.clone());
    provider.distance_to_tile = 10_000_000.0;
    provider.tile_eligible_for_unloading = true;

    let mut qp = QuadtreePrimitive::new(QuadtreePrimitiveOptions {
        tile_provider: provider,
        maximum_screen_space_error: Some(2.0),
        tile_cache_size: Some(1),
        loading_descendant_limit: Some(20),
        preload_ancestors: Some(true),
        preload_siblings: Some(false),
    });

    let mut camera = create_substantive_camera(Cartesian3::new(10_000_000.0, 0.0, 0.0));

    // Frame 1: loads the 2 roots
    let mut frame1 = create_substantive_frame_state(1, &mut camera);
    qp.begin_frame(&frame1);
    qp.render(&mut frame1, &mut camera);
    qp.end_frame(&mut frame1);

    // Frame 2: render both roots
    let mut frame2 = create_substantive_frame_state(2, &mut camera);
    qp.begin_frame(&frame2);
    qp.render(&mut frame2, &mut camera);
    qp.end_frame(&mut frame2);

    assert_eq!(qp.tile_replacement_queue.count(), 2);

    let root0 = qp.level_zero_tiles.as_ref().unwrap()[0].clone();

    // Verify root 0 data exists
    assert!(root0.borrow().data.is_some());
    assert_eq!(root0.borrow().state, QuadtreeTileLoadState::Done);

    // Frame 3: start frame, mark only root 1 as rendered (root 0 unrendered)
    let frame3 = create_substantive_frame_state(3, &mut camera);
    qp.begin_frame(&frame3);

    // Queue a dummy load to trigger process_tile_load_queue and trimming
    let dummy_child = QuadtreeTile::northwest_child(&root0);
    qp.tile_load_queue_low.push(dummy_child);

    // Mark only root 1 as rendered this frame
    qp.tile_replacement_queue.mark_tile_rendered((0, 1, 0));

    // Trim tiles with cache size = 1
    qp.process_tile_load_queue(&frame3);

    // Root 0 should have been freed (resources cleared, state = Start, data = None)!
    assert_eq!(
        root0.borrow().state,
        QuadtreeTileLoadState::Start,
        "Eligible unrendered tile should be freed on cache trim"
    );
    assert!(
        root0.borrow().data.is_none(),
        "Eligible unrendered tile data must be freed"
    );
}

#[test]
fn test_cache_trim_ineligible_tiles_remain_in_queue() {
    let scheme: Arc<dyn TilingScheme> = Arc::new(GeographicTilingScheme::default());
    let mut provider = SubstantiveTileProvider::new(scheme.clone());
    provider.distance_to_tile = 10_000_000.0;
    // Tiles are INELIGIBLE for unloading
    provider.tile_eligible_for_unloading = false;

    let mut qp = QuadtreePrimitive::new(QuadtreePrimitiveOptions {
        tile_provider: provider,
        maximum_screen_space_error: Some(2.0),
        tile_cache_size: Some(1),
        loading_descendant_limit: Some(20),
        preload_ancestors: Some(true),
        preload_siblings: Some(false),
    });

    let mut camera = create_substantive_camera(Cartesian3::new(10_000_000.0, 0.0, 0.0));

    // Frame 1: load roots
    let mut frame1 = create_substantive_frame_state(1, &mut camera);
    qp.begin_frame(&frame1);
    qp.render(&mut frame1, &mut camera);
    qp.end_frame(&mut frame1);

    // Frame 2: render roots
    let mut frame2 = create_substantive_frame_state(2, &mut camera);
    qp.begin_frame(&frame2);
    qp.render(&mut frame2, &mut camera);
    qp.end_frame(&mut frame2);

    assert_eq!(qp.tile_replacement_queue.count(), 2);

    let root0 = qp.level_zero_tiles.as_ref().unwrap()[0].clone();

    // Frame 3: start frame, mark only root 1 rendered
    let frame3 = create_substantive_frame_state(3, &mut camera);
    qp.begin_frame(&frame3);

    let dummy = QuadtreeTile::northwest_child(&root0);
    qp.tile_load_queue_low.push(dummy);
    qp.tile_replacement_queue.mark_tile_rendered((0, 1, 0));

    // Trim with cache size 1: root 0 is ineligible, so it must NOT be freed!
    qp.process_tile_load_queue(&frame3);

    assert_eq!(
        root0.borrow().state,
        QuadtreeTileLoadState::Done,
        "Ineligible tile must NOT be freed during cache trim"
    );
    assert!(
        root0.borrow().data.is_some(),
        "Ineligible tile data must NOT be freed"
    );
}

#[test]
fn test_cache_trim_with_borrowed_descendant_avoids_panic() {
    let scheme: Arc<dyn TilingScheme> = Arc::new(GeographicTilingScheme::default());
    let provider = SubstantiveTileProvider::new(scheme.clone());

    let mut qp = QuadtreePrimitive::new(QuadtreePrimitiveOptions {
        tile_provider: provider,
        maximum_screen_space_error: Some(2.0),
        tile_cache_size: Some(1),
        loading_descendant_limit: Some(20),
        preload_ancestors: Some(true),
        preload_siblings: Some(false),
    });

    let mut camera = create_substantive_camera(Cartesian3::new(10_000_000.0, 0.0, 0.0));
    let mut frame1 = create_substantive_frame_state(1, &mut camera);
    qp.begin_frame(&frame1);
    qp.render(&mut frame1, &mut camera);
    qp.end_frame(&mut frame1);

    let root0 = qp.level_zero_tiles.as_ref().unwrap()[0].clone();
    let child = QuadtreeTile::northwest_child(&root0);

    // Hold a borrow guard on child
    let guard = child.borrow();

    // Start frame 2
    let frame2 = create_substantive_frame_state(2, &mut camera);
    qp.begin_frame(&frame2);

    // Trigger trimming: should NOT panic even though child is borrowed
    qp.tile_load_queue_low.push(root0.clone());
    qp.process_tile_load_queue(&frame2);

    drop(guard);
}

#[test]
fn test_load_queues_retain_length_until_progress_events() {
    let scheme: Arc<dyn TilingScheme> = Arc::new(GeographicTilingScheme::default());
    let provider = SubstantiveTileProvider::new(scheme);

    let mut qp = QuadtreePrimitive::new(QuadtreePrimitiveOptions {
        tile_provider: provider,
        maximum_screen_space_error: Some(2.0),
        tile_cache_size: Some(10),
        loading_descendant_limit: Some(20),
        preload_ancestors: Some(true),
        preload_siblings: Some(false),
    });

    let progress_events = Arc::new(Mutex::new(Vec::new()));
    let events_clone = progress_events.clone();

    qp.tile_load_progress_event.add_event_listener(move |len| {
        events_clone.lock().unwrap().push(*len);
    });

    let mut camera = create_substantive_camera(Cartesian3::new(10_000_000.0, 0.0, 0.0));

    // Frame 1: Roots not yet loaded -> queued with high priority (length = 2)
    let mut frame1 = create_substantive_frame_state(1, &mut camera);
    qp.begin_frame(&frame1);
    qp.render(&mut frame1, &mut camera);
    assert_eq!(qp.tile_load_queue_high.len(), 2);

    // end_frame processes queue and fires progress event
    qp.end_frame(&mut frame1);

    // The progress event MUST have received 2, not 0!
    assert!(
        progress_events.lock().unwrap().contains(&2),
        "Progress event must have received queue length 2 during end_frame, got {:?}",
        *progress_events.lock().unwrap()
    );

    // Frame 2: Tiles are loaded and renderable, nothing queued
    let mut frame2 = create_substantive_frame_state(2, &mut camera);
    qp.begin_frame(&frame2);
    qp.render(&mut frame2, &mut camera);
    qp.end_frame(&mut frame2);

    // Queue length changed to 0, so progress event must have received 0
    assert!(
        progress_events.lock().unwrap().contains(&0),
        "Progress event must have received queue length 0 when loading finished, got {:?}",
        *progress_events.lock().unwrap()
    );
}

#[test]
fn find_tile_skips_mutably_borrowed_ancestors() {
    let scheme: Arc<dyn TilingScheme> = Arc::new(GeographicTilingScheme::default());
    let roots = QuadtreeTile::create_level_zero_tiles(scheme);
    let root = roots[0].clone();
    let child = QuadtreeTile::northwest_child(&root);
    let grandchild = QuadtreeTile::northwest_child(&child);
    let root_guard = root.borrow_mut();
    assert!(QuadtreePrimitive::<SubstantiveTileProvider>::find_tile_in_roots(Some(&roots), (0, 0, 0)).is_none());
    drop(root_guard);
    let child_guard = child.borrow_mut();
    let coordinate = { let tile = grandchild.borrow(); (tile.level, tile.x, tile.y) };
    assert!(QuadtreePrimitive::<SubstantiveTileProvider>::find_tile_in_roots(Some(&roots), coordinate).is_none());
    drop(child_guard);
    assert!(QuadtreePrimitive::<SubstantiveTileProvider>::find_tile_in_roots(Some(&roots), coordinate).is_some());
}
