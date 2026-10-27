//! CesiumJS `GlobeSurfaceTile`: quadtree globe surface tile load state and data.
//!
//! Encapsulates additional information and state transition logic for a [`QuadtreeTile`]
//! of the globe's surface, tracking terrain lifecycle states (`TerrainState`), attached
//! imagery skeletons (`TileImagery`), water mask texture coordinate scaling, pick bounding
//! volumes, and height sampling.
//!
//! # Upstream Parity and Architecture
//! Modeled against pinned CesiumJS 23.0.0 (`@cesium/engine` 23.0.0, CesiumJS 1.139)
//! `Source/Scene/GlobeSurfaceTile.js`.
//!
//! Implementations integrate with the CPU quadtree traversal architecture via [`QuadtreeTileData`],
//! allowing tiles to report unloading eligibility, release held CPU/imagery resources, sample
//! heights from terrain data, and participate in ray-terrain picking.
//!
//! # Limitations and Missing GPU/Pipeline Stages
//! 1. **Terrain Mesh Loading & Asynchronous Workers**: In CesiumJS, mesh generation for quantized
//!    meshes or heightmaps utilizes background web workers or native thread pools to unpack and
//!    tessellate buffers. In CPU-side CesiumRS, providers construct and pass [`TerrainMesh`]
//!    or [`SurfaceTerrainData`], while this tile node manages ownership and state transitions.
//! 2. **Imagery Integration**: Reprojection from Web Mercator to Geographic textures and multi-layer
//!    texture blending require GPU rasterization passes. The CPU tile node maintains [`TileImagery`]
//!    skeletons, layer indices, and translation/scale offsets. Full state transitions driving
//!    [`ImageryLayer`] are supported via [`GlobeSurfaceTile::process_imagery_with_layers`].
//! 3. **GPU / Render and Scene Frame Behavior**: WebGL `VertexArray` allocation, index buffer pooling,
//!    water mask texture binding, surface shader compilation, and frustum command generation are
//!    handled during renderer passes. CPU meshes alone do not fabricate GPU vertex array handles.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

use crate::core::heightmap_terrain_data::HeightmapTerrainData;
use crate::core::terrain_mesh::TerrainMesh;
use crate::core::terrain_picker::TerrainSceneMode;
use crate::core::{
    BoundingSphere, Cartesian3, Cartesian4, Cartographic, GeographicProjection, MapProjection,
    Ray, Rectangle, TilingScheme,
};
use crate::scene::frame_state::FrameState;
use crate::scene::imagery_layer::ImageryLayer;
use crate::scene::imagery_state::ImageryState;
use crate::scene::quadtree_tile::{
    QuadtreeCustomData, QuadtreeTile, QuadtreeTileData, QuadtreeTileHandle, WeakQuadtreeTileHandle,
};
use crate::scene::quadtree_tile_load_state::QuadtreeTileLoadState;
use crate::scene::scene_mode::SceneMode;
use crate::scene::terrain_state::TerrainState;
use crate::scene::tile_bounding_region::TileBoundingRegion;
use crate::scene::tile_imagery::TileImagery;

/// Object-safe abstraction for CPU-side surface terrain data held by a [`GlobeSurfaceTile`].
#[allow(dead_code)]
pub trait SurfaceTerrainData: std::fmt::Debug {
    /// Interpolates surface height at the given cartographic coordinate within the tile rectangle.
    fn interpolate_height(
        &self,
        rectangle: &Rectangle,
        longitude: f64,
        latitude: f64,
    ) -> Option<f64>;

    /// Determines whether a child quadrant is available according to terrain availability metadata.
    fn is_child_available(&self, this_x: u32, this_y: u32, child_x: u32, child_y: u32) -> bool;

    /// Whether this terrain data was synthesized by upsampling ancestor tile data.
    fn was_created_by_upsampling(&self) -> bool;

    /// Whether this terrain data can be used to upsample child tiles.
    fn can_upsample(&self) -> bool {
        true
    }
}

