//! CesiumJS `QuadtreePrimitive`: level-of-detail surface tile renderer and LOD scheduler.
//!
//! Divides the globe surface into a hierarchical quadtree of tiles (`QuadtreeTile`),
//! providing partial visibility and screen-space error (SSE) evaluation,
//! priority-ordered loading (high, medium, low), ancestor and sibling preloading,
//! replacement cache trimming (`TileReplacementQueue`), and surface height query routing.
//!
//! Note on GPU and scheduler limits: tile mesh geometry rendering, WebGL draw commands,
//! shader pipelines, and asynchronous WebWorker mesh decoding are executed by the GPU
//! scene backend and terrain provider implementations rather than mock-allocated in this
//! CPU primitive state machine. Real terrain providers, scene integration and
//! full traversal parity remain unverified; load and height time budgets are
//! not enforced, and provider height sampling is an adaptation in projected modes.

use crate::core::{
    math::fog, Cartesian3, Cartographic, Event, Ray, Rectangle, TilingScheme, Visibility,
};
use crate::scene::camera::{Camera, CameraFrustum};
use crate::scene::frame_state::FrameState;
use crate::scene::quadtree_occluders::QuadtreeOccluders;
use crate::scene::quadtree_tile_load_state::QuadtreeTileLoadState;
use crate::scene::scene_mode::SceneMode;
use crate::scene::tile_replacement_queue::TileReplacementQueue;
use crate::scene::tile_selection_result::TileSelectionResult;
use super::quadtree_tile::{
    HeightCallback, QuadtreeCustomData, QuadtreeTile, QuadtreeTileHandle,
};
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

pub const DEFAULT_MAXIMUM_SCREEN_SPACE_ERROR: f64 = 2.0;
pub const DEFAULT_TILE_CACHE_SIZE: usize = 100;
pub const DEFAULT_LOADING_DESCENDANT_LIMIT: usize = 20;

/// Provider interface supplying tile data, visibility, and geometry error.
pub trait QuadtreeTileProvider {
    fn tiling_scheme(&self) -> Arc<dyn TilingScheme>;
    fn get_level_maximum_geometric_error(&self, level: u32) -> f64;
    fn load_tile(&mut self, frame_state: &FrameState, tile: &QuadtreeTileHandle);
    fn compute_tile_visibility(
        &self,
        tile: &QuadtreeTile,
        frame_state: &FrameState,
        occluders: Option<&QuadtreeOccluders>,
    ) -> Visibility;
    fn compute_distance_to_tile(&self, _tile: &QuadtreeTile, _frame_state: &FrameState) -> f64 {
        0.0
    }
    fn show_tile_this_frame(
        &mut self,
        tile: &QuadtreeTileHandle,
        frame_state: &mut FrameState,
    );
    fn can_refine(&self, _tile: &QuadtreeTile) -> bool {
        true
    }
    fn can_render_without_losing_detail(&self, _tile: &QuadtreeTile) -> bool {
        false
    }
    fn compute_tile_load_priority(&self, _tile: &QuadtreeTile, _frame_state: &FrameState) -> f64 {
        0.0
    }
    fn initialize(&mut self, _frame_state: &FrameState) {}
    fn begin_update(&mut self, _frame_state: &FrameState) {}
    fn end_update(&mut self, _frame_state: &FrameState) {}
    fn cancel_reprojections(&mut self) {}
    fn update(&mut self, _frame_state: &FrameState) {}
    fn destroy(&mut self) {}
    fn is_destroyed(&self) -> bool {
        false
    }
}

/// Computes the default geometric error for level zero of a quadtree.
pub fn compute_default_level_zero_maximum_geometric_error(
    tiling_scheme: &dyn TilingScheme,
) -> f64 {
    (tiling_scheme.ellipsoid().maximum_radius * 2.0 * std::f64::consts::PI * 0.25)
        / (65.0 * tiling_scheme.get_number_of_x_tiles_at_level(0) as f64)
}

/// Tracks traversal state details while visiting a quadtree tile subtree.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TraversalDetails {
    pub all_are_renderable: bool,
    pub any_were_rendered_last_frame: bool,
    pub not_yet_renderable_count: usize,
}

impl Default for TraversalDetails {
    fn default() -> Self {
        Self {
            all_are_renderable: true,
            any_were_rendered_last_frame: false,
            not_yet_renderable_count: 0,
        }
    }
}

/// Tracks traversal quad details for all four children of a parent tile.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct TraversalQuadDetails {
    pub southwest: TraversalDetails,
    pub southeast: TraversalDetails,
    pub northwest: TraversalDetails,
    pub northeast: TraversalDetails,
}

impl TraversalQuadDetails {
    pub fn combine(&self, result: &mut TraversalDetails) {
        result.all_are_renderable = self.southwest.all_are_renderable
            && self.southeast.all_are_renderable
            && self.northwest.all_are_renderable
            && self.northeast.all_are_renderable;
        result.any_were_rendered_last_frame = self.southwest.any_were_rendered_last_frame
            || self.southeast.any_were_rendered_last_frame
            || self.northwest.any_were_rendered_last_frame
            || self.northeast.any_were_rendered_last_frame;
        result.not_yet_renderable_count = self.southwest.not_yet_renderable_count
            + self.southeast.not_yet_renderable_count
            + self.northwest.not_yet_renderable_count
            + self.northeast.not_yet_renderable_count;
    }
}

/// Debug counters and flags for [`QuadtreePrimitive`].
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct QuadtreeDebug {
    pub enable_debug_output: bool,
    pub max_depth: i32,
    pub max_depth_visited: u32,
    pub tiles_visited: usize,
    pub tiles_culled: usize,
    pub tiles_rendered: usize,
    pub tiles_waiting_for_children: usize,
    pub suspend_lod_update: bool,
}

/// Registration record for an asynchronous height update callback.
#[derive(Clone)]
pub struct HeightCallbackRegistration {
    pub id: usize,
    pub position_cartographic: Cartographic,
    pub position_on_ellipsoid_surface: Option<Cartesian3>,
    pub level: i32,
    pub callback: Option<HeightCallback>,
}

