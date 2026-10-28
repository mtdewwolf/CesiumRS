//! Dedicated CPU behavior, hierarchy, spatial hash, neighbor lookup, and LOD selection
//! tests for Scene `QuadtreeTile` and `QuadtreePrimitive`.

pub use cesium_rs::core;
pub use cesium_rs::scene;

use cesium_rs::core::{
    Cartesian3, Cartographic, GeographicTilingScheme, JulianDate, Rectangle,
    TimeStandard, TilingScheme, Visibility, WebMercatorTilingScheme,
};
use cesium_rs::renderer::UniformFrame;
use cesium_rs::scene::camera::{Camera, CameraOptions};
use cesium_rs::scene::frame_state::FrameState;
use cesium_rs::scene::quadtree_occluders::QuadtreeOccluders;
use cesium_rs::scene::quadtree_tile_load_state::QuadtreeTileLoadState;

use cesium_rs::scene::quadtree_tile;
use cesium_rs::scene::quadtree_primitive;

use quadtree_primitive::{
    compute_default_level_zero_maximum_geometric_error, QuadtreePrimitive,
    QuadtreePrimitiveOptions, QuadtreeTileProvider,
};
use quadtree_tile::{
    create_spatial_hash_key, LruCache, QuadtreeTile, QuadtreeTileHandle,
};
use serde_json::Value;
use std::cell::RefCell;
use std::sync::Arc;

struct MockQuadtreeTileProvider {
    scheme: Arc<dyn TilingScheme>,
    max_geometric_error_level_zero: f64,
    can_refine_max_level: u32,
    loaded_tiles: RefCell<Vec<String>>,
    shown_tiles: RefCell<Vec<String>>,
    visibility: Visibility,
}

impl MockQuadtreeTileProvider {
    fn new(scheme: Arc<dyn TilingScheme>) -> Self {
        Self {
            scheme,
            max_geometric_error_level_zero: 10000.0,
            can_refine_max_level: 5,
            loaded_tiles: RefCell::new(Vec::new()),
            shown_tiles: RefCell::new(Vec::new()),
            visibility: Visibility::Full,
        }
    }
}

impl QuadtreeTileProvider for MockQuadtreeTileProvider {
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
        tile.borrow_mut().state = QuadtreeTileLoadState::Done;
        tile.borrow_mut().renderable = true;
    }

    fn compute_tile_visibility(
        &self,
        _tile: &QuadtreeTile,
        _frame_state: &FrameState,
        _occluders: Option<&QuadtreeOccluders>,
    ) -> Visibility {
        self.visibility
    }

    fn compute_distance_to_tile(&self, _tile: &QuadtreeTile, _frame_state: &FrameState) -> f64 {
        10_000_000.0
    }

    fn show_tile_this_frame(
        &mut self,
        tile: &QuadtreeTileHandle,
        _frame_state: &mut FrameState,
    ) {
        let (level, x, y) = {
            let b = tile.borrow();
            (b.level, b.x, b.y)
        };
        self.shown_tiles.borrow_mut().push(format!("{level}_{x}_{y}"));
    }

    fn can_refine(&self, tile: &QuadtreeTile) -> bool {
        tile.level < self.can_refine_max_level
    }
}

fn create_test_camera(position: Cartesian3) -> Camera {
    let mut camera = Camera::new(CameraOptions::new(800.0, 600.0));
    camera.position = position;
    camera
}

fn create_test_frame_state(frame_number: u32, camera: &mut Camera) -> FrameState {
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
    frame_state
}