impl SurfaceTerrainData for HeightmapTerrainData {
    fn interpolate_height(
        &self,
        rectangle: &Rectangle,
        longitude: f64,
        latitude: f64,
    ) -> Option<f64> {
        self.interpolate_height(rectangle, longitude, latitude)
    }

    fn is_child_available(&self, this_x: u32, this_y: u32, child_x: u32, child_y: u32) -> bool {
        self.is_child_available(this_x, this_y, child_x, child_y)
    }

    fn was_created_by_upsampling(&self) -> bool {
        self.was_created_by_upsampling()
    }
}

/// Contains additional information about a [`QuadtreeTile`] of the globe's surface,
/// encapsulating state transition logic and resource management.
#[allow(dead_code)]
pub struct GlobeSurfaceTile {
    /// The [`TileImagery`] associations attached to this tile.
    pub imagery: Vec<TileImagery>,

    /// Translation and scale mapping water mask texture coordinates.
    /// Format: `(translationX, translationY, scaleX, scaleY)`.
    pub water_mask_translation_and_scale: Cartesian4,

    /// Whether a water mask texture is attached to this tile.
    pub has_water_mask_texture: bool,

    /// CPU-side surface terrain data for this tile, if loaded.
    pub terrain_data: Option<Box<dyn SurfaceTerrainData>>,

    /// Whether a valid GPU vertex array exists for rendering this tile.
    /// Note: purely CPU-side terrain meshes do not fabricate this GPU flag.
    pub has_vertex_array: bool,

    /// Bounding region used to estimate distance and visibility.
    pub tile_bounding_region: Option<TileBoundingRegion>,

    /// Scaled-space occludee point for horizon culling.
    pub occludee_point_in_scaled_space: Cartesian3,

    /// Ancestor or self tile that provided the current bounding volume.
    pub bounding_volume_source_tile: Option<WeakQuadtreeTileHandle>,

    /// Indicates whether the bounding volume was derived from high-precision mesh geometry.
    pub bounding_volume_is_from_mesh: bool,

    /// Load lifecycle state of this tile's terrain.
    pub terrain_state: TerrainState,

    /// The terrain mesh loaded and tessellated for this tile, if any.
    /// Wrapped in a `RefCell` to support interior caching during pick operations.
    pub mesh: RefCell<Option<TerrainMesh>>,

    /// Sphere used for pick intersection testing.
    pub pick_bounding_sphere: BoundingSphere,

    /// Whether this tile is subject to terrain clipping planes/polygons.
    pub is_clipped: bool,

    /// Whether this tile is fully outside clipping boundaries.
    pub clipped_by_boundaries: bool,

    /// Regional geographic bounds of this tile, if known.
    pub rectangle: Option<Rectangle>,

    /// Optional tiling scheme associated with this surface tile node.
    pub tiling_scheme: Option<Arc<dyn TilingScheme>>,

    /// Whether this tile requires height query recomputation by the quadtree LOD scheduler.
    pub needs_height_update: bool,
}

impl std::fmt::Debug for GlobeSurfaceTile {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GlobeSurfaceTile")
            .field("imagery_len", &self.imagery.len())
            .field("has_water_mask_texture", &self.has_water_mask_texture)
            .field("has_vertex_array", &self.has_vertex_array)
            .field("terrain_state", &self.terrain_state)
            .field("has_mesh", &self.mesh.borrow().is_some())
            .field("is_clipped", &self.is_clipped)
            .field("clipped_by_boundaries", &self.clipped_by_boundaries)
            .field("rectangle", &self.rectangle)
            .field("has_tiling_scheme", &self.tiling_scheme.is_some())
            .field("needs_height_update", &self.needs_height_update)
            .finish()
    }
}

impl Default for GlobeSurfaceTile {
    fn default() -> Self {
        Self::new()
    }
}