/// Construction options for [`QuadtreePrimitive`].
pub struct QuadtreePrimitiveOptions<P: QuadtreeTileProvider> {
    pub tile_provider: P,
    pub maximum_screen_space_error: Option<f64>,
    pub tile_cache_size: Option<usize>,
    pub loading_descendant_limit: Option<usize>,
    pub preload_ancestors: Option<bool>,
    pub preload_siblings: Option<bool>,
}

/// Level-of-detail surface tile primitive.
pub struct QuadtreePrimitive<P: QuadtreeTileProvider> {
    pub tile_provider: P,
    pub maximum_screen_space_error: f64,
    pub tile_cache_size: usize,
    pub loading_descendant_limit: usize,
    pub preload_ancestors: bool,
    pub preload_siblings: bool,
    pub occluders: QuadtreeOccluders,
    pub debug: QuadtreeDebug,

    pub tile_load_progress_event: Event<usize>,
    pub last_tile_load_queue_length: usize,

    pub tile_replacement_queue: TileReplacementQueue<(u32, i32, i32)>,
    pub level_zero_tiles: Option<Vec<QuadtreeTileHandle>>,

    pub tiles_rendered_this_frame: Vec<QuadtreeTileHandle>,
    pub tiles_to_render: Vec<QuadtreeTileHandle>,

    pub tile_load_queue_high: Vec<QuadtreeTileHandle>,
    pub tile_load_queue_medium: Vec<QuadtreeTileHandle>,
    pub tile_load_queue_low: Vec<QuadtreeTileHandle>,

    pub load_queue_time_slice: f64,
    pub update_heights_time_slice: f64,
    pub tiles_invalidated: bool,

    pub add_height_callbacks: Vec<HeightCallbackRegistration>,
    pub remove_height_callbacks: Vec<HeightCallbackRegistration>,
    pub active_height_callbacks: HashMap<usize, HeightCallbackRegistration>,
    pub tile_to_update_heights: Vec<QuadtreeTileHandle>,

    pub camera_position_cartographic: Option<Cartographic>,
    pub camera_reference_frame_origin_cartographic: Option<Cartographic>,

    pub last_selection_frame_number: Option<u32>,
    pub next_height_callback_id: usize,
    pub destroyed: bool,
}

impl<P: QuadtreeTileProvider> QuadtreePrimitive<P> {
    /// Creates a new quadtree primitive.
    pub fn new(options: QuadtreePrimitiveOptions<P>) -> Self {
        let occluders = QuadtreeOccluders::new(options.tile_provider.tiling_scheme().ellipsoid());

        Self {
            tile_provider: options.tile_provider,
            maximum_screen_space_error: options
                .maximum_screen_space_error
                .unwrap_or(DEFAULT_MAXIMUM_SCREEN_SPACE_ERROR),
            tile_cache_size: options
                .tile_cache_size
                .unwrap_or(DEFAULT_TILE_CACHE_SIZE),
            loading_descendant_limit: options
                .loading_descendant_limit
                .unwrap_or(DEFAULT_LOADING_DESCENDANT_LIMIT),
            preload_ancestors: options.preload_ancestors.unwrap_or(true),
            preload_siblings: options.preload_siblings.unwrap_or(false),
            occluders,
            debug: QuadtreeDebug::default(),
            tile_load_progress_event: Event::new(),
            last_tile_load_queue_length: 0,
            tile_replacement_queue: TileReplacementQueue::new(),
            level_zero_tiles: None,
            tiles_rendered_this_frame: Vec::new(),
            tiles_to_render: Vec::new(),
            tile_load_queue_high: Vec::new(),
            tile_load_queue_medium: Vec::new(),
            tile_load_queue_low: Vec::new(),
            load_queue_time_slice: 5.0,
            update_heights_time_slice: 2.0,
            tiles_invalidated: false,
            add_height_callbacks: Vec::new(),
            remove_height_callbacks: Vec::new(),
            active_height_callbacks: HashMap::new(),
            tile_to_update_heights: Vec::new(),
            camera_position_cartographic: None,
            camera_reference_frame_origin_cartographic: None,
            last_selection_frame_number: None,
            next_height_callback_id: 1,
            destroyed: false,
        }
    }

    pub fn tile_provider(&self) -> &P {
        &self.tile_provider
    }

    pub fn tile_provider_mut(&mut self) -> &mut P {
        &mut self.tile_provider
    }

    pub fn occluders(&self) -> &QuadtreeOccluders {
        &self.occluders
    }

    pub fn occluders_mut(&mut self) -> &mut QuadtreeOccluders {
        &mut self.occluders
    }

    /// Marks all tiles as invalidated. They will be freed on the next frame.
    pub fn invalidate_all_tiles(&mut self) {
        self.tiles_invalidated = true;
    }

    fn invalidate_all_tiles_now(&mut self) {
        self.tile_replacement_queue = TileReplacementQueue::new();
        self.clear_tile_load_queue();

        if let Some(level_zero_tiles) = self.level_zero_tiles.take() {
            for tile in level_zero_tiles {
                for data in std::mem::take(&mut tile.borrow_mut().custom_data) {
                    if let Some(active) = self.active_height_callbacks.get_mut(&data.id) {
                        active.level = -1;
                        self.add_height_callbacks.push(active.clone());
                    } else if data.callback.is_some() {
                        self.add_height_callbacks.push(HeightCallbackRegistration {
                            id: data.id,
                            position_cartographic: data.position_cartographic,
                            position_on_ellipsoid_surface: data.position_on_ellipsoid_surface,
                            level: -1,
                            callback: data.callback,
                        });
                    }
                }
                tile.borrow_mut().free_resources();
            }
        }

        self.tile_provider.cancel_reprojections();
    }

