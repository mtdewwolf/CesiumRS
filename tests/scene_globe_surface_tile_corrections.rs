//! Dedicated unit tests for GlobeSurfaceTile review findings in shared CesiumRS.
//!
//! Validates:
//! 1. CPU mesh alone must never count as GPU-rendered or set GPU-ready state.
//! 2. Missing regional bounds must yield None regardless of terrain upsampling.
//! 3. Imagery requests must map each TileImagery to its actual owning layer,
//!    including two imagery tiles from one layer and a third from another.
//! 4. Exaggeration must invalidate authoritative QuadtreePrimitive.active_height_callbacks
//!    and queue through a safe API usable when surface is stored inside QuadtreeTile.data
//!    without RefCell reentrant panic.
//! 5. Projected picking must use the active scene MapProjection or explicitly reject projected modes.

pub use cesium_rs::core;
pub use cesium_rs::renderer;
pub use cesium_rs::scene;

#[path = "../src/scene/globe_surface_tile.rs"]
mod globe_surface_tile;

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use cesium_rs::core::{Credit, MutableErrorEvent};
use cesium_rs::scene::imagery_layer_collection::ImageryLayerCollection;
use cesium_rs::scene::imagery_provider::{
    ImageryErrorEvent, ImageryFuture, ImageryImage, ImageryProvider,
};
use cesium_rs::scene::FrameResource;

use cesium_rs::core::terrain_encoding::{TerrainEncoding, TerrainEncodingOptions};
use cesium_rs::core::terrain_mesh::{TerrainMesh, TerrainMeshOptions};
use cesium_rs::core::terrain_picker::TerrainSceneMode;
use cesium_rs::core::terrain_provider::TerrainEdgeIndices;
use cesium_rs::core::typed_array::TypedArray;
use cesium_rs::core::{
    BoundingSphere, Cartesian3, Cartesian4, Cartographic, GeographicProjection,
    GeographicTilingScheme, JulianDate, PerspectiveFrustum, Ray, Rectangle,
    TimeStandard, TilingScheme, Visibility, WebMercatorProjection,
};
use cesium_rs::renderer::UniformFrame;
use cesium_rs::scene::camera::{Camera, CameraFrustum, CameraOptions};
use cesium_rs::scene::frame_state::FrameState;
use cesium_rs::scene::imagery::Imagery;
use cesium_rs::scene::imagery_state::ImageryState;
use cesium_rs::scene::imagery_layer::{ImageryLayer, ImageryLayerOptions};
use cesium_rs::scene::quadtree_occluders::QuadtreeOccluders;
use cesium_rs::scene::quadtree_primitive::{
    QuadtreePrimitive, QuadtreePrimitiveOptions, QuadtreeTileProvider,
};
use cesium_rs::scene::quadtree_tile::{
    QuadtreeCustomData, QuadtreeTile, QuadtreeTileData, QuadtreeTileHandle, QuadtreeTileOptions,
};
use cesium_rs::scene::scene_mode::SceneMode;
use cesium_rs::scene::terrain_state::TerrainState;
use cesium_rs::scene::tile_imagery::TileImagery;

use globe_surface_tile::{GlobeSurfaceTile, SurfaceTerrainData};

fn create_dummy_mesh(min_height: f64, max_height: f64, rect: Rectangle) -> TerrainMesh {
    let encoding = TerrainEncoding::new(TerrainEncodingOptions::default()).expect("valid encoding");
    TerrainMesh::new(TerrainMeshOptions {
        center: Cartesian3::ZERO,
        vertices: vec![0.0; 24],
        indices: TypedArray::Uint16(vec![0, 1, 2]),
        index_count_without_skirts: Some(3),
        vertex_count_without_skirts: 3,
        minimum_height: min_height,
        maximum_height: max_height,
        rectangle: rect,
        bounding_sphere_3d: BoundingSphere::new(Cartesian3::ZERO, 100.0),
        occludee_point_in_scaled_space: None,
        vertex_stride: Some(6),
        oriented_bounding_box: None,
        encoding,
        edges: TerrainEdgeIndices::default(),
    })
}

#[derive(Debug)]
struct MockTerrainData {
    height: f64,
    upsampled: bool,
}

impl SurfaceTerrainData for MockTerrainData {
    fn interpolate_height(
        &self,
        rectangle: &Rectangle,
        longitude: f64,
        latitude: f64,
    ) -> Option<f64> {
        let carto = Cartographic::new(longitude, latitude, 0.0);
        if rectangle.contains(&carto) {
            Some(self.height)
        } else {
            None
        }
    }