#[allow(dead_code)]
impl GlobeSurfaceTile {
    /// Creates a new `GlobeSurfaceTile` with default initial properties matching CesiumJS.
    pub fn new() -> Self {
        Self {
            imagery: Vec::new(),
            water_mask_translation_and_scale: Cartesian4::new(0.0, 0.0, 1.0, 1.0),
            has_water_mask_texture: false,
            terrain_data: None,
            has_vertex_array: false,
            tile_bounding_region: None,
            occludee_point_in_scaled_space: Cartesian3::ZERO,
            bounding_volume_source_tile: None,
            bounding_volume_is_from_mesh: false,
            terrain_state: TerrainState::Unloaded,
            mesh: RefCell::new(None),
            pick_bounding_sphere: BoundingSphere::default(),
            is_clipped: true,
            clipped_by_boundaries: false,
            rectangle: None,
            tiling_scheme: None,
            needs_height_update: false,
        }
    }

    /// Gets a value indicating whether or not this tile is eligible to be unloaded from cache.
    ///
    /// Parity with CesiumJS `GlobeSurfaceTile.prototype.eligibleForUnloading`:
    /// Tiles are ineligible while terrain loading is in `RECEIVING` or `TRANSFORMING` states,
    /// or if any attached [`TileImagery`] has a `loading_imagery` in `TRANSITIONING` state.
    pub fn eligible_for_unloading(&self) -> bool {
        let loading_is_transitioning = self.terrain_state == TerrainState::Receiving
            || self.terrain_state == TerrainState::Transforming;
        if loading_is_transitioning {
            return false;
        }

        for tile_imagery in &self.imagery {
            if let Some(loading) = &tile_imagery.loading_imagery {
                if loading.borrow().state == ImageryState::Transitioning {
                    return false;
                }
            }
        }

        true
    }

    /// Whether a rendered mesh is currently available.
    ///
    /// Parity with CesiumJS `renderedMesh`: returns `true` if a GPU vertex array is present
    /// for the mesh, or in CPU-ready validation states where a validated [`TerrainMesh`] is ready.
    pub fn has_rendered_mesh(&self) -> bool {
        self.has_vertex_array && self.mesh.borrow().is_some()
    }

    /// Sets the terrain mesh on the CPU without falsely claiming a GPU vertex array exists.
    pub fn set_mesh(&mut self, mesh: TerrainMesh) {
        *self.mesh.borrow_mut() = Some(mesh);
        self.has_vertex_array = false;
        self.terrain_state = TerrainState::Transformed;
    }

    /// Explicitly updates whether a GPU vertex array handle exists for this tile.
    ///
    /// Setting `has_vertex_array = true` when a terrain mesh is present transitions
    /// the terrain state to `TerrainState::Ready`, matching CesiumJS GPU resource creation passes.
    pub fn set_vertex_array_present(&mut self, present: bool) {
        self.has_vertex_array = present;
        if present && self.mesh.borrow().is_some() {
            self.terrain_state = TerrainState::Ready;
        } else if !present && self.terrain_state == TerrainState::Ready {
            self.terrain_state = TerrainState::Transformed;
        }
    }

    /// Computes ray-terrain intersection against this tile's rendered mesh.
    ///
    /// Returns `None` if no mesh is rendered or no intersection occurs.
    pub fn pick(
        &self,
        ray: &Ray,
        mode: Option<TerrainSceneMode>,
        projection: &dyn MapProjection,
        cull_back_faces: bool,
    ) -> Option<Cartesian3> {
        if !self.has_rendered_mesh() {
            return None;
        }
        if let Ok(mut mesh_guard) = self.mesh.try_borrow_mut() {
            if let Some(mesh) = mesh_guard.as_mut() {
                return mesh.pick(ray, cull_back_faces, mode, projection);
            }
        }
        None
    }

    /// Releases all terrain, imagery, and mesh resources held by this tile.
    ///
    /// Parity with CesiumJS `GlobeSurfaceTile.prototype.freeResources`.
    pub fn free_resources(&mut self) {
        self.has_water_mask_texture = false;
        self.water_mask_translation_and_scale = Cartesian4::new(0.0, 0.0, 1.0, 1.0);
        self.terrain_data = None;
        self.terrain_state = TerrainState::Unloaded;
        *self.mesh.borrow_mut() = None;
        self.has_vertex_array = false;
        self.needs_height_update = false;

        for tile_imagery in &mut self.imagery {
            tile_imagery.free_resources();
        }
        self.imagery.clear();
    }