#[test]
fn test_quadtree_tile_fixture_parity() {
    let fixture_str = include_str!("data/QuadtreeTile.json");
    let fixture: Value = serde_json::from_str(fixture_str).expect("Valid QuadtreeTile fixture");

    let scheme: Arc<dyn TilingScheme> = Arc::new(GeographicTilingScheme::default());
    let roots = QuadtreeTile::create_level_zero_tiles(scheme.clone());

    let cases = fixture["cases"].as_array().expect("cases array");
    for case in cases {
        let test_name = case["test"].as_str().unwrap();
        match test_name {
            "level_zero_roots" => {
                let count = case["count"].as_u64().unwrap() as usize;
                assert_eq!(roots.len(), count);

                let r0 = &case["root0"];
                assert_eq!(roots[0].borrow().x, r0["x"].as_i64().unwrap() as i32);
                assert_eq!(roots[0].borrow().y, r0["y"].as_i64().unwrap() as i32);
                assert_eq!(roots[0].borrow().level, r0["level"].as_u64().unwrap() as u32);

                let r1 = &case["root1"];
                assert_eq!(roots[1].borrow().x, r1["x"].as_i64().unwrap() as i32);
                assert_eq!(roots[1].borrow().y, r1["y"].as_i64().unwrap() as i32);
                assert_eq!(roots[1].borrow().level, r1["level"].as_u64().unwrap() as u32);
            }
            "root0_children_coordinates" => {
                let nw = QuadtreeTile::northwest_child(&roots[0]);
                let ne = QuadtreeTile::northeast_child(&roots[0]);
                let sw = QuadtreeTile::southwest_child(&roots[0]);
                let se = QuadtreeTile::southeast_child(&roots[0]);

                assert_eq!(nw.borrow().x, case["northwest"]["x"].as_i64().unwrap() as i32);
                assert_eq!(nw.borrow().y, case["northwest"]["y"].as_i64().unwrap() as i32);
                assert_eq!(nw.borrow().level, case["northwest"]["level"].as_u64().unwrap() as u32);

                assert_eq!(ne.borrow().x, case["northeast"]["x"].as_i64().unwrap() as i32);
                assert_eq!(ne.borrow().y, case["northeast"]["y"].as_i64().unwrap() as i32);
                assert_eq!(ne.borrow().level, case["northeast"]["level"].as_u64().unwrap() as u32);

                assert_eq!(sw.borrow().x, case["southwest"]["x"].as_i64().unwrap() as i32);
                assert_eq!(sw.borrow().y, case["southwest"]["y"].as_i64().unwrap() as i32);
                assert_eq!(sw.borrow().level, case["southwest"]["level"].as_u64().unwrap() as u32);

                assert_eq!(se.borrow().x, case["southeast"]["x"].as_i64().unwrap() as i32);
                assert_eq!(se.borrow().y, case["southeast"]["y"].as_i64().unwrap() as i32);
                assert_eq!(se.borrow().level, case["southeast"]["level"].as_u64().unwrap() as u32);
            }
            "root1_children_coordinates" => {
                let nw = QuadtreeTile::northwest_child(&roots[1]);
                let ne = QuadtreeTile::northeast_child(&roots[1]);
                let sw = QuadtreeTile::southwest_child(&roots[1]);
                let se = QuadtreeTile::southeast_child(&roots[1]);

                assert_eq!(nw.borrow().x, case["northwest"]["x"].as_i64().unwrap() as i32);
                assert_eq!(ne.borrow().x, case["northeast"]["x"].as_i64().unwrap() as i32);
                assert_eq!(sw.borrow().x, case["southwest"]["x"].as_i64().unwrap() as i32);
                assert_eq!(se.borrow().x, case["southeast"]["x"].as_i64().unwrap() as i32);
            }
            "spatial_hash_keys" => {
                let results = case["results"].as_array().unwrap();
                for item in results {
                    let lon = item["lon"].as_f64().unwrap();
                    let lat = item["lat"].as_f64().unwrap();
                    let sse = item["sse"].as_f64().unwrap();
                    let rect_arr = item["rect"].as_array().unwrap();
                    let rect = Rectangle::new(
                        rect_arr[0].as_f64().unwrap(),
                        rect_arr[1].as_f64().unwrap(),
                        rect_arr[2].as_f64().unwrap(),
                        rect_arr[3].as_f64().unwrap(),
                    );
                    let expected_key = item["key"].as_str().unwrap();

                    let actual_key = create_spatial_hash_key(lon, lat, &rect, sse);
                    assert_eq!(actual_key, expected_key, "Calculated spatial hash key match");
                }
            }
            "neighbor_topology" => {
                let neighbor_cases = case["cases"].as_array().unwrap();
                for ncase in neighbor_cases {
                    let desc = ncase["desc"].as_str().unwrap();
                    let from = &ncase["from"];
                    let dir = ncase["dir"].as_str().unwrap();
                    let to = &ncase["to"];

                    let from_level = from["level"].as_u64().unwrap() as u32;
                    let from_x = from["x"].as_i64().unwrap() as i32;
                    let from_y = from["y"].as_i64().unwrap() as i32;

                    let from_tile = if from_level == 0 {
                        roots
                            .iter()
                            .find(|t| t.borrow().x == from_x && t.borrow().y == from_y)
                            .unwrap()
                            .clone()
                    } else {
                        let parent_x = from_x / 2;
                        let parent = roots.iter().find(|t| t.borrow().x == parent_x).unwrap();
                        let is_north = (from_y % 2) == 0;
                        let is_west = (from_x % 2) == 0;
                        match (is_north, is_west) {
                            (true, true) => QuadtreeTile::northwest_child(parent),
                            (true, false) => QuadtreeTile::northeast_child(parent),
                            (false, true) => QuadtreeTile::southwest_child(parent),
                            (false, false) => QuadtreeTile::southeast_child(parent),
                        }
                    };

                    let neighbor = match dir {
                        "east" => QuadtreeTile::find_tile_to_east(&from_tile, &roots),
                        "west" => QuadtreeTile::find_tile_to_west(&from_tile, &roots),
                        "north" => QuadtreeTile::find_tile_to_north(&from_tile, &roots),
                        "south" => QuadtreeTile::find_tile_to_south(&from_tile, &roots),
                        _ => unreachable!(),
                    };

                    if to.is_null() {
                        assert!(neighbor.is_none(), "Expected no neighbor for {desc}");
                    } else {
                        assert!(neighbor.is_some(), "Expected neighbor for {desc}");
                        let n_b = neighbor.unwrap();
                        let b = n_b.borrow();
                        assert_eq!(
                            b.level,
                            to["level"].as_u64().unwrap() as u32,
                            "level match for {desc}"
                        );
                        assert_eq!(
                            b.x,
                            to["x"].as_i64().unwrap() as i32,
                            "x match for {desc}"
                        );
                        assert_eq!(
                            b.y,
                            to["y"].as_i64().unwrap() as i32,
                            "y match for {desc}"
                        );
                    }
                }
            }
            other => panic!("Unexpected test name: {other}"),
        }
    }
}