    fn clear_tile_load_queue(&mut self) {
        self.debug.max_depth = 0;
        self.debug.max_depth_visited = 0;
        self.debug.tiles_visited = 0;
        self.debug.tiles_culled = 0;
        self.debug.tiles_rendered = 0;
        self.debug.tiles_waiting_for_children = 0;

        self.tile_load_queue_high.clear();
        self.tile_load_queue_medium.clear();
        self.tile_load_queue_low.clear();
    }

    /// Invokes a specified function for each quadtree tile that is loaded.
    pub fn for_each_loaded_tile(&self, mut tile_function: impl FnMut(&QuadtreeTile)) {
        if let Some(level_zero_tiles) = &self.level_zero_tiles {
            let mut stack = level_zero_tiles.clone();
            while let Some(tile_handle) = stack.pop() {
                let b = tile_handle.borrow();
                if b.state != QuadtreeTileLoadState::Start {
                    tile_function(&b);
                }
                if let Some(nw) = &b.northwest_child {
                    stack.push(nw.clone());
                }
                if let Some(ne) = &b.northeast_child {
                    stack.push(ne.clone());
                }
                if let Some(sw) = &b.southwest_child {
                    stack.push(sw.clone());
                }
                if let Some(se) = &b.southeast_child {
                    stack.push(se.clone());
                }
            }
        }
    }

    /// Invokes a specified function for each quadtree tile that was rendered in the most recent frame.
    pub fn for_each_rendered_tile(&self, mut tile_function: impl FnMut(&QuadtreeTile)) {
        for tile_handle in &self.tiles_rendered_this_frame {
            tile_function(&tile_handle.borrow());
        }
    }

    /// Registers a callback to be notified when a new tile covering `cartographic` is loaded.
    pub fn update_height(
        &mut self,
        cartographic: Cartographic,
        callback: impl FnMut(Cartographic) + 'static,
    ) -> usize {
        let id = self.next_height_callback_id;
        self.next_height_callback_id += 1;

        let registration = HeightCallbackRegistration {
            id,
            position_cartographic: cartographic,
            position_on_ellipsoid_surface: None,
            level: -1,
            callback: Some(Rc::new(RefCell::new(callback))),
        };

        self.add_height_callbacks.push(registration.clone());
        self.active_height_callbacks.insert(id, registration);
        id
    }

    /// Removes a previously registered height callback by ID.
    pub fn remove_height_callback(&mut self, id: usize) {
        if let Some(pos) = self.add_height_callbacks.iter().position(|r| r.id == id) {
            let mut reg = self.add_height_callbacks.remove(pos);
            reg.callback = None;
            self.remove_height_callbacks.push(reg);
            self.active_height_callbacks.remove(&id);
        } else if let Some(mut reg) = self.active_height_callbacks.remove(&id) {
            reg.callback = None;
            self.remove_height_callbacks.push(reg);
        }
    }

    /// Authoritative entrypoint to update vertical exaggeration for a tile handle.
    ///
    /// - Safely updates the tile exaggeration without reentrant `RefCell` panics.
    /// - Invalidates the authoritative registration level (`level = -1`) in
    ///   `self.active_height_callbacks` for all custom data on the tile.
    /// - Re-enqueues `tile_handle` into `self.tile_to_update_heights` so height queries are recomputed.
    pub fn update_tile_exaggeration(
        &mut self,
        tile_handle: &QuadtreeTileHandle,
        vertical_exaggeration: f64,
        relative_height: f64,
    ) -> bool {
        let changed = {
            let mut tile = tile_handle.borrow_mut();
            tile.update_exaggeration(vertical_exaggeration, relative_height)
        };

        if changed {
            let tile = tile_handle.borrow();
            for data in tile.custom_data.iter().chain(tile.added_custom_data.iter()) {
                if let Some(reg) = self.active_height_callbacks.get_mut(&data.id) {
                    reg.level = -1;
                }
            }
            drop(tile);

            if !self.tile_to_update_heights.iter().any(|h| Rc::ptr_eq(h, tile_handle)) {
                self.tile_to_update_heights.push(Rc::clone(tile_handle));
            }
        }

        changed
    }

    /// Prepares state for a new frame.
    pub fn begin_frame(&mut self, frame_state: &FrameState) {
        if !frame_state.passes.render {
            return;
        }

        if self.tiles_invalidated {
            self.invalidate_all_tiles_now();
            self.tiles_invalidated = false;
        }

        self.tile_provider.initialize(frame_state);
        self.clear_tile_load_queue();

        if self.debug.suspend_lod_update {
            return;
        }

        self.tile_replacement_queue.mark_start_of_render_frame();
        self.tiles_rendered_this_frame.clear();
    }

    /// Renders selected quadtree tiles for this frame.
    pub fn render(&mut self, frame_state: &mut FrameState, camera: &mut Camera) {
        if frame_state.passes.render {
            self.tile_provider.begin_update(frame_state);

            self.select_tiles_for_rendering(frame_state, camera);
            self.create_render_commands_for_selected_tiles(frame_state);

            self.tile_provider.end_update(frame_state);
        }
    }

    /// Completes the frame, processing tile load queues and height updates.
    pub fn end_frame(&mut self, frame_state: &mut FrameState) {
        if !frame_state.passes.render || frame_state.mode == SceneMode::Morphing {
            return;
        }

        self.process_tile_load_queue(frame_state);
        self.update_heights(frame_state);
        self.update_tile_load_progress(frame_state);
    }

    fn queue_tile_load_internal(
        provider: &P,
        queue: &mut Vec<QuadtreeTileHandle>,
        tile: &QuadtreeTileHandle,
        frame_state: &FrameState,
    ) {
        if !tile.borrow().needs_loading() {
            return;
        }
        let priority = provider.compute_tile_load_priority(&tile.borrow(), frame_state);
        tile.borrow_mut().load_priority = priority;
        queue.push(tile.clone());
    }

    fn add_tile_to_render_list(&mut self, tile: &QuadtreeTileHandle) {
        self.tiles_to_render.push(tile.clone());
        if !self.tiles_rendered_this_frame.iter().any(|t| Rc::ptr_eq(t, tile)) {
            self.tiles_rendered_this_frame.push(tile.clone());
        }
    }