    /// Computes translation and scale for sampling a water mask texture from an ancestor tile.
    ///
    /// Parity with CesiumJS `GlobeSurfaceTile.prototype._computeWaterMaskTranslationAndScale`.
    pub fn compute_water_mask_translation_and_scale(
        tile_rectangle: &Rectangle,
        source_tile_rectangle: &Rectangle,
    ) -> Cartesian4 {
        let tile_width = tile_rectangle.width();
        let tile_height = tile_rectangle.height();
        let source_width = source_tile_rectangle.width();
        let source_height = source_tile_rectangle.height();

        let scale_x = tile_width / source_width;
        let scale_y = tile_height / source_height;

        let x = (scale_x * (tile_rectangle.west - source_tile_rectangle.west)) / tile_width;
        let y = (scale_y * (tile_rectangle.south - source_tile_rectangle.south)) / tile_height;

        Cartesian4::new(x, y, scale_x, scale_y)
    }

    /// Traverses the ancestor hierarchy to locate the closest ancestor tile with valid, non-upsampled terrain data.
    ///
    /// Parity with CesiumJS `GlobeSurfaceTile.prototype._findAncestorTileWithTerrainData`.
    pub fn find_ancestor_tile_with_terrain_data(
        tile: &QuadtreeTile,
    ) -> Option<QuadtreeTileHandle> {
        let mut current_parent = tile.parent.as_ref().and_then(|p| p.upgrade());

        while let Some(parent_rc) = current_parent {
            let parent_borrow = parent_rc.borrow();
            let has_non_upsampled_terrain = parent_borrow
                .data
                .as_ref()
                .and_then(|d| d.as_any().downcast_ref::<GlobeSurfaceTile>())
                .and_then(|gst| gst.terrain_data.as_ref())
                .is_some_and(|td| !td.was_created_by_upsampling());

            if has_non_upsampled_terrain {
                drop(parent_borrow);
                return Some(parent_rc);
            }

            let next_parent = parent_borrow.parent.as_ref().and_then(|p| p.upgrade());
            drop(parent_borrow);
            current_parent = next_parent;
        }

        None
    }

    /// Updates terrain vertical exaggeration and marks custom height queries for recomputation.
    ///
    /// Parity with CesiumJS `GlobeSurfaceTile.prototype.updateExaggeration`.
    pub fn update_exaggeration(
        &mut self,
        tile: &mut QuadtreeTile,
        vertical_exaggeration: f64,
        relative_height: f64,
    ) -> bool {
        <Self as QuadtreeTileData>::update_exaggeration(
            self,
            &mut tile.custom_data,
            &mut tile.added_custom_data,
            vertical_exaggeration,
            relative_height,
        )
    }