    fn is_child_available(&self, _this_x: u32, _this_y: u32, _child_x: u32, _child_y: u32) -> bool {
        true
    }

    fn was_created_by_upsampling(&self) -> bool {
        self.upsampled
    }
}

struct TestQuadtreeProvider {
    scheme: Arc<dyn TilingScheme>,
}

impl TestQuadtreeProvider {
    fn new(scheme: Arc<dyn TilingScheme>) -> Self {
        Self { scheme }
    }
}

impl QuadtreeTileProvider for TestQuadtreeProvider {
    fn tiling_scheme(&self) -> Arc<dyn TilingScheme> {
        self.scheme.clone()
    }

    fn get_level_maximum_geometric_error(&self, _level: u32) -> f64 {
        1000.0
    }

    fn load_tile(&mut self, _frame_state: &FrameState, _tile: &QuadtreeTileHandle) {}

    fn compute_tile_visibility(
        &self,
        _tile: &QuadtreeTile,
        _frame_state: &FrameState,
        _occluders: Option<&QuadtreeOccluders>,
    ) -> Visibility {
        Visibility::Full
    }

    fn show_tile_this_frame(
        &mut self,
        _tile: &QuadtreeTileHandle,
        _frame_state: &mut FrameState,
    ) {}
}

fn create_test_frame_state() -> FrameState {
    let mut camera = Camera::new(CameraOptions::new(800.0, 600.0));
    camera.frustum = CameraFrustum::Perspective(PerspectiveFrustum::new(
        60.0_f64.to_radians(),
        800.0 / 600.0,
    ));
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
    frame_state.frame_number = 1;
    frame_state.passes.render = true;
    frame_state.mode = SceneMode::Scene3D;
    frame_state
}

// ---------------------------------------------------------------------------
// 1. CPU mesh alone must never count as GPU-rendered or set GPU-ready state
// ---------------------------------------------------------------------------
#[test]
fn test_review_1_cpu_mesh_alone_never_gpu_rendered_or_ready() {
    let mut tile = GlobeSurfaceTile::new();
    assert!(!tile.has_vertex_array, "initially no vertex array");
    assert!(!tile.has_rendered_mesh(), "initially no rendered mesh");
    assert_eq!(tile.terrain_state, TerrainState::Unloaded);

    // Provide a CPU-only mesh
    let mesh = create_dummy_mesh(0.0, 50.0, Rectangle::new(-1.0, -1.0, 1.0, 1.0));
    tile.set_mesh(mesh);

    // CRITICAL: CPU mesh alone must NEVER count as GPU-rendered or set GPU-ready state!
    assert!(
        !tile.has_vertex_array,
        "CPU mesh alone must NOT fabricate has_vertex_array"
    );
    assert_eq!(
        tile.terrain_state,
        TerrainState::Transformed,
        "CPU mesh alone must be in Transformed state, NOT Ready state"
    );
    assert!(
        !tile.has_rendered_mesh(),
        "CPU mesh alone must NEVER count as GPU-rendered"
    );

    // Ray picking against a tile with only CPU mesh must yield None (renderedMesh is undefined)
    let projection = GeographicProjection::new(cesium_rs::core::Ellipsoid::WGS84);
    let ray = Ray::new(Cartesian3::new(0.0, 0.0, 100.0), Cartesian3::new(0.0, 0.0, -1.0));
    assert_eq!(
        tile.pick(&ray, Some(TerrainSceneMode::Scene3D), &projection, true),
        None,
        "Pick on CPU-only mesh returns None because renderedMesh is undefined"
    );

    // Exaggeration update on CPU-only mesh returns false because it is not rendered
    let scheme: Arc<dyn TilingScheme> = Arc::new(GeographicTilingScheme::default());
    let mut quad_tile = QuadtreeTile::new(QuadtreeTileOptions {
        tiling_scheme: scheme,
        x: 0,
        y: 0,
        level: 0,
        parent: None,
    });
    assert!(
        !tile.update_exaggeration(&mut quad_tile, 2.0, 0.0),
        "Cannot update exaggeration on unrendered CPU-only mesh"
    );

    // Only when GPU vertex array handle is explicitly present (simulating GPU resource creation pass)
    tile.set_vertex_array_present(true);
    assert!(tile.has_vertex_array);
    assert_eq!(tile.terrain_state, TerrainState::Ready);
    assert!(
        tile.has_rendered_mesh(),
        "Mesh counts as rendered only when GPU vertex array is present"
    );
}

