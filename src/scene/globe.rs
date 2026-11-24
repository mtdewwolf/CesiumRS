//! CesiumJS globe lifecycle and terrain queries.

use super::{
    depth_plane::DepthPlane,
    globe_surface_tile_provider::tile_sphere,
    quadtree_occluders::QuadtreeOccluders,
    quadtree_primitive::{QuadtreePrimitive, QuadtreePrimitiveOptions},
    quadtree_tile::{QuadtreeTile, QuadtreeTileHandle},
    quadtree_tile_provider::QuadtreeTileProvider,
    FrameResource, FrameState, GlobeSurfaceOptions, GlobeSurfaceTile, GlobeSurfaceTileProvider,
    GlobeTerrainProviderAdapter, GlobeTerrainSource, GlobeTranslucency, GlobeTranslucencyFrame, ImageryLayerCollection,
    SceneGlobe, SceneMode,
};
use crate::{
    core::{
        intersection_tests::ray_sphere, terrain_mesh::TerrainMesh, terrain_picker::TerrainSceneMode, Cartesian3,
        Cartographic, Ellipsoid, EllipsoidTerrainProvider, Event, MapProjection, Ray, RuntimeError,
    },
    renderer::{Context, Gl, Texture},
};
use std::{
    future::Future,
    pin::Pin,
    rc::Rc,
    task::{Context as TaskContext, Poll, Waker},
};

pub const DEFAULT_OCEAN_NORMAL_MAP_URL: &str = "Assets/Textures/waterNormalsSmall.jpg";

pub type OceanNormalMapFuture<G> = Pin<Box<dyn Future<Output = Result<Rc<Texture<G>>, RuntimeError>>>>;
pub type OceanNormalMapLoader<G> = Rc<dyn Fn(&str) -> OceanNormalMapFuture<G>>;

#[derive(Clone, Debug)]
pub struct GlobeOptions {
    pub ellipsoid: Ellipsoid,
    pub show: bool,
    pub maximum_screen_space_error: f64,
    pub tile_cache_size: usize,
    pub loading_descendant_limit: usize,
    pub preload_ancestors: bool,
    pub preload_siblings: bool,
    pub depth_test_against_terrain: bool,
    pub surface: GlobeSurfaceOptions,
}

impl GlobeOptions {
    pub fn new(ellipsoid: Ellipsoid) -> Self {
        Self {
            ellipsoid,
            show: true,
            maximum_screen_space_error: 2.0,
            tile_cache_size: 100,
            loading_descendant_limit: 20,
            preload_ancestors: true,
            preload_siblings: false,
            depth_test_against_terrain: false,
            surface: GlobeSurfaceOptions::new(ellipsoid),
        }
    }
}

impl Default for GlobeOptions {
    fn default() -> Self {
        Self::new(Ellipsoid::WGS84)
    }
}

pub struct Globe<G: Gl + 'static> {
    ellipsoid: Ellipsoid,
    pub show: bool,
    pub maximum_screen_space_error: f64,
    pub tile_cache_size: usize,
    pub loading_descendant_limit: usize,
    pub preload_ancestors: bool,
    pub preload_siblings: bool,
    pub depth_test_against_terrain: bool,
    pub translucency: GlobeTranslucency,
    pub terrain_provider_changed: Event<Rc<dyn GlobeTerrainSource>>,
    pub surface: QuadtreePrimitive<GlobeSurfaceTileProvider<G>>,
    ocean_normal_map_url: Option<String>,
    ocean_normal_map_loader: Option<OceanNormalMapLoader<G>>,
    ocean_normal_map_future: Option<OceanNormalMapFuture<G>>,
    ocean_normal_map_dirty: bool,
    exaggeration: [f64; 2],
    depth_plane: DepthPlane<G>,
}