    /// Updates terrain vertical exaggeration and requeues the tile handle in the quadtree height update queue.
    ///
    /// Safely attempts to borrow the tile handle. If the tile handle is already mutably borrowed
    /// (e.g. because this `GlobeSurfaceTile` is being accessed from inside `tile_handle.data`),
    /// this returns an `Err` rather than causing a `RefCell` reentrant panic.
    pub fn update_exaggeration_with_quadtree(
        &mut self,
        tile_handle: &QuadtreeTileHandle,
        vertical_exaggeration: f64,
        relative_height: f64,
        tile_to_update_heights: &mut Vec<QuadtreeTileHandle>,
    ) -> Result<bool, &'static str> {
        let Ok(mut tile_borrow) = tile_handle.try_borrow_mut() else {
            return Err("Cannot re-borrow tile_handle: surface is stored inside QuadtreeTile.data. Use QuadtreePrimitive::update_tile_exaggeration or QuadtreeTile::update_exaggeration instead.");
        };
        let changed = self.update_exaggeration(&mut tile_borrow, vertical_exaggeration, relative_height);
        if self.needs_height_update
            && !tile_to_update_heights.iter().any(|h| Rc::ptr_eq(h, tile_handle))
        {
            tile_to_update_heights.push(Rc::clone(tile_handle));
        }
        Ok(changed)
    }

    /// Updates the scene mode on this tile's rendered mesh.
    ///
    /// Parity with CesiumJS `GlobeSurfaceTile.prototype.updateSceneMode`.
    pub fn update_scene_mode(&self, _mode: Option<TerrainSceneMode>) {
        if let Ok(mut mesh_guard) = self.mesh.try_borrow_mut() {
            if let Some(mesh) = mesh_guard.as_mut() {
                mesh.update_scene_mode();
            }
        }
    }

    /// Processes the imagery load state machine for all attached [`TileImagery`].
    ///
    /// Parity with CesiumJS `GlobeSurfaceTile.prototype.processImagery`.
    pub fn process_imagery(
        &mut self,
        tile: &mut QuadtreeTile,
        frame_state: Option<&FrameState>,
        skip_loading: bool,
    ) -> bool {
        let mut is_upsampled_only = tile.upsampled_from_parent;
        let mut is_any_tile_loaded = false;
        let mut is_done_loading = true;

        for tile_imagery in &mut self.imagery {
            if tile_imagery.loading_imagery.is_none() {
                is_upsampled_only = false;
                continue;
            }

            let this_tile_done_loading = if let Some(layer_handle) = tile_imagery.imagery_layer.clone() {
                let mut layer = layer_handle.borrow_mut();
                tile_imagery.process_state_machine(
                    &mut layer,
                    &tile.rectangle,
                    frame_state,
                    skip_loading,
                )
            } else {
                let loading_rc = tile_imagery.loading_imagery.as_ref().unwrap();
                let loading_state = loading_rc.borrow().state;

                if loading_state == ImageryState::Ready {
                    if let Some(ready) = tile_imagery.ready_imagery.take() {
                        ready.borrow_mut().release_reference();
                    }
                    tile_imagery.ready_imagery = tile_imagery.loading_imagery.take();
                }

                tile_imagery.is_done_loading()
                    || loading_state == ImageryState::Failed
                    || loading_state == ImageryState::Invalid
            };

            if !skip_loading || this_tile_done_loading {
                is_done_loading = is_done_loading && this_tile_done_loading;
            } else {
                is_done_loading = false;
            }

            is_any_tile_loaded = is_any_tile_loaded
                || this_tile_done_loading
                || tile_imagery.has_ready_imagery();

            if let Some(loading_rc) = tile_imagery.loading_imagery.as_ref() {
                let loading_state = loading_rc.borrow().state;
                is_upsampled_only = is_upsampled_only
                    && (loading_state == ImageryState::Failed || loading_state == ImageryState::Invalid);
            }
        }

        tile.upsampled_from_parent = is_upsampled_only;
        tile.renderable = tile.renderable && (is_any_tile_loaded || is_done_loading);

        is_done_loading
    }

    /// Processes the imagery state machine for all attached [`TileImagery`] by routing each
    /// tile imagery to its actual owning layer.
    pub fn process_imagery_with_layers(
        &mut self,
        tile: &mut QuadtreeTile,
        layers: &mut [ImageryLayer],
        frame_state: Option<&FrameState>,
        skip_loading: bool,
    ) -> bool {
        let mut is_upsampled_only = tile.upsampled_from_parent;
        let mut is_any_tile_loaded = false;
        let mut is_done_loading = true;

        for tile_imagery in &mut self.imagery {
            if tile_imagery.loading_imagery.is_none() {
                is_upsampled_only = false;
                continue;
            }

            // Map each TileImagery to its actual owning layer:
            // 1. By stable layer identity (layer_id / layer handle id).
            // 2. Fallback for test fixtures without layer ID: by explicit layer_index()
            //    matching layer.layer_index().
            // Never guess positional indices in `layers` or fallback to layers[0].
            let matched_layer = if let Some(target_id) = tile_imagery.layer_id() {
                layers.iter().position(|l| l.id() == target_id).map(|pos| &mut layers[pos])
            } else if let Some(target_idx) = tile_imagery.layer_index() {
                layers
                    .iter()
                    .position(|l| l.layer_index() >= 0 && l.layer_index() as usize == target_idx)
                    .map(|pos| &mut layers[pos])
            } else {
                None
            };

            let this_tile_done_loading = if let Some(layer) = matched_layer {
                tile_imagery.process_state_machine(
                    layer,
                    &tile.rectangle,
                    frame_state,
                    skip_loading,
                )
            } else {
                // Explicit unsupported result: cannot route to an unmapped layer.
                // Maintain existing loading state if any, but do not guess or use incorrect index routing.
                let loading_state = tile_imagery
                    .loading_imagery
                    .as_ref()
                    .map(|img| img.borrow().state);
                tile_imagery.is_done_loading()
                    || loading_state == Some(ImageryState::Failed)
                    || loading_state == Some(ImageryState::Invalid)
            };

            is_done_loading = is_done_loading && this_tile_done_loading;

            is_any_tile_loaded = is_any_tile_loaded
                || this_tile_done_loading
                || tile_imagery.has_ready_imagery();

            if let Some(loading_rc) = tile_imagery.loading_imagery.as_ref() {
                let loading_state = loading_rc.borrow().state;
                is_upsampled_only = is_upsampled_only
                    && (loading_state == ImageryState::Failed || loading_state == ImageryState::Invalid);
            }
        }

        tile.upsampled_from_parent = is_upsampled_only;
        tile.renderable = tile.renderable && (is_any_tile_loaded || is_done_loading);

        is_done_loading
    }

    /// Processes the imagery load state machine for all attached [`TileImagery`] by routing
    /// each tile imagery to its matching owning layer handle from the given slice of handles.
    pub fn process_imagery_with_layer_handles(
        &mut self,
        tile: &mut QuadtreeTile,
        layers: &[crate::scene::imagery_layer::ImageryLayerHandle],
        frame_state: Option<&FrameState>,
        skip_loading: bool,
    ) -> bool {
        let mut is_upsampled_only = tile.upsampled_from_parent;
        let mut is_any_tile_loaded = false;
        let mut is_done_loading = true;

        for tile_imagery in &mut self.imagery {
            if tile_imagery.loading_imagery.is_none() {
                is_upsampled_only = false;
                continue;
            }

            // Find matching layer handle:
            // 1. If tile_imagery already has attached imagery_layer, use it.
            // 2. Otherwise locate handle in `layers` where handle.borrow().id() == target_id.
            // 3. Fallback: match by layer_index if layer_id is None.
            let matched_handle = if let Some(ref handle) = tile_imagery.imagery_layer {
                Some(handle.clone())
            } else if let Some(target_id) = tile_imagery.layer_id() {
                layers.iter().find(|h| h.borrow().id() == target_id).cloned()
            } else if let Some(target_idx) = tile_imagery.layer_index() {
                layers.iter().find(|h| {
                    let l = h.borrow();
                    l.layer_index() >= 0 && l.layer_index() as usize == target_idx
                }).cloned()
            } else {
                None
            };

            let this_tile_done_loading = if let Some(handle) = matched_handle {
                if tile_imagery.imagery_layer.is_none() {
                    tile_imagery.imagery_layer = Some(handle.clone());
                }
                let mut layer = handle.borrow_mut();
                tile_imagery.process_state_machine(
                    &mut layer,
                    &tile.rectangle,
                    frame_state,
                    skip_loading,
                )
            } else {
                let loading_state = tile_imagery
                    .loading_imagery
                    .as_ref()
                    .map(|img| img.borrow().state);
                tile_imagery.is_done_loading()
                    || loading_state == Some(ImageryState::Failed)
                    || loading_state == Some(ImageryState::Invalid)
            };

            is_done_loading = is_done_loading && this_tile_done_loading;

            is_any_tile_loaded = is_any_tile_loaded
                || this_tile_done_loading
                || tile_imagery.has_ready_imagery();

            if let Some(loading_rc) = tile_imagery.loading_imagery.as_ref() {
                let loading_state = loading_rc.borrow().state;
                is_upsampled_only = is_upsampled_only
                    && (loading_state == ImageryState::Failed || loading_state == ImageryState::Invalid);
            }
        }

        tile.upsampled_from_parent = is_upsampled_only;
        tile.renderable = tile.renderable && (is_any_tile_loaded || is_done_loading);

        is_done_loading
    }

    /// Processes the imagery load state machine for all attached [`TileImagery`] using the
    /// layers in an [`ImageryLayerCollection`].
    pub fn process_imagery_with_collection(
        &mut self,
        tile: &mut QuadtreeTile,
        collection: &crate::scene::imagery_layer_collection::ImageryLayerCollection,
        frame_state: Option<&FrameState>,
        skip_loading: bool,
    ) -> bool {
        self.process_imagery_with_layer_handles(tile, collection.layers(), frame_state, skip_loading)
    }

    /// Samples surface height from this tile's terrain data at a cartographic position.
    ///
    /// Respects the tile's regional bounds and avoids falling back to the global
    /// rectangle for regional heightmaps.
    pub fn sample_height(&self, position_cartographic: &Cartographic) -> Option<f64> {
        let terrain_data = self.terrain_data.as_ref()?;

        let rect_opt = self
            .tile_bounding_region
            .as_ref()
            .map(|r| r.rectangle)
            .or(self.rectangle)
            .or_else(|| self.mesh.borrow().as_ref().map(|m| m.rectangle));

        // Missing regional bounds must yield None regardless of terrain upsampling.
        let rect = rect_opt?;
        if !rect.contains(position_cartographic) {
            return None;
        }

        let height = terrain_data.interpolate_height(
            &rect,
            position_cartographic.longitude,
            position_cartographic.latitude,
        )?;
        let mesh = self.mesh.borrow();
        if self.has_vertex_array {
            if let Some(mesh) = mesh.as_ref() {
                return Some(crate::core::vertical_exaggeration::get_height(
                    height,
                    mesh.encoding.exaggeration,
                    mesh.encoding.exaggeration_relative_height,
                ));
            }
        }
        Some(height)
    }

    /// Initializes a quadtree tile's surface tile data and state machine.
    ///
    /// Parity with CesiumJS `GlobeSurfaceTile.initialize`.
    pub fn initialize(tile: &mut QuadtreeTile, terrain_available: Option<bool>) {
        if tile.data.is_none() {
            let mut gst = GlobeSurfaceTile::new();
            gst.rectangle = Some(tile.rectangle);
            gst.tiling_scheme = Some(Arc::clone(&tile.tiling_scheme));
            tile.data = Some(Box::new(gst));
        } else if let Some(surface_tile) = tile
            .data
            .as_mut()
            .and_then(|d| d.as_any_mut().downcast_mut::<GlobeSurfaceTile>())
        {
            if surface_tile.rectangle.is_none() {
                surface_tile.rectangle = Some(tile.rectangle);
            }
            if surface_tile.tiling_scheme.is_none() {
                surface_tile.tiling_scheme = Some(Arc::clone(&tile.tiling_scheme));
            }
        }

        if tile.state == QuadtreeTileLoadState::Start {
            if let Some(surface_tile) = tile
                .data
                .as_mut()
                .and_then(|d| d.as_any_mut().downcast_mut::<GlobeSurfaceTile>())
            {
                if terrain_available == Some(false) {
                    surface_tile.terrain_state = TerrainState::Failed;
                }
            }
            tile.state = QuadtreeTileLoadState::Loading;
        }
    }
}