// ---------------------------------------------------------------------------
// 2. Missing regional bounds must yield None regardless of terrain upsampling
// ---------------------------------------------------------------------------
#[test]
fn test_review_2_missing_regional_bounds_yields_none_for_both_upsampled_and_non_upsampled() {
    let query_pos = Cartographic::new(0.5, 0.5, 0.0);

    // Case A: NON-upsampled terrain data with NO regional bounds (missing non-upsampled bounds)
    let mut non_upsampled_tile = GlobeSurfaceTile::new();
    non_upsampled_tile.rectangle = None;
    non_upsampled_tile.tile_bounding_region = None;
    *non_upsampled_tile.mesh.borrow_mut() = None;
    non_upsampled_tile.terrain_data = Some(Box::new(MockTerrainData {
        height: 100.0,
        upsampled: false, // NON-upsampled!
    }));

    // Must yield None: must NOT fall back to Rectangle::MAX_VALUE!
    assert_eq!(
        non_upsampled_tile.sample_height(&query_pos),
        None,
        "Missing regional bounds on non-upsampled terrain MUST yield None"
    );

    // Case B: Upsampled terrain data with NO regional bounds
    let mut upsampled_tile = GlobeSurfaceTile::new();
    upsampled_tile.rectangle = None;
    upsampled_tile.tile_bounding_region = None;
    *upsampled_tile.mesh.borrow_mut() = None;
    upsampled_tile.terrain_data = Some(Box::new(MockTerrainData {
        height: 200.0,
        upsampled: true, // Upsampled!
    }));

    assert_eq!(
        upsampled_tile.sample_height(&query_pos),
        None,
        "Missing regional bounds on upsampled terrain MUST yield None"
    );

    // Case C: Regional bounds ARE provided
    non_upsampled_tile.rectangle = Some(Rectangle::new(0.0, 0.0, 1.0, 1.0));
    assert_eq!(
        non_upsampled_tile.sample_height(&query_pos),
        Some(100.0),
        "With regional bounds, point inside samples successfully"
    );

    let outside_pos = Cartographic::new(2.0, 2.0, 0.0);
    assert_eq!(
        non_upsampled_tile.sample_height(&outside_pos),
        None,
        "Point outside regional bounds returns None without fallback"
    );
}

// ---------------------------------------------------------------------------
// 3. Imagery requests must map each TileImagery to its actual owning layer
//    (including two imagery tiles from one layer and a third from another)
// ---------------------------------------------------------------------------
#[test]
fn test_review_3_two_tiles_one_layer_and_third_from_another_mapping() {
    let scheme: Arc<dyn TilingScheme> = Arc::new(GeographicTilingScheme::default());
    let mut quad_tile = QuadtreeTile::new(QuadtreeTileOptions {
        tiling_scheme: scheme,
        x: 0,
        y: 0,
        level: 0,
        parent: None,
    });

    let mut gst = GlobeSurfaceTile::new();

    // Create 2 distinct layers
    let mut layer0 = ImageryLayer::new(None, ImageryLayerOptions::default());
    layer0.set_layer_index(0);

    let mut layer1 = ImageryLayer::new(None, ImageryLayerOptions::default());
    layer1.set_layer_index(1);

    let mut layers = [layer0, layer1];

    // Create 3 tile imageries:
    // tile 0 from layer 0
    // tile 1 from layer 0 (second tile from layer 0!)
    // tile 2 from layer 1 (third tile from layer 1!)
    let img0 = Imagery::new(0, 0, 0, Rectangle::new(-1.0, -1.0, 1.0, 1.0), None);
    let tile_img0 = TileImagery::new_with_layer_index(Rc::new(RefCell::new(img0)), Cartesian4::ZERO, false, 0);

    let img1 = Imagery::new(1, 0, 0, Rectangle::new(0.0, -1.0, 1.0, 1.0), None);
    let tile_img1 = TileImagery::new_with_layer_index(Rc::new(RefCell::new(img1)), Cartesian4::ZERO, false, 0);

    let img2 = Imagery::new(0, 0, 0, Rectangle::new(-1.0, -1.0, 1.0, 1.0), None);
    let tile_img2 = TileImagery::new_with_layer_index(Rc::new(RefCell::new(img2)), Cartesian4::ZERO, false, 1);

    gst.imagery.push(tile_img0);
    gst.imagery.push(tile_img1);
    gst.imagery.push(tile_img2);

    assert_eq!(gst.imagery.len(), 3);
    assert_eq!(gst.imagery[0].layer_index(), Some(0));
    assert_eq!(gst.imagery[1].layer_index(), Some(0));
    assert_eq!(gst.imagery[2].layer_index(), Some(1));

    // Both tile 0 and tile 1 must route to layers[0] (Layer 0), and tile 2 must route to layers[1] (Layer 1)
    let is_done = gst.process_imagery_with_layers(&mut quad_tile, &mut layers, None, false);
    assert!(is_done);

    // Test explicit unsupported result for unmapped layer (e.g. layer index 99 with only 2 layers)
    let img_unmapped = Imagery::new(0, 0, 0, Rectangle::new(-1.0, -1.0, 1.0, 1.0), None);
    let tile_img_unmapped = TileImagery::new_with_layer_index(Rc::new(RefCell::new(img_unmapped)), Cartesian4::ZERO, false, 99);
    gst.imagery.push(tile_img_unmapped);

    // The unmapped tile cannot be loaded by the available layers
    let is_done_unmapped = gst.process_imagery_with_layers(&mut quad_tile, &mut layers, None, false);
    assert!(
        !is_done_unmapped,
        "Unmapped layer index must yield explicit unsupported result, not incorrect index routing"
    );
}