impl<G: Gl + 'static> Globe<G> {
    pub fn new(context: &Context<G>, options: GlobeOptions) -> Self {
        let terrain = EllipsoidTerrainProvider::new(None, Some(options.ellipsoid), None);
        Self::with_terrain_provider(context, options, Rc::new(GlobeTerrainProviderAdapter::new(terrain, None)))
    }

    pub fn with_terrain_provider(
        context: &Context<G>,
        options: GlobeOptions,
        terrain_provider: Rc<dyn GlobeTerrainSource>,
    ) -> Self {
        let mut tile_provider = GlobeSurfaceTileProvider::new(context, terrain_provider);
        tile_provider.options = options.surface;
        let surface = QuadtreePrimitive::new(QuadtreePrimitiveOptions {
            tile_provider,
            maximum_screen_space_error: Some(options.maximum_screen_space_error),
            tile_cache_size: Some(options.tile_cache_size),
            loading_descendant_limit: Some(options.loading_descendant_limit),
            preload_ancestors: Some(options.preload_ancestors),
            preload_siblings: Some(options.preload_siblings),
        });
        Self {
            ellipsoid: options.ellipsoid,
            show: options.show,
            maximum_screen_space_error: options.maximum_screen_space_error,
            tile_cache_size: options.tile_cache_size,
            loading_descendant_limit: options.loading_descendant_limit,
            preload_ancestors: options.preload_ancestors,
            preload_siblings: options.preload_siblings,
            depth_test_against_terrain: options.depth_test_against_terrain,
            translucency: GlobeTranslucency::new(),
            terrain_provider_changed: Event::new(),
            surface,
            ocean_normal_map_url: Some(DEFAULT_OCEAN_NORMAL_MAP_URL.into()),
            ocean_normal_map_loader: None,
            ocean_normal_map_future: None,
            ocean_normal_map_dirty: true,
            exaggeration: [1.0, 0.0],
            depth_plane: DepthPlane::default(),
        }
    }

    pub fn ellipsoid(&self) -> &Ellipsoid {
        &self.ellipsoid
    }

    pub fn surface_options(&self) -> &GlobeSurfaceOptions {
        &self.surface.tile_provider.options
    }
    pub fn surface_options_mut(&mut self) -> &mut GlobeSurfaceOptions {
        &mut self.surface.tile_provider.options
    }

    pub fn imagery_layers(&self) -> &ImageryLayerCollection {
        &self.surface.tile_provider.imagery_layers
    }
    pub fn imagery_layers_mut(&mut self) -> &mut ImageryLayerCollection {
        &mut self.surface.tile_provider.imagery_layers
    }
    pub fn imagery_layers_updated_event(&mut self) -> &mut Event<()> {
        &mut self.surface.tile_provider.imagery_layers_updated_event
    }
    pub fn tile_load_progress_event(&mut self) -> &mut Event<usize> {
        &mut self.surface.tile_load_progress_event
    }

    pub fn terrain_provider(&self) -> &Rc<dyn GlobeTerrainSource> {
        self.surface.tile_provider.terrain_provider()
    }

    pub fn set_terrain_provider(&mut self, provider: Rc<dyn GlobeTerrainSource>) {
        if Rc::ptr_eq(self.terrain_provider(), &provider) {
            return;
        }
        self.surface.occluders = QuadtreeOccluders::new(provider.tiling_scheme().ellipsoid());
        self.surface.tile_provider.replace_terrain_provider(provider.clone());
        self.surface.invalidate_all_tiles();
        self.surface.tiles_rendered_this_frame.clear();
        self.surface.tiles_to_render.clear();
        self.ocean_normal_map_future = None;
        self.ocean_normal_map_dirty = true;
        self.terrain_provider_changed.raise_event(&provider);
    }

    pub fn tiles_loaded(&self) -> bool {
        !self.surface.tiles_invalidated
            && self.surface.tile_load_queue_high.is_empty()
            && self.surface.tile_load_queue_medium.is_empty()
            && self.surface.tile_load_queue_low.is_empty()
    }

    pub fn ocean_normal_map_url(&self) -> Option<&str> {
        self.ocean_normal_map_url.as_deref()
    }

    pub fn set_ocean_normal_map_url(&mut self, url: Option<String>) {
        if self.ocean_normal_map_url != url {
            self.ocean_normal_map_url = url;
            self.ocean_normal_map_dirty = true;
            self.ocean_normal_map_future = None;
        }
    }

    pub fn set_ocean_normal_map_loader(&mut self, loader: OceanNormalMapLoader<G>) {
        self.ocean_normal_map_loader = Some(loader);
        self.ocean_normal_map_future = None;
        self.ocean_normal_map_dirty = true;
    }

    fn poll_ocean_normal_map(&mut self, frame: &FrameState) -> Result<(), RuntimeError> {
        if !self.terrain_provider().has_water_mask() {
            return Ok(());
        }
        if self.ocean_normal_map_dirty {
            self.ocean_normal_map_dirty = false;
            if let Some(url) = self.ocean_normal_map_url.as_deref() {
                self.ocean_normal_map_future = self.ocean_normal_map_loader.as_ref().map(|loader| loader(url));
            } else {
                self.surface.tile_provider.ocean_normal_map = None;
            }
        }
        if let Some(future) = &mut self.ocean_normal_map_future {
            let waker = frame.render_request.as_ref().map(super::SceneRenderRequest::waker);
            if let Poll::Ready(texture) =
                future.as_mut().poll(&mut TaskContext::from_waker(waker.as_ref().unwrap_or(Waker::noop())))
            {
                self.ocean_normal_map_future = None;
                self.surface.tile_provider.ocean_normal_map = Some(texture?);
            }
        }
        Ok(())
    }

    pub fn update(&mut self, frame: &mut FrameState) -> Result<(), RuntimeError> {
        self.check_alive()?;
        if self.show {
            self.surface.tile_provider.update(frame);
        }
        Ok(())
    }

    pub fn begin_frame(&mut self, frame: &mut FrameState) -> Result<(), RuntimeError> {
        self.check_alive()?;
        let base_alpha = self.surface_options().base_color.alpha;
        let state = &mut self.surface.tile_provider.translucency_state;
        state.update(
            &self.translucency,
            GlobeTranslucencyFrame {
                show: self.show,
                base_alpha,
                camera_underground: frame.camera_underground,
                depth_test_against_terrain: self.depth_test_against_terrain,
                depth_texture: frame.context_capabilities.depth_texture,
                mode: frame.mode,
            },
        );
        frame.clear_globe_depth = self.show && (!self.depth_test_against_terrain || frame.mode == SceneMode::Scene2D);
        frame.globe_depth_plane = None;
        if frame.clear_globe_depth
            && state.use_depth_plane
            && frame.mode == SceneMode::Scene3D
            && frame.context.is_some()
        {
            let mut camera = QuadtreePrimitive::<GlobeSurfaceTileProvider<G>>::camera_from_frame_state(frame);
            self.depth_plane.update(&mut camera, frame)?;
            frame.globe_depth_plane = self.depth_plane.command().cloned();
        }
        frame.globe_translucency_state = Some(FrameResource::new(state.clone()));
        if !self.show {
            return Ok(());
        }
        self.surface.maximum_screen_space_error = self.maximum_screen_space_error;
        self.surface.tile_cache_size = self.tile_cache_size;
        self.surface.loading_descendant_limit = self.loading_descendant_limit;
        self.surface.preload_ancestors = self.preload_ancestors;
        self.surface.preload_siblings = self.preload_siblings;
        if frame.passes.render {
            self.poll_ocean_normal_map(frame)?;
            self.surface.begin_frame(frame);
            let exaggeration = [frame.vertical_exaggeration, frame.vertical_exaggeration_relative_height];
            if self.exaggeration != exaggeration {
                let loaded = self.surface.tile_provider.loaded_tiles();
                for tile in loaded {
                    if self.surface.update_tile_exaggeration(&tile, exaggeration[0], exaggeration[1]) {
                        self.surface.tile_provider.refresh_exaggerated_tile(&tile)?;
                    }
                }
                self.exaggeration = exaggeration;
            }
        }
        self.surface.tile_provider.prepare_render(frame)
    }

    pub fn render(&mut self, frame: &mut FrameState) -> Result<(), RuntimeError> {
        self.check_alive()?;
        if !self.show {
            return Ok(());
        }
        if frame.passes.render {
            let mut camera = QuadtreePrimitive::<GlobeSurfaceTileProvider<G>>::camera_from_frame_state(frame);
            camera.update(frame.mode).map_err(|e| RuntimeError::new(e.to_string()))?;
            self.surface.render(frame, &mut camera);
        } else if frame.passes.pick || frame.passes.pick_voxel || frame.passes.depth {
            self.surface.tile_provider.render_tiles(&self.surface.tiles_to_render, frame);
        }
        self.surface.tile_provider.take_draw_error()
    }

    pub fn end_frame(&mut self, frame: &mut FrameState) -> Result<(), RuntimeError> {
        self.check_alive()?;
        if self.show {
            self.surface.end_frame(frame);
        }
        self.surface.tile_provider.take_draw_error()
    }

    /// Ray intersection in the current scene coordinates.
    pub fn pick_world_coordinates(&self, ray: &Ray, frame: &FrameState, cull_back_faces: bool) -> Option<Cartesian3> {
        if self.is_destroyed() {
            return None;
        }
        let mode = terrain_mode(frame.mode);
        let projection = frame.uniform.map_projection.as_ref();
        let mut candidates = Vec::new();
        for handle in &self.surface.tiles_rendered_this_frame {
            let tile = handle.borrow();
            if let Some(sphere) = self.with_mesh(&tile, |mesh| Some(tile_sphere(&tile.rectangle, mesh, frame))) {
                if let Some(interval) = ray_sphere(ray, &sphere) {
                    candidates.push((interval.start, handle));
                }
            }
        }
        candidates.sort_by(|(a, _), (b, _)| a.total_cmp(b));
        let mut closest = None;
        let mut distance_squared = f64::INFINITY;
        for (_, handle) in candidates {
            if let Some(hit) =
                self.with_mesh(&handle.borrow(), |mesh| mesh.pick(ray, cull_back_faces, Some(mode), projection))
            {
                let distance = ray.origin.distance_squared(hit);
                if distance < distance_squared {
                    distance_squared = distance;
                    closest = Some(hit);
                }
            }
        }
        closest
    }

    /// Ray intersection in ellipsoid-fixed coordinates, including projected views.
    pub fn pick(&self, ray: &Ray, frame: &FrameState) -> Option<Cartesian3> {
        let hit = self.pick_world_coordinates(ray, frame, true)?;
        if frame.mode == SceneMode::Scene3D {
            return Some(hit);
        }
        let position = frame.uniform.map_projection.unproject(Cartesian3::new(hit.y, hit.z, hit.x));
        Some(self.ellipsoid.cartographic_to_cartesian(&position))
    }

    pub fn rendered_surface_tiles(&self) -> &[QuadtreeTileHandle] {
        &self.surface.tiles_rendered_this_frame
    }

    /// Intersects the deepest loaded mesh at the supplied longitude and latitude.
    pub fn get_height(&self, position: &Cartographic) -> Option<f64> {
        if self.is_destroyed() || self.surface.tiles_invalidated {
            return None;
        }
        let scheme = self.terrain_provider().tiling_scheme();
        let ellipsoid = scheme.ellipsoid();
        let mut current =
            self.surface.level_zero_tiles.as_ref()?.iter().find(|t| t.borrow().rectangle.contains(position))?.clone();
        let mut best = None;
        loop {
            let tile = current.borrow();
            let hit = self.with_mesh(&tile, |mesh| sample_mesh_height(mesh, position, ellipsoid, scheme.projection()));
            best = hit.or(best);
            let child = [&tile.southwest_child, &tile.southeast_child, &tile.northwest_child, &tile.northeast_child]
                .into_iter()
                .flatten()
                .find(|child| child.borrow().rectangle.contains(position))
                .cloned();
            drop(tile);
            let Some(child) = child else {
                return best;
            };
            current = child;
        }
    }

    fn with_mesh<T>(&self, tile: &QuadtreeTile, callback: impl FnOnce(&mut TerrainMesh) -> Option<T>) -> Option<T> {
        if let Some(surface) = tile.data.as_ref().and_then(|d| d.as_any().downcast_ref::<GlobeSurfaceTile>()) {
            if surface.has_rendered_mesh() {
                return callback(surface.mesh.try_borrow_mut().ok()?.as_mut()?);
            }
        }
        let mut mesh = self.surface.tile_provider.fill_mesh(tile)?.mesh.clone();
        callback(&mut mesh)
    }

    pub fn is_destroyed(&self) -> bool {
        self.surface.is_destroyed()
    }

    fn check_alive(&self) -> Result<(), RuntimeError> {
        if self.is_destroyed() {
            Err(RuntimeError::new("Globe was destroyed"))
        } else {
            Ok(())
        }
    }

    pub fn destroy(&mut self) -> Result<(), RuntimeError> {
        if self.is_destroyed() {
            return Ok(());
        }
        self.ocean_normal_map_future = None;
        self.ocean_normal_map_loader = None;
        self.depth_plane.destroy();
        self.show = false;
        self.surface.release_tiles();
        self.surface.destroy();
        self.surface.tile_provider.take_draw_error()
    }
}