    fn contains_needed_position(&self, rectangle: &Rectangle) -> bool {
        if let Some(pos) = &self.camera_position_cartographic {
            if rectangle.contains(pos) {
                return true;
            }
        }
        if let Some(pos) = &self.camera_reference_frame_origin_cartographic {
            if rectangle.contains(pos) {
                return true;
            }
        }
        false
    }

    /// Computes the estimated screen-space error (SSE) in pixels for a tile.
    pub fn screen_space_error(
        &self,
        frame_state: &FrameState,
        camera: &Camera,
        tile: &QuadtreeTile,
    ) -> f64 {
        let max_geometric_error = self
            .tile_provider
            .get_level_maximum_geometric_error(tile.level);
        let distance = tile.distance;
        let height = frame_state.drawing_buffer_height as f64;
        let pixel_ratio = if frame_state.pixel_ratio > 0.0 {
            frame_state.pixel_ratio
        } else {
            1.0
        };

        if frame_state.mode == SceneMode::Scene2D {
            let max_dim = (frame_state
                .drawing_buffer_width
                .max(frame_state.drawing_buffer_height) as f64)
                .max(1.0);
            let pixel_size = match camera.frustum {
                CameraFrustum::Orthographic(f) => {
                    let off = f.off_center_frustum();
                    ((off.top - off.bottom).max(off.right - off.left)) / max_dim
                }
                CameraFrustum::OrthographicOffCenter(f) => {
                    ((f.top - f.bottom).max(f.right - f.left)) / max_dim
                }
                _ => 1.0,
            };
            let mut error = max_geometric_error / pixel_size;
            if frame_state.fog.enabled && frame_state.mode != SceneMode::Scene2D {
                error -= fog(tile.distance, frame_state.fog.density)
                    * frame_state.fog.sse;
            }
            return error / pixel_ratio;
        }

        match camera.frustum {
            CameraFrustum::Orthographic(f) => {
                let max_dim = (frame_state
                    .drawing_buffer_width
                    .max(frame_state.drawing_buffer_height) as f64)
                    .max(1.0);
                let off = f.off_center_frustum();
                let pixel_size = ((off.top - off.bottom).max(off.right - off.left)) / max_dim;
                let mut error = max_geometric_error / pixel_size;
                if frame_state.fog.enabled && frame_state.mode != SceneMode::Scene2D {
                    error -= fog(tile.distance, frame_state.fog.density)
                        * frame_state.fog.sse;
                }
                error / pixel_ratio
            }
            CameraFrustum::OrthographicOffCenter(f) => {
                let max_dim = (frame_state
                    .drawing_buffer_width
                    .max(frame_state.drawing_buffer_height) as f64)
                    .max(1.0);
                let pixel_size = ((f.top - f.bottom).max(f.right - f.left)) / max_dim;
                let mut error = max_geometric_error / pixel_size;
                if frame_state.fog.enabled && frame_state.mode != SceneMode::Scene2D {
                    error -= fog(tile.distance, frame_state.fog.density)
                        * frame_state.fog.sse;
                }
                error / pixel_ratio
            }
            CameraFrustum::Perspective(f) => {
                let sse_denominator = f.sse_denominator();
                let mut error = (max_geometric_error * height) / (distance * sse_denominator);
                if frame_state.fog.enabled {
                    error -= fog(distance, frame_state.fog.density)
                        * frame_state.fog.sse;
                }
                error / pixel_ratio
            }
        }
    }