#[test]
fn test_quadtree_primitive_fixture_parity() {
    let fixture_str = include_str!("data/QuadtreePrimitive.json");
    let fixture: Value =
        serde_json::from_str(fixture_str).expect("Valid QuadtreePrimitive fixture");

    let geo_scheme = GeographicTilingScheme::default();
    let merc_scheme = WebMercatorTilingScheme::default();

    let cases = fixture["cases"].as_array().expect("cases array");
    for case in cases {
        let test_name = case["test"].as_str().unwrap();
        match test_name {
            "default_geometric_errors" => {
                let expected_geo = case["geographic"].as_f64().unwrap();
                let expected_merc = case["web_mercator"].as_f64().unwrap();

                let actual_geo = compute_default_level_zero_maximum_geometric_error(&geo_scheme);
                let actual_merc =
                    compute_default_level_zero_maximum_geometric_error(&merc_scheme);

                assert!((actual_geo - expected_geo).abs() < 1e-6);
                assert!((actual_merc - expected_merc).abs() < 1e-6);
            }
            "primitive_defaults" => {
                let provider = MockQuadtreeTileProvider::new(Arc::new(geo_scheme.clone()));
                let qp = QuadtreePrimitive::new(QuadtreePrimitiveOptions {
                    tile_provider: provider,
                    maximum_screen_space_error: None,
                    tile_cache_size: None,
                    loading_descendant_limit: None,
                    preload_ancestors: None,
                    preload_siblings: None,
                });

                assert_eq!(
                    qp.maximum_screen_space_error,
                    case["maximumScreenSpaceError"].as_f64().unwrap()
                );
                assert_eq!(
                    qp.tile_cache_size,
                    case["tileCacheSize"].as_u64().unwrap() as usize
                );
                assert_eq!(
                    qp.loading_descendant_limit,
                    case["loadingDescendantLimit"].as_u64().unwrap() as usize
                );
                assert_eq!(
                    qp.preload_ancestors,
                    case["preloadAncestors"].as_bool().unwrap()
                );
                assert_eq!(
                    qp.preload_siblings,
                    case["preloadSiblings"].as_bool().unwrap()
                );
            }
            other => panic!("Unexpected test name: {other}"),
        }
    }
}