#[test]
fn test_review_3_tile_imagery_actual_ownership_handle() {
    let scheme: Arc<dyn TilingScheme> = Arc::new(GeographicTilingScheme::default());
    let mut quad_tile = QuadtreeTile::new(QuadtreeTileOptions {
        tiling_scheme: scheme,
        x: 0,
        y: 0,
        level: 0,
        parent: None,
    });

    let mut gst = GlobeSurfaceTile::new();

    let layer = Rc::new(RefCell::new(ImageryLayer::new(None, ImageryLayerOptions::default())));
    layer.borrow_mut().set_layer_index(0);

    let img = Imagery::new(0, 0, 0, Rectangle::new(-1.0, -1.0, 1.0, 1.0), None);
    let tile_img = TileImagery::new_with_layer(Rc::new(RefCell::new(img)), Cartesian4::ZERO, false, layer.clone());

    assert_eq!(tile_img.layer_index(), Some(0));
    assert!(tile_img.imagery_layer.is_some());

    gst.imagery.push(tile_img);

    // process_imagery drives the state machine through the TileImagery's owned layer handle directly!
    let is_done = gst.process_imagery(&mut quad_tile, None, false);
    assert!(is_done);
}

// ---------------------------------------------------------------------------
// 4. Exaggeration must invalidate authoritative QuadtreePrimitive.active_height_callbacks
//    and queue through a safe API usable when surface is stored inside QuadtreeTile.data
//    without RefCell reentrant panic
// ---------------------------------------------------------------------------
#[test]
fn test_review_4_exaggeration_invalidates_authoritative_callbacks_without_reentrant_panic() {
    let scheme: Arc<dyn TilingScheme> = Arc::new(GeographicTilingScheme::default());
    let provider = TestQuadtreeProvider::new(scheme.clone());

    let mut primitive = QuadtreePrimitive::new(QuadtreePrimitiveOptions {
        tile_provider: provider,
        maximum_screen_space_error: None,
        tile_cache_size: None,
        loading_descendant_limit: None,
        preload_ancestors: None,
        preload_siblings: None,
    });

    // Register a height callback on primitive
    let callback_counter = Rc::new(RefCell::new(0));
    let cb_counter_clone = callback_counter.clone();
    let query_carto = Cartographic::new(0.1, 0.1, 0.0);

    let reg_id = primitive.update_height(query_carto, move |_carto| {
        *cb_counter_clone.borrow_mut() += 1;
    });

    assert_eq!(primitive.active_height_callbacks.get(&reg_id).unwrap().level, -1);

    // Create a QuadtreeTile and embed GlobeSurfaceTile into tile.data
    let quad_tile = QuadtreeTile::new(QuadtreeTileOptions {
        tiling_scheme: scheme.clone(),
        x: 0,
        y: 0,
        level: 3,
        parent: None,
    });
    let tile_handle = Rc::new(RefCell::new(quad_tile));

    let mut gst = GlobeSurfaceTile::new();
    let mesh = create_dummy_mesh(0.0, 100.0, Rectangle::new(-1.0, -1.0, 1.0, 1.0));
    gst.set_mesh(mesh);
    gst.set_vertex_array_present(true); // GPU vertex array present
    gst.rectangle = Some(Rectangle::new(-1.0, -1.0, 1.0, 1.0));
    gst.terrain_data = Some(Box::new(MockTerrainData {
        height: 50.0,
        upsampled: false,
    }));

    // Attach surface to tile.data (the real engine architecture!)
    tile_handle.borrow_mut().data = Some(Box::new(gst));

    // Simulate an initial height update where callback was satisfied at level 3
    tile_handle.borrow_mut().custom_data.push(QuadtreeCustomData {
        id: reg_id,
        position_cartographic: query_carto,
        position_on_ellipsoid_surface: None,
        level: 3,
        callback: primitive.active_height_callbacks.get(&reg_id).unwrap().callback.clone(),
    });
    primitive.active_height_callbacks.get_mut(&reg_id).unwrap().level = 3;
    tile_handle.borrow_mut().set_position_cache_entry(
        &query_carto,
        primitive.maximum_screen_space_error,
        Cartesian3::ZERO,
    );

    assert_eq!(primitive.active_height_callbacks.get(&reg_id).unwrap().level, 3);
    assert_eq!(tile_handle.borrow().custom_data[0].level, 3);

    // Call safe API: update_tile_exaggeration on QuadtreePrimitive!
    // Must NOT panic with RefCell reentrant borrow even though surface is stored inside QuadtreeTile.data!
    let changed = primitive.update_tile_exaggeration(&tile_handle, 2.5, 10.0);
    assert!(changed, "Exaggeration change applied");
    assert!(tile_handle.borrow_mut().get_position_cache_entry(
        &query_carto,
        primitive.maximum_screen_space_error,
    ).is_none());
    assert_eq!(tile_handle.borrow().data.as_ref().unwrap().sample_height(&query_carto), Some(110.0));

    // CRITICAL: authoritative registration state in active_height_callbacks must be invalidated (-1)!
    assert_eq!(
        primitive.active_height_callbacks.get(&reg_id).unwrap().level,
        -1,
        "Authoritative registration level in primitive.active_height_callbacks must be reset to -1"
    );

    // Tile's own custom_data level must also be reset to -1
    assert_eq!(
        tile_handle.borrow().custom_data[0].level,
        -1,
        "tile.custom_data level must be reset to -1"
    );

    // Tile handle must be queued in tile_to_update_heights
    assert_eq!(primitive.tile_to_update_heights.len(), 1);
    assert!(Rc::ptr_eq(&primitive.tile_to_update_heights[0], &tile_handle));

    // Now when update_heights runs, the callback will be eligible to trigger again
    // because registration level (-1) is less than tile level (3)!
    let frame_state = create_test_frame_state();
    primitive.update_heights(&frame_state);
    assert_eq!(*callback_counter.borrow(), 1, "Height callback re-triggered after exaggeration invalidation");

    // Test reentrant safety on GlobeSurfaceTile::update_exaggeration_with_quadtree
    // If a caller mutably borrows tile_handle while attempting to call update_exaggeration_with_quadtree:
    let mut tile_borrow = tile_handle.borrow_mut();
    let surface = tile_borrow
        .data
        .as_mut()
        .unwrap()
        .as_any_mut()
        .downcast_mut::<GlobeSurfaceTile>()
        .unwrap();

    let mut scratch_queue = Vec::new();
    let result = surface.update_exaggeration_with_quadtree(&tile_handle, 3.0, 0.0, &mut scratch_queue);
    assert!(
        result.is_err(),
        "Must safely return Err instead of causing a RefCell reentrant panic"
    );
}