    /// Performs tile selection across the quadtree hierarchy for the current frame.
    pub fn select_tiles_for_rendering(
        &mut self,
        frame_state: &FrameState,
        camera: &mut Camera,
    ) {
        if self.debug.suspend_lod_update {
            return;
        }

        self.tiles_to_render.clear();

        if self.level_zero_tiles.is_none() {
            let scheme = self.tile_provider.tiling_scheme();
            self.level_zero_tiles = Some(QuadtreeTile::create_level_zero_tiles(scheme));
        }

        self.occluders
            .ellipsoid_mut()
            .set_camera_position(camera.position);

        let level_zero_tiles = self.level_zero_tiles.as_mut().unwrap();

        let camera_pos_carto = camera
            .position_cartographic()
            .unwrap_or(Cartographic::ZERO);
        self.camera_position_cartographic = Some(camera_pos_carto);

        let camera_origin = camera.transform().get_translation();
        self.camera_reference_frame_origin_cartographic = self
            .tile_provider
            .tiling_scheme()
            .ellipsoid()
            .cartesian_to_cartographic(camera_origin);

        // Sort root tiles near-to-far relative to camera cartographic center
        let dist_sq = |tile: &QuadtreeTileHandle| {
            let center = tile.borrow().rectangle.center();
            let dlon = center.longitude - camera_pos_carto.longitude;
            let dlat = center.latitude - camera_pos_carto.latitude;
            dlon * dlon + dlat * dlat
        };
        level_zero_tiles.sort_by(|a, b| {
            dist_sq(a)
                .partial_cmp(&dist_sq(b))
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        // Distribute added / removed custom height callbacks
        let added = std::mem::take(&mut self.add_height_callbacks);
        for data in added {
            if let Some(tile) = level_zero_tiles
                .iter()
                .find(|t| t.borrow().rectangle.contains(&data.position_cartographic))
            {
                tile.borrow_mut().added_custom_data.push(QuadtreeCustomData {
                    id: data.id,
                    position_cartographic: data.position_cartographic,
                    position_on_ellipsoid_surface: data.position_on_ellipsoid_surface,
                    level: data.level,
                    callback: data.callback,
                });
            }
        }

        let removed = std::mem::take(&mut self.remove_height_callbacks);
        for data in removed {
            if let Some(tile) = level_zero_tiles
                .iter()
                .find(|t| t.borrow().rectangle.contains(&data.position_cartographic))
            {
                tile.borrow_mut()
                    .removed_custom_data
                    .push(QuadtreeCustomData {
                        id: data.id,
                        position_cartographic: data.position_cartographic,
                        position_on_ellipsoid_surface: data.position_on_ellipsoid_surface,
                        level: data.level,
                        callback: None,
                    });
            }
        }

        for tile in level_zero_tiles.iter() {
            QuadtreeTile::update_custom_data(tile);
        }

        let use_occluders = level_zero_tiles.len() > 1;

        let roots = level_zero_tiles.clone();
        for tile in &roots {
            let coord = {
                let b = tile.borrow();
                (b.level, b.x, b.y)
            };
            self.tile_replacement_queue.mark_tile_rendered(coord);

            if !tile.borrow().renderable {
                Self::queue_tile_load_internal(
                    &self.tile_provider,
                    &mut self.tile_load_queue_high,
                    tile,
                    frame_state,
                );
                self.debug.tiles_waiting_for_children += 1;
            } else {
                let mut traversal_details = TraversalDetails::default();
                self.visit_if_visible(
                    frame_state,
                    camera,
                    tile,
                    use_occluders,
                    false,
                    &mut traversal_details,
                );
            }
        }

        self.last_selection_frame_number = Some(frame_state.frame_number);
    }

    fn visit_if_visible(
        &mut self,
        frame_state: &FrameState,
        camera: &Camera,
        tile: &QuadtreeTileHandle,
        use_occluders: bool,
        ancestor_meets_sse: bool,
        traversal_details: &mut TraversalDetails,
    ) {
        let occluders = if use_occluders {
            Some(&self.occluders)
        } else {
            None
        };
        let vis = self.tile_provider.compute_tile_visibility(
            &tile.borrow(),
            frame_state,
            occluders,
        );

        let distance = self
            .tile_provider
            .compute_distance_to_tile(&tile.borrow(), frame_state);
        tile.borrow_mut().distance = distance;

        if vis != Visibility::None {
            return self.visit_tile(
                frame_state,
                camera,
                tile,
                ancestor_meets_sse,
                traversal_details,
            );
        }

        self.debug.tiles_culled += 1;
        let coord = {
            let b = tile.borrow();
            (b.level, b.x, b.y)
        };
        self.tile_replacement_queue.mark_tile_rendered(coord);

        traversal_details.all_are_renderable = true;
        traversal_details.any_were_rendered_last_frame = false;
        traversal_details.not_yet_renderable_count = 0;

        let needed = self.contains_needed_position(&tile.borrow().rectangle);
        if needed {
            if tile.borrow().data.is_none() {
                Self::queue_tile_load_internal(
                    &self.tile_provider,
                    &mut self.tile_load_queue_medium,
                    tile,
                    frame_state,
                );
            }

            let last_frame = self.last_selection_frame_number;
            let last_res = if tile.borrow().last_selection_result_frame == last_frame {
                tile.borrow().last_selection_result
            } else {
                TileSelectionResult::None
            };

            if last_res != TileSelectionResult::CulledButNeeded
                && last_res != TileSelectionResult::Rendered
            {
                self.tile_to_update_heights.push(tile.clone());
            }

            tile.borrow_mut().last_selection_result = TileSelectionResult::CulledButNeeded;
        } else if self.preload_siblings || tile.borrow().level == 0 {
            Self::queue_tile_load_internal(
                &self.tile_provider,
                &mut self.tile_load_queue_low,
                tile,
                frame_state,
            );
            tile.borrow_mut().last_selection_result = TileSelectionResult::Culled;
        } else {
            tile.borrow_mut().last_selection_result = TileSelectionResult::Culled;
        }

        tile.borrow_mut().last_selection_result_frame = Some(frame_state.frame_number);
    }

    fn visit_tile(
        &mut self,
        frame_state: &FrameState,
        camera: &Camera,
        tile: &QuadtreeTileHandle,
        mut ancestor_meets_sse: bool,
        traversal_details: &mut TraversalDetails,
    ) {
        self.debug.tiles_visited += 1;
        let coord = {
            let b = tile.borrow();
            (b.level, b.x, b.y)
        };
        self.tile_replacement_queue.mark_tile_rendered(coord);
        QuadtreeTile::update_custom_data(tile);

        if tile.borrow().level > self.debug.max_depth_visited {
            self.debug.max_depth_visited = tile.borrow().level;
        }

        let meets_sse = self.screen_space_error(frame_state, camera, &tile.borrow())
            < self.maximum_screen_space_error;

        let last_frame = self.last_selection_frame_number;
        let last_frame_selection_result = if tile.borrow().last_selection_result_frame == last_frame
        {
            tile.borrow().last_selection_result
        } else {
            TileSelectionResult::None
        };

        if meets_sse || ancestor_meets_sse {
            let one_rendered_last_frame =
                TileSelectionResult::original_result(last_frame_selection_result.value())
                    == TileSelectionResult::Rendered.value();
            let two_culled_or_not_visited =
                TileSelectionResult::original_result(last_frame_selection_result.value())
                    == TileSelectionResult::Culled.value()
                    || last_frame_selection_result == TileSelectionResult::None;
            let three_completely_loaded = tile.borrow().state == QuadtreeTileLoadState::Done;

            let mut renderable =
                one_rendered_last_frame || two_culled_or_not_visited || three_completely_loaded;

            if !renderable {
                renderable = self
                    .tile_provider
                    .can_render_without_losing_detail(&tile.borrow());
            }

            if renderable {
                if meets_sse {
                    Self::queue_tile_load_internal(
                        &self.tile_provider,
                        &mut self.tile_load_queue_medium,
                        tile,
                        frame_state,
                    );
                }
                self.add_tile_to_render_list(tile);

                let is_renderable = tile.borrow().renderable;
                traversal_details.all_are_renderable = is_renderable;
                traversal_details.any_were_rendered_last_frame =
                    last_frame_selection_result == TileSelectionResult::Rendered;
                traversal_details.not_yet_renderable_count = if is_renderable { 0 } else { 1 };

                tile.borrow_mut().last_selection_result_frame = Some(frame_state.frame_number);
                tile.borrow_mut().last_selection_result = TileSelectionResult::Rendered;

                if !traversal_details.any_were_rendered_last_frame {
                    self.tile_to_update_heights.push(tile.clone());
                }
                return;
            }

            ancestor_meets_sse = true;
            if meets_sse {
                Self::queue_tile_load_internal(
                    &self.tile_provider,
                    &mut self.tile_load_queue_high,
                    tile,
                    frame_state,
                );
            }
        }

        if self.tile_provider.can_refine(&tile.borrow()) {
            let nw = QuadtreeTile::northwest_child(tile);
            let ne = QuadtreeTile::northeast_child(tile);
            let sw = QuadtreeTile::southwest_child(tile);
            let se = QuadtreeTile::southeast_child(tile);

            let all_are_upsampled = sw.borrow().upsampled_from_parent
                && se.borrow().upsampled_from_parent
                && nw.borrow().upsampled_from_parent
                && ne.borrow().upsampled_from_parent;

            if all_are_upsampled {
                self.add_tile_to_render_list(tile);
                Self::queue_tile_load_internal(
                    &self.tile_provider,
                    &mut self.tile_load_queue_medium,
                    tile,
                    frame_state,
                );

                for child in [&sw, &se, &nw, &ne] {
                    let b = child.borrow();
                    self.tile_replacement_queue
                        .mark_tile_rendered((b.level, b.x, b.y));
                }

                let is_renderable = tile.borrow().renderable;
                traversal_details.all_are_renderable = is_renderable;
                traversal_details.any_were_rendered_last_frame =
                    last_frame_selection_result == TileSelectionResult::Rendered;
                traversal_details.not_yet_renderable_count = if is_renderable { 0 } else { 1 };

                tile.borrow_mut().last_selection_result_frame = Some(frame_state.frame_number);
                tile.borrow_mut().last_selection_result = TileSelectionResult::Rendered;

                if !traversal_details.any_were_rendered_last_frame {
                    self.tile_to_update_heights.push(tile.clone());
                }
                return;
            }

            tile.borrow_mut().last_selection_result_frame = Some(frame_state.frame_number);
            tile.borrow_mut().last_selection_result = TileSelectionResult::Refined;

            let first_rendered_descendant_index = self.tiles_to_render.len();
            let load_index_low = self.tile_load_queue_low.len();
            let load_index_medium = self.tile_load_queue_medium.len();
            let load_index_high = self.tile_load_queue_high.len();
            let tiles_to_update_heights_index = self.tile_to_update_heights.len();

            self.visit_visible_children_near_to_far(
                &sw,
                &se,
                &nw,
                &ne,
                frame_state,
                camera,
                ancestor_meets_sse,
                traversal_details,
            );

            if first_rendered_descendant_index != self.tiles_to_render.len() {
                let all_are_renderable = traversal_details.all_are_renderable;
                let any_were_rendered_last_frame = traversal_details.any_were_rendered_last_frame;
                let not_yet_renderable_count = traversal_details.not_yet_renderable_count;
                let mut queued_for_load = false;

                if !all_are_renderable && !any_were_rendered_last_frame {
                    // Kick descendants
                    for i in first_rendered_descendant_index..self.tiles_to_render.len() {
                        let work_tile = self.tiles_to_render[i].clone();
                        let mut curr: Option<QuadtreeTileHandle> = Some(work_tile);
                        while let Some(w) = curr {
                            if Rc::ptr_eq(&w, tile) {
                                break;
                            }
                            if w.borrow().last_selection_result
                                == TileSelectionResult::RenderedAndKicked
                                || w.borrow().last_selection_result
                                    == TileSelectionResult::RefinedAndKicked
                            {
                                break;
                            }
                            let kicked_val =
                                TileSelectionResult::kick(w.borrow().last_selection_result.value());
                            if let Some(kicked) = TileSelectionResult::from_value(kicked_val) {
                                w.borrow_mut().last_selection_result = kicked;
                            }
                            curr = w.borrow().parent.as_ref().and_then(|p| p.upgrade());
                        }
                    }

                    self.tiles_to_render.truncate(first_rendered_descendant_index);
                    self.tile_to_update_heights
                        .truncate(tiles_to_update_heights_index);
                    self.add_tile_to_render_list(tile);

                    tile.borrow_mut().last_selection_result = TileSelectionResult::Rendered;

                    let was_rendered_last_frame =
                        last_frame_selection_result == TileSelectionResult::Rendered;
                    if !was_rendered_last_frame
                        && not_yet_renderable_count > self.loading_descendant_limit
                    {
                        self.tile_load_queue_low.truncate(load_index_low);
                        self.tile_load_queue_medium.truncate(load_index_medium);
                        self.tile_load_queue_high.truncate(load_index_high);

                        Self::queue_tile_load_internal(
                            &self.tile_provider,
                            &mut self.tile_load_queue_high,
                            tile,
                            frame_state,
                        );
                        traversal_details.not_yet_renderable_count =
                            if tile.borrow().renderable { 0 } else { 1 };
                        queued_for_load = true;
                    }

                    traversal_details.all_are_renderable = tile.borrow().renderable;
                    traversal_details.any_were_rendered_last_frame = was_rendered_last_frame;

                    if !was_rendered_last_frame {
                        self.tile_to_update_heights.push(tile.clone());
                    }

                    self.debug.tiles_waiting_for_children += 1;
                }

                if self.preload_ancestors && !queued_for_load {
                    Self::queue_tile_load_internal(
                        &self.tile_provider,
                        &mut self.tile_load_queue_low,
                        tile,
                        frame_state,
                    );
                }
            } else {
                Self::queue_tile_load_internal(
                    &self.tile_provider,
                    &mut self.tile_load_queue_high,
                    tile,
                    frame_state,
                );
                self.debug.tiles_waiting_for_children += 1;
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn visit_visible_children_near_to_far(
        &mut self,
        sw: &QuadtreeTileHandle,
        se: &QuadtreeTileHandle,
        nw: &QuadtreeTileHandle,
        ne: &QuadtreeTileHandle,
        frame_state: &FrameState,
        camera: &Camera,
        ancestor_meets_sse: bool,
        traversal_details: &mut TraversalDetails,
    ) {
        let mut quad_details = TraversalQuadDetails::default();

        let mut children = [sw.clone(), se.clone(), nw.clone(), ne.clone()];
        let dist = |t: &QuadtreeTileHandle| {
            self.tile_provider
                .compute_distance_to_tile(&t.borrow(), frame_state)
        };
        children.sort_by(|a, b| {
            dist(a)
                .partial_cmp(&dist(b))
                .unwrap_or(std::cmp::Ordering::Equal)
        });

        for child in &children {
            let mut details = TraversalDetails::default();
            self.visit_if_visible(
                frame_state,
                camera,
                child,
                false,
                ancestor_meets_sse,
                &mut details,
            );

            if Rc::ptr_eq(child, sw) {
                quad_details.southwest = details;
            } else if Rc::ptr_eq(child, se) {
                quad_details.southeast = details;
            } else if Rc::ptr_eq(child, nw) {
                quad_details.northwest = details;
            } else {
                quad_details.northeast = details;
            }
        }

        quad_details.combine(traversal_details);
    }

    fn create_render_commands_for_selected_tiles(&mut self, frame_state: &mut FrameState) {
        let tiles = self.tiles_to_render.clone();
        for tile in &tiles {
            self.tile_provider.show_tile_this_frame(tile, frame_state);
        }
    }

    /// Finds a tile in a level-zero tile slice hierarchy by coordinate `(level, x, y)`.
    pub fn find_tile_in_roots(
        roots: Option<&[QuadtreeTileHandle]>,
        coord: (u32, i32, i32),
    ) -> Option<QuadtreeTileHandle> {
        let (target_level, target_x, target_y) = coord;
        let level_zero_tiles = roots?;
        if target_level == 0 {
            return level_zero_tiles
                .iter()
                .find(|t| {
                    t.try_borrow().is_ok_and(|b| b.x == target_x && b.y == target_y)
                })
                .cloned();
        }

        if target_x < 0 || target_y < 0 {
            return None;
        }

        let root_x = target_x >> target_level;
        let root_y = target_y >> target_level;
        let root = level_zero_tiles
            .iter()
            .find(|t| {
                t.try_borrow().is_ok_and(|b| b.x == root_x && b.y == root_y)
            })?
            .clone();

        let mut current = root;
        for step in (0..target_level).rev() {
            let bit_x = (target_x >> step) & 1;
            let bit_y = (target_y >> step) & 1;
            let next_child = {
                let b = current.try_borrow().ok()?;
                match (bit_y, bit_x) {
                    (0, 0) => b.northwest_child.clone(),
                    (0, 1) => b.northeast_child.clone(),
                    (1, 0) => b.southwest_child.clone(),
                    (1, 1) => b.southeast_child.clone(),
                    _ => unreachable!(),
                }
            };
            current = next_child?;
        }
        Some(current)
    }

    /// Finds a tile in the quadtree hierarchy matching coordinate `(level, x, y)`.
    pub fn find_tile(&self, coord: (u32, i32, i32)) -> Option<QuadtreeTileHandle> {
        Self::find_tile_in_roots(self.level_zero_tiles.as_deref(), coord)
    }

    /// Dispatches asynchronous loading for tiles in the load queues.
    pub fn process_tile_load_queue(&mut self, frame_state: &FrameState) {
        if self.tile_load_queue_high.is_empty()
            && self.tile_load_queue_medium.is_empty()
            && self.tile_load_queue_low.is_empty()
        {
            return;
        }

        let roots = self.level_zero_tiles.as_deref();
        self.tile_replacement_queue
            .trim_tiles(self.tile_cache_size, |coord| {
                if let Some(tile) = Self::find_tile_in_roots(roots, coord) {
                    if let Ok(b) = tile.try_borrow() {
                        if !b.eligible_for_unloading() || !b.descendants_available() {
                            return false;
                        }
                    } else {
                        return false;
                    }
                    if let Ok(mut b) = tile.try_borrow_mut() {
                        b.free_resources();
                        true
                    } else {
                        false
                    }
                } else {
                    // Missing or temporarily borrowed ancestors cannot be
                    // safely unloaded; keep their queue entry for a later pass.
                    false
                }
            });

        self.tile_load_queue_high.sort_by(|a, b| {
            a.borrow()
                .load_priority
                .partial_cmp(&b.borrow().load_priority)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        for tile in &self.tile_load_queue_high {
            let coord = {
                let b = tile.borrow();
                (b.level, b.x, b.y)
            };
            self.tile_replacement_queue.mark_tile_rendered(coord);
            self.tile_provider.load_tile(frame_state, tile);
        }

        self.tile_load_queue_medium.sort_by(|a, b| {
            a.borrow()
                .load_priority
                .partial_cmp(&b.borrow().load_priority)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        for tile in &self.tile_load_queue_medium {
            let coord = {
                let b = tile.borrow();
                (b.level, b.x, b.y)
            };
            self.tile_replacement_queue.mark_tile_rendered(coord);
            self.tile_provider.load_tile(frame_state, tile);
        }

        self.tile_load_queue_low.sort_by(|a, b| {
            a.borrow()
                .load_priority
                .partial_cmp(&b.borrow().load_priority)
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        for tile in &self.tile_load_queue_low {
            let coord = {
                let b = tile.borrow();
                (b.level, b.x, b.y)
            };
            self.tile_replacement_queue.mark_tile_rendered(coord);
            self.tile_provider.load_tile(frame_state, tile);
        }
    }

    /// Updates terrain heights and invokes height query callbacks.
    pub fn update_heights(&mut self, frame_state: &FrameState) {
        if self.tile_to_update_heights.is_empty() {
            return;
        }

        let scheme = self.tile_provider.tiling_scheme();
        let ellipsoid = scheme.ellipsoid();
        let mut try_next_frame = Vec::new();

        while !self.tile_to_update_heights.is_empty() {
            let tile = self.tile_to_update_heights.remove(0);
            let has_mesh = tile.borrow().data.as_ref().is_some_and(|d| d.has_mesh());
            if !has_mesh {
                let selection_result = if tile.borrow().last_selection_result_frame
                    == self.last_selection_frame_number
                {
                    tile.borrow().last_selection_result
                } else {
                    TileSelectionResult::None
                };

                if selection_result == TileSelectionResult::Rendered
                    || selection_result == TileSelectionResult::CulledButNeeded
                {
                    try_next_frame.push(tile.clone());
                }
                tile.borrow_mut().clear_position_cache();
                continue;
            }

            let (tile_level, upsampled) = {
                let b = tile.borrow();
                (
                    b.level,
                    b.upsampled_from_parent
                        || b.data.as_ref().is_some_and(|d| d.was_created_by_upsampling()),
                )
            };

            let mut custom_data = std::mem::take(&mut tile.borrow_mut().custom_data);
            for data in custom_data.iter_mut() {
                let Some(registration) = self.active_height_callbacks.get(&data.id) else {
                    continue;
                };
                // Custom data is copied into multiple tiles; the registration
                // owns the authoritative level across all copies.
                let last_level = data.level.max(registration.level);

                if (tile_level as i32) > last_level && !upsampled {
                    let mut position = tile
                        .borrow_mut()
                        .get_position_cache_entry(&data.position_cartographic, self.maximum_screen_space_error);

                    if position.is_none() {
                        if data.position_on_ellipsoid_surface.is_none() {
                            data.position_on_ellipsoid_surface = Some(
                                ellipsoid.cartographic_to_cartesian(&Cartographic::new(
                                    data.position_cartographic.longitude,
                                    data.position_cartographic.latitude,
                                    0.0,
                                )),
                            );
                        }
                        let surface_pos = data.position_on_ellipsoid_surface.unwrap();

                        let ray = if frame_state.mode == SceneMode::Scene3D {
                            let surface_normal = ellipsoid
                                .geodetic_surface_normal(surface_pos)
                                .unwrap_or(Cartesian3::UNIT_Z);
                            let ray_origin = ellipsoid
                                .get_surface_normal_intersection_with_z_axis(surface_pos, 11500.0)
                                .unwrap_or_else(|| {
                                    let min_h = tile
                                        .borrow()
                                        .data
                                        .as_ref()
                                        .and_then(|d| d.minimum_height())
                                        .unwrap_or(0.0);
                                    let magnitude = min_h.min(-11500.0);
                                    let vec = surface_normal.multiply_by_scalar(magnitude.abs() + 1.0);
                                    surface_pos.subtract(vec)
                                });
                            Ray::new(ray_origin, surface_normal)
                        } else {
                            let mut scratch_carto = data.position_cartographic;
                            scratch_carto.height = -11500.0;
                            let scratch_pos = frame_state.uniform.map_projection.project(&scratch_carto);
                            let transformed = Cartesian3::new(scratch_pos.z, scratch_pos.x, scratch_pos.y);
                            Ray::new(transformed, Cartesian3::UNIT_X)
                        };

                        position = {
                            let b = tile.borrow();
                            let d = b.data.as_ref().unwrap();
                            if let Some(height) = d.sample_height(&data.position_cartographic) {
                                let carto = Cartographic::new(
                                    data.position_cartographic.longitude,
                                    data.position_cartographic.latitude,
                                    height,
                                );
                                Some(ellipsoid.cartographic_to_cartesian(&carto))
                            } else {
                                d.pick_with_projection(
                                    &ray,
                                    frame_state.mode,
                                    frame_state.uniform.map_projection.as_ref(),
                                    false,
                                )
                            }
                        };

                        if let Some(pos) = position {
                            tile.borrow_mut().set_position_cache_entry(
                                &data.position_cartographic,
                                self.maximum_screen_space_error,
                                pos,
                            );
                        }
                    }

                    if let Some(pos) = position {
                        data.level = tile_level as i32;
                        if let Some(reg) = self.active_height_callbacks.get_mut(&data.id) {
                            reg.level = data.level;
                            reg.position_on_ellipsoid_surface = data.position_on_ellipsoid_surface;
                        }
                        if let Some(carto) = ellipsoid.cartesian_to_cartographic(pos) {
                            if let Some(cb) = &data.callback {
                                (cb.borrow_mut())(carto);
                            }
                        }
                    }
                }
            }
            tile.borrow_mut().custom_data = custom_data;
        }

        self.tile_to_update_heights.extend(try_next_frame);
    }

    /// Raises tile load progress events when the queue length changes.
    pub fn update_tile_load_progress(&mut self, _frame_state: &mut FrameState) {
        let current_length = self.tile_load_queue_high.len()
            + self.tile_load_queue_medium.len()
            + self.tile_load_queue_low.len();

        if current_length != self.last_tile_load_queue_length || self.tiles_invalidated {
            self.tile_load_progress_event.raise_event(&current_length);
            self.last_tile_load_queue_length = current_length;
        }

        if self.debug.enable_debug_output && !self.debug.suspend_lod_update {
            self.debug.max_depth = self
                .tiles_to_render
                .iter()
                .map(|t| t.borrow().level as i32)
                .max()
                .unwrap_or(-1);
            self.debug.tiles_rendered = self.tiles_to_render.len();
        }
    }

    /// Destroys the quadtree primitive and associated tile provider resources.
    pub fn destroy(&mut self) {
        self.tile_provider.destroy();
        self.destroyed = true;
    }

    /// Returns true if this object was destroyed.
    pub fn is_destroyed(&self) -> bool {
        self.destroyed
    }
}