#[test]
fn test_lru_cache_behavior() {
    let mut cache: LruCache<String, i32> = LruCache::new(3);
    assert!(cache.is_empty());
    assert_eq!(cache.len(), 0);

    cache.set("a".to_string(), 1);
    cache.set("b".to_string(), 2);
    cache.set("c".to_string(), 3);
    assert_eq!(cache.len(), 3);

    // Access "a" to make it most recently used
    assert_eq!(cache.get(&"a".to_string()), Some(&1));

    // Adding "d" should evict "b" (since "a" was used recently, order was: b, c, a)
    cache.set("d".to_string(), 4);
    assert_eq!(cache.len(), 3);
    assert_eq!(cache.get(&"b".to_string()), None);
    assert_eq!(cache.get(&"a".to_string()), Some(&1));
    assert_eq!(cache.get(&"c".to_string()), Some(&3));
    assert_eq!(cache.get(&"d".to_string()), Some(&4));

    cache.clear();
    assert!(cache.is_empty());
    assert_eq!(cache.get(&"a".to_string()), None);
}

#[test]
fn test_quadtree_tile_lazy_children_and_free_resources() {
    let scheme: Arc<dyn TilingScheme> = Arc::new(GeographicTilingScheme::default());
    let roots = QuadtreeTile::create_level_zero_tiles(scheme);
    let root0 = &roots[0];

    assert!(root0.borrow().northwest_child.is_none());
    assert!(root0.borrow().northeast_child.is_none());
    assert!(root0.borrow().southwest_child.is_none());
    assert!(root0.borrow().southeast_child.is_none());

    let _children = QuadtreeTile::children(root0);
    assert!(root0.borrow().northwest_child.is_some());
    assert!(root0.borrow().northeast_child.is_some());
    assert!(root0.borrow().southwest_child.is_some());
    assert!(root0.borrow().southeast_child.is_some());

    // Spatial quadrant child lookup
    let pos_nw = Cartographic::new(-2.0, 0.5, 0.0);
    let child_nw = QuadtreeTile::child_tile_at_position(root0, &pos_nw);
    assert_eq!(child_nw.borrow().x, 0);
    assert_eq!(child_nw.borrow().y, 0);

    let pos_se = Cartographic::new(-0.5, -0.5, 0.0);
    let child_se = QuadtreeTile::child_tile_at_position(root0, &pos_se);
    assert_eq!(child_se.borrow().x, 1);
    assert_eq!(child_se.borrow().y, 1);

    // Free resources
    root0.borrow_mut().renderable = true;
    root0.borrow_mut().state = QuadtreeTileLoadState::Done;
    root0.borrow_mut().free_resources();

    assert_eq!(root0.borrow().state, QuadtreeTileLoadState::Start);
    assert!(!root0.borrow().renderable);
    assert!(root0.borrow().northwest_child.is_none());
    assert!(root0.borrow().northeast_child.is_none());
    assert!(root0.borrow().southwest_child.is_none());
    assert!(root0.borrow().southeast_child.is_none());
}