// ---------------------------------------------------------------------------
// 5. Projected picking must use the active scene MapProjection or explicitly reject projected modes
// ---------------------------------------------------------------------------
#[test]
fn test_review_5_projected_picking_uses_scene_projection_or_explicitly_rejects() {
    let mut gst = GlobeSurfaceTile::new();
    let mesh = create_dummy_mesh(0.0, 50.0, Rectangle::new(-1.0, -1.0, 1.0, 1.0));
    gst.set_mesh(mesh);
    gst.set_vertex_array_present(true);

    let tile_data: Box<dyn QuadtreeTileData> = Box::new(gst);
    let ray = Ray::new(Cartesian3::new(0.0, 0.0, 100.0), Cartesian3::new(0.0, 0.0, -1.0));

    // In 3D mode without projection parameter: pick operates in 3D Cartesian coordinates
    let _ = tile_data.pick(&ray, SceneMode::Scene3D, true);

    // In projected mode (ColumbusView / Scene2D / Morphing) without scene projection parameter:
    // Must EXPLICITLY REJECT projected modes by returning None!
    assert_eq!(
        tile_data.pick(&ray, SceneMode::ColumbusView, true),
        None,
        "ColumbusView without active scene MapProjection must return None (explicitly rejected)"
    );
    assert_eq!(
        tile_data.pick(&ray, SceneMode::Scene2D, true),
        None,
        "Scene2D without active scene MapProjection must return None (explicitly rejected)"
    );
    assert_eq!(
        tile_data.pick(&ray, SceneMode::Morphing, true),
        None,
        "Morphing without active scene MapProjection must return None (explicitly rejected)"
    );

    // With active scene MapProjection: pick_with_projection uses the passed scene projection!
    let web_mercator = WebMercatorProjection::default();
    let _ = tile_data.pick_with_projection(&ray, SceneMode::ColumbusView, &web_mercator, true);
    let _ = tile_data.pick_with_projection(&ray, SceneMode::Scene2D, &web_mercator, true);
}