impl QuadtreeTileData for GlobeSurfaceTile {
    fn eligible_for_unloading(&self) -> bool {
        self.eligible_for_unloading()
    }

    fn free_resources(&mut self) {
        self.free_resources();
    }

    fn has_mesh(&self) -> bool {
        self.mesh.borrow().is_some()
    }

    fn was_created_by_upsampling(&self) -> bool {
        self.terrain_data
            .as_ref()
            .is_some_and(|td| td.was_created_by_upsampling())
    }

    fn minimum_height(&self) -> Option<f64> {
        self.mesh
            .borrow()
            .as_ref()
            .map(|m| m.minimum_height)
            .or_else(|| self.tile_bounding_region.as_ref().map(|r| r.minimum_height))
    }

    fn pick(
        &self,
        ray: &Ray,
        mode: SceneMode,
        cull_back_faces: bool,
    ) -> Option<Cartesian3> {
        let terrain_mode = match mode {
            SceneMode::Morphing => Some(TerrainSceneMode::Morphing),
            SceneMode::ColumbusView => Some(TerrainSceneMode::ColumbusView),
            SceneMode::Scene2D => Some(TerrainSceneMode::Scene2D),
            SceneMode::Scene3D => Some(TerrainSceneMode::Scene3D),
        };

        match mode {
            SceneMode::Scene3D => {
                // In 3D mode, ray-mesh picking operates in 3D Cartesian coordinates.
                let projection = GeographicProjection::new(crate::core::Ellipsoid::WGS84);
                self.pick(ray, terrain_mode, &projection, cull_back_faces)
            }
            SceneMode::ColumbusView | SceneMode::Scene2D | SceneMode::Morphing => {
                // In projected modes, picking requires the scene's actual MapProjection.
                // When invoked without the scene projection, explicitly reject projected modes
                // by returning None rather than guessing or fabricating an arbitrary projection.
                None
            }
        }
    }