#[test]
fn test_quadtree_primitive_render_loop_lifecycle() {
    let scheme: Arc<dyn TilingScheme> = Arc::new(GeographicTilingScheme::default());
    let provider = MockQuadtreeTileProvider::new(scheme);
    let mut qp = QuadtreePrimitive::new(QuadtreePrimitiveOptions {
        tile_provider: provider,
        maximum_screen_space_error: Some(2.0),
        tile_cache_size: Some(50),
        loading_descendant_limit: Some(20),
        preload_ancestors: Some(true),
        preload_siblings: Some(false),
    });

    let mut camera = create_test_camera(Cartesian3::new(10_000_000.0, 0.0, 0.0));

    // Frame 1: Root tiles created, not renderable yet -> load queue high, 0 tiles rendered
    let mut frame1 = create_test_frame_state(1, &mut camera);
    qp.begin_frame(&frame1);
    qp.render(&mut frame1, &mut camera);
    assert_eq!(qp.tiles_to_render.len(), 0);
    assert_eq!(qp.debug.tiles_waiting_for_children, 2);
    assert_eq!(qp.tile_load_queue_high.len(), 2);

    qp.end_frame(&mut frame1);
    // After end_frame, provider loaded the 2 root tiles
    assert_eq!(qp.tile_provider().loaded_tiles.borrow().len(), 2);

    // Frame 2: Root tiles are now loaded and renderable -> rendered
    let mut frame2 = create_test_frame_state(2, &mut camera);
    qp.begin_frame(&frame2);
    qp.render(&mut frame2, &mut camera);
    assert_eq!(qp.tiles_to_render.len(), 2);
    assert_eq!(qp.tile_provider().shown_tiles.borrow().len(), 2);
    assert_eq!(qp.tile_replacement_queue.count(), 2);

    qp.end_frame(&mut frame2);

    // Frame 3: Invalidation test
    qp.invalidate_all_tiles();
    let frame3 = create_test_frame_state(3, &mut camera);
    qp.begin_frame(&frame3);
    // After invalidate_all_tiles, level zero tiles are recreated and replacement queue cleared
    assert_eq!(qp.tile_replacement_queue.count(), 0);
}

#[test]
fn test_height_callbacks_and_replacement_trimming() {
    let scheme: Arc<dyn TilingScheme> = Arc::new(GeographicTilingScheme::default());
    let provider = MockQuadtreeTileProvider::new(scheme);
    let mut qp = QuadtreePrimitive::new(QuadtreePrimitiveOptions {
        tile_provider: provider,
        maximum_screen_space_error: Some(2.0),
        tile_cache_size: Some(2),
        loading_descendant_limit: Some(20),
        preload_ancestors: Some(true),
        preload_siblings: Some(false),
    });

    let id = qp.update_height(Cartographic::new(-1.0, 0.2, 0.0), |_pos| {});
    assert_eq!(id, 1);
    assert_eq!(qp.add_height_callbacks.len(), 1);

    qp.remove_height_callback(id);
    assert_eq!(qp.add_height_callbacks.len(), 0);
    assert_eq!(qp.remove_height_callbacks.len(), 1);

    // Replacement queue marking and trimming
    let mut rq = qp.tile_replacement_queue;
    rq.mark_tile_rendered((0, 0, 0));
    rq.mark_tile_rendered((0, 1, 0));
    rq.mark_tile_rendered((1, 0, 0));
    assert_eq!(rq.count(), 3);

    rq.mark_start_of_render_frame();
    // Render only (1, 0, 0)
    rq.mark_tile_rendered((1, 0, 0));

    // Trim to 1 tile: unrendered (0, 0, 0) and (0, 1, 0) can be trimmed
    rq.trim_tiles(1, |_coord| true);
    assert!(rq.count() <= 2);
}

#[test]
fn borrowed_descendant_does_not_partially_free_subtree() {
    let scheme = Arc::new(GeographicTilingScheme::default());
    let root = QuadtreeTile::create_level_zero_tiles(scheme)[0].clone();
    let child = QuadtreeTile::northwest_child(&root);
    let grandchild = QuadtreeTile::northwest_child(&child);
    let guard = grandchild.borrow();
    assert!(std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| root.borrow_mut().free_resources())).is_err());
    drop(guard);
    assert!(root.borrow().northwest_child.is_some());
    assert!(child.borrow().northwest_child.is_some());
    root.borrow_mut().free_resources();
    assert!(root.borrow().northwest_child.is_none());
}