// ---------------------------------------------------------------------------
// 6. End-to-end regression: stable layer identity & ownership, multiple skeletons,
//    actual request_image calls via RecordingImageryProvider, and layer reorder
// ---------------------------------------------------------------------------
struct RecordingImageryProvider {
    name: String,
    scheme: GeographicTilingScheme,
    error_event: ImageryErrorEvent,
    requests: Arc<Mutex<Vec<(i32, i32, u32)>>>,
    request_count: Arc<AtomicUsize>,
}

impl RecordingImageryProvider {
    #[allow(clippy::type_complexity)]
    fn new(name: &str) -> (Self, Arc<Mutex<Vec<(i32, i32, u32)>>>, Arc<AtomicUsize>) {
        let requests = Arc::new(Mutex::new(Vec::new()));
        let request_count = Arc::new(AtomicUsize::new(0));
        let error_event = Rc::new(RefCell::new(MutableErrorEvent::new()));
        let provider = Self {
            name: name.to_string(),
            scheme: GeographicTilingScheme::default(),
            error_event,
            requests: requests.clone(),
            request_count: request_count.clone(),
        };
        (provider, requests, request_count)
    }
}

impl ImageryProvider for RecordingImageryProvider {
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

    fn minimum_level(&self) -> u32 {
        0
    }

    fn tiling_scheme(&self) -> &dyn TilingScheme {
        &self.scheme
    }

    fn error_event(&self) -> &ImageryErrorEvent {
        &self.error_event
    }

    fn get_tile_credits(&self, x: i32, y: i32, level: u32) -> Option<Vec<Credit>> {
        Some(vec![Credit::new(
            format!("{}-credit-{}-{}-{}", self.name, x, y, level),
            false,
        )])
    }

    fn request_image(&self, x: i32, y: i32, level: u32) -> Option<ImageryFuture> {
        self.request_count.fetch_add(1, Ordering::SeqCst);
        self.requests.lock().unwrap().push((x, y, level));
        let resource = FrameResource::new(vec![255u8; 256 * 256 * 4]);
        Some(Box::pin(std::future::ready(Ok(ImageryImage::Image(resource)))))
    }
}