impl<G: Gl + 'static> SceneGlobe for Globe<G> {
    fn show(&self) -> bool {
        self.show && !self.is_destroyed()
    }
    fn update(&mut self, frame: &mut FrameState) -> Result<(), RuntimeError> {
        Globe::update(self, frame)
    }
    fn render(&mut self, frame: &mut FrameState) -> Result<(), RuntimeError> {
        self.begin_frame(frame)?;
        Globe::render(self, frame)?;
        self.end_frame(frame)
    }
    fn pick(&self, ray: &Ray, frame: &FrameState) -> Option<Cartesian3> {
        Globe::pick(self, ray, frame)
    }
    fn rendered_surface_tiles(&self) -> &[QuadtreeTileHandle] {
        Globe::rendered_surface_tiles(self)
    }
    fn get_height(&self, position: &Cartographic) -> Option<f64> {
        Globe::get_height(self, position)
    }
    fn destroy(&mut self) -> Result<(), RuntimeError> {
        Globe::destroy(self)
    }
}

fn terrain_mode(mode: SceneMode) -> TerrainSceneMode {
    match mode {
        SceneMode::Morphing => TerrainSceneMode::Morphing,
        SceneMode::ColumbusView => TerrainSceneMode::ColumbusView,
        SceneMode::Scene2D => TerrainSceneMode::Scene2D,
        SceneMode::Scene3D => TerrainSceneMode::Scene3D,
    }
}

fn sample_mesh_height(
    mesh: &mut TerrainMesh,
    position: &Cartographic,
    ellipsoid: &Ellipsoid,
    projection: &dyn MapProjection,
) -> Option<f64> {
    let position = Cartographic::new(position.longitude, position.latitude, 0.0);
    let direction = ellipsoid.geodetic_surface_normal_cartographic(&position);
    let on_surface = ellipsoid.cartographic_to_cartesian(&position);
    let min = crate::core::vertical_exaggeration::get_height(
        mesh.minimum_height,
        mesh.encoding.exaggeration,
        mesh.encoding.exaggeration_relative_height,
    );
    let origin = ellipsoid
        .get_surface_normal_intersection_with_z_axis(on_surface, 11_500.0)
        .unwrap_or_else(|| on_surface.subtract(direction.multiply_by_scalar(min.min(-11_500.0).abs() + 1.0)));
    let point = mesh.pick(&Ray { origin, direction }, false, Some(TerrainSceneMode::Scene3D), projection)?;
    ellipsoid.cartesian_to_cartographic(point).map(|position| position.height)
}