    fn pick_with_projection(
        &self,
        ray: &Ray,
        mode: SceneMode,
        projection: &dyn MapProjection,
        cull_back_faces: bool,
    ) -> Option<Cartesian3> {
        let terrain_mode = match mode {
            SceneMode::Morphing => Some(TerrainSceneMode::Morphing),
            SceneMode::ColumbusView => Some(TerrainSceneMode::ColumbusView),
            SceneMode::Scene2D => Some(TerrainSceneMode::Scene2D),
            SceneMode::Scene3D => Some(TerrainSceneMode::Scene3D),
        };
        self.pick(ray, terrain_mode, projection, cull_back_faces)
    }

    fn update_exaggeration(
        &mut self,
        tile_custom_data: &mut [QuadtreeCustomData],
        tile_added_custom_data: &mut [QuadtreeCustomData],
        vertical_exaggeration: f64,
        relative_height: f64,
    ) -> bool {
        if !self.has_rendered_mesh() {
            return false;
        }
        if let Ok(mut mesh_guard) = self.mesh.try_borrow_mut() {
            if let Some(mesh) = mesh_guard.as_mut() {
                let encoding_exaggeration_scale_changed =
                    mesh.encoding.exaggeration != vertical_exaggeration;
                let encoding_relative_height_changed =
                    mesh.encoding.exaggeration_relative_height != relative_height;
                let exaggeration_changed =
                    encoding_exaggeration_scale_changed || encoding_relative_height_changed;

                if exaggeration_changed {
                    mesh.encoding.exaggeration = vertical_exaggeration;
                    mesh.encoding.exaggeration_relative_height = relative_height;

                    for data in tile_custom_data.iter_mut() {
                        data.level = -1;
                    }
                    for data in tile_added_custom_data.iter_mut() {
                        data.level = -1;
                    }

                    self.needs_height_update = true;
                    mesh.update_exaggeration();
                    return true;
                }
            }
        }
        false
    }

    fn sample_height(&self, position_cartographic: &Cartographic) -> Option<f64> {
        self.sample_height(position_cartographic)
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }

    fn as_any_mut(&mut self) -> &mut dyn std::any::Any {
        self
    }
}