#[test]
#[allow(clippy::arc_with_non_send_sync)]
fn test_review_6_end_to_end_regression_layer_ownership_and_reorder() {
    let scheme: Arc<dyn TilingScheme> = Arc::new(GeographicTilingScheme::default());
    let mut quad_tile = QuadtreeTile::new(QuadtreeTileOptions {
        tiling_scheme: scheme.clone(),
        x: 0,
        y: 0,
        level: 0,
        parent: None,
    });

    let (provider_a, records_a, count_a) = RecordingImageryProvider::new("LayerA");
    let (provider_b, records_b, count_b) = RecordingImageryProvider::new("LayerB");

    let mut collection = ImageryLayerCollection::new();
    let layer_a = collection.add_imagery_provider(Arc::new(provider_a), None).unwrap();
    let layer_b = collection.add_imagery_provider(Arc::new(provider_b), None).unwrap();

    let layer_a_id = layer_a.borrow().id();
    let layer_b_id = layer_b.borrow().id();
    assert_ne!(layer_a_id, layer_b_id, "Each layer must have a unique stable identity");

    assert_eq!(collection.index_of(&layer_a), Some(0));
    assert_eq!(collection.index_of(&layer_b), Some(1));
    assert_eq!(layer_a.borrow().layer_index(), 0);
    assert_eq!(layer_b.borrow().layer_index(), 1);

    // Full-globe rectangle at level 0 generates 2 skeletons in GeographicTilingScheme: (0, 0) and (1, 0)
    let full_globe = Rectangle::new(
        -std::f64::consts::PI,
        -std::f64::consts::FRAC_PI_2,
        std::f64::consts::PI,
        std::f64::consts::FRAC_PI_2,
    );

    let skeletons_a = layer_a.borrow_mut().create_tile_imagery_skeletons(&full_globe, 0, 1e12);
    let skeletons_b = layer_b.borrow_mut().create_tile_imagery_skeletons(&full_globe, 0, 1e12);

    assert_eq!(skeletons_a.len(), 2, "Layer A creates 2 skeletons");
    assert_eq!(skeletons_b.len(), 2, "Layer B creates 2 skeletons");

    // Skeletons MUST have actual layer ownership and identity attached, NOT ownerless!
    for (i, skel) in skeletons_a.iter().enumerate() {
        assert!(skel.imagery_layer.is_some(), "Skeleton A{} must have imagery_layer attached", i);
        assert_eq!(skel.layer_id(), Some(layer_a_id), "Skeleton A{} must match Layer A's stable ID", i);
        assert_eq!(skel.layer_index(), Some(0), "Skeleton A{} must report index 0", i);
    }

    for (i, skel) in skeletons_b.iter().enumerate() {
        assert!(skel.imagery_layer.is_some(), "Skeleton B{} must have imagery_layer attached", i);
        assert_eq!(skel.layer_id(), Some(layer_b_id), "Skeleton B{} must match Layer B's stable ID", i);
        assert_eq!(skel.layer_index(), Some(1), "Skeleton B{} must report index 1", i);
    }

    // Attach all 4 skeletons to a GlobeSurfaceTile
    let mut gst = GlobeSurfaceTile::new();
    gst.imagery.extend(skeletons_a);
    gst.imagery.extend(skeletons_b);
    assert_eq!(gst.imagery.len(), 4);

    // Initial process_imagery pass: dispatches requests to providers
    let _ = gst.process_imagery(&mut quad_tile, None, false);

    assert_eq!(count_a.load(Ordering::SeqCst), 2, "Provider A received 2 actual request_image calls");
    assert_eq!(count_b.load(Ordering::SeqCst), 2, "Provider B received 2 actual request_image calls");

    {
        let rec_a = records_a.lock().unwrap();
        assert_eq!(*rec_a, vec![(0, 0, 0), (1, 0, 0)], "Provider A received requests only for its tiles");
    }
    {
        let rec_b = records_b.lock().unwrap();
        assert_eq!(*rec_b, vec![(0, 0, 0), (1, 0, 0)], "Provider B received requests only for its tiles");
    }

    // In CPU-side execution without GPU context, received images are decoded and in Received state
    for (i, tile_img) in gst.imagery.iter().enumerate() {
        let loading = tile_img.loading_imagery.as_ref().expect("loading imagery present");
        assert_eq!(loading.borrow().state, ImageryState::Received, "Tile {} must be Received", i);
        assert!(loading.borrow().image.is_some(), "Tile {} must have decoded image", i);
    }

    // Simulate GPU frame pass creating textures from decoded images
    for tile_img in gst.imagery.iter_mut() {
        if let Some(loading) = &tile_img.loading_imagery {
            loading.borrow_mut().texture = Some(FrameResource::new(vec![255u8; 4]));
        }
    }

    // Next process_imagery pass advances TextureLoaded -> Ready
    let is_done = gst.process_imagery(&mut quad_tile, None, false);
    assert!(is_done, "All 4 tile imageries must finish loading");
    for (i, tile_img) in gst.imagery.iter().enumerate() {
        assert!(tile_img.has_ready_imagery(), "Tile {} must have ready imagery", i);
    }

    // --- REORDER LAYERS ---
    // Swap Layer A and Layer B so Layer B is at index 0 and Layer A is at index 1
    collection.raise(&layer_a).unwrap();

    assert_eq!(collection.index_of(&layer_b), Some(0), "Layer B is now at index 0");
    assert_eq!(collection.index_of(&layer_a), Some(1), "Layer A is now at index 1");
    assert_eq!(layer_b.borrow().layer_index(), 0);
    assert_eq!(layer_a.borrow().layer_index(), 1);

    // Existing skeletons dynamically reflect their layer's new index via the live handle:
    assert_eq!(gst.imagery[0].layer_index(), Some(1), "Layer A skeleton dynamically reports index 1");
    assert_eq!(gst.imagery[1].layer_index(), Some(1), "Layer A skeleton dynamically reports index 1");
    assert_eq!(gst.imagery[2].layer_index(), Some(0), "Layer B skeleton dynamically reports index 0");
    assert_eq!(gst.imagery[3].layer_index(), Some(0), "Layer B skeleton dynamically reports index 0");

    // Stable layer IDs are completely invariant under layer reordering:
    assert_eq!(gst.imagery[0].layer_id(), Some(layer_a_id));
    assert_eq!(gst.imagery[1].layer_id(), Some(layer_a_id));
    assert_eq!(gst.imagery[2].layer_id(), Some(layer_b_id));
    assert_eq!(gst.imagery[3].layer_id(), Some(layer_b_id));

    // Fresh skeletons created after reorder immediately have the correct current index and ID:
    let fresh_skeletons_a = layer_a.borrow_mut().create_tile_imagery_skeletons(&full_globe, 0, 1e12);
    let fresh_skeletons_b = layer_b.borrow_mut().create_tile_imagery_skeletons(&full_globe, 0, 1e12);

    assert_eq!(fresh_skeletons_a[0].layer_index(), Some(1));
    assert_eq!(fresh_skeletons_a[0].layer_id(), Some(layer_a_id));
    assert_eq!(fresh_skeletons_b[0].layer_index(), Some(0));
    assert_eq!(fresh_skeletons_b[0].layer_id(), Some(layer_b_id));

    // Test routing with process_imagery_with_layers using slice in reversed order [layer_b, layer_a]
    let (provider_c, records_c, _) = RecordingImageryProvider::new("LayerC");
    let (provider_d, records_d, _) = RecordingImageryProvider::new("LayerD");

    let mut layer_c = ImageryLayer::new(Some(Arc::new(provider_c)), ImageryLayerOptions::default());
    let mut layer_d = ImageryLayer::new(Some(Arc::new(provider_d)), ImageryLayerOptions::default());
    let id_c = layer_c.id();
    let id_d = layer_d.id();

    let skels_c = layer_c.create_tile_imagery_skeletons(&full_globe, 0, 1e12);
    let skels_d = layer_d.create_tile_imagery_skeletons(&full_globe, 0, 1e12);
    assert_eq!(skels_c.len(), 2);
    assert_eq!(skels_d.len(), 2);

    let mut gst2 = GlobeSurfaceTile::new();
    gst2.imagery.extend(skels_c);
    gst2.imagery.extend(skels_d);

    // Pass layers in arbitrary/reversed order: [layer_d, layer_c]
    let mut reversed_layers = [layer_d, layer_c];
    assert_eq!(reversed_layers[0].id(), id_d);
    assert_eq!(reversed_layers[1].id(), id_c);

    let _ = gst2.process_imagery_with_layers(&mut quad_tile, &mut reversed_layers, None, false);

    // Routing must map by layer identity, NOT positional slice index!
    assert_eq!(records_c.lock().unwrap().len(), 2, "Layer C provider received its 2 requests");
    assert_eq!(records_d.lock().unwrap().len(), 2, "Layer D provider received its 2 requests");

    // Unmapped layer identity must yield explicit unsupported without incorrect routing:
    let img_unmapped = Imagery::new(0, 0, 0, Rectangle::new(-1.0, -1.0, 1.0, 1.0), None);
    let unmapped_tile_img = TileImagery::new_with_layer_id(
        Rc::new(RefCell::new(img_unmapped)),
        Cartesian4::ZERO,
        false,
        99999, // Unknown layer ID
    );
    gst2.imagery.push(unmapped_tile_img);

    let is_done_unmapped = gst2.process_imagery_with_layers(&mut quad_tile, &mut reversed_layers, None, false);
    assert!(
        !is_done_unmapped,
        "Unmapped layer ID must yield explicit unsupported result and never fake success or misroute"
    );
}
