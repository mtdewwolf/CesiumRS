//! Terrain and imagery loading, tile selection support, and globe draw commands.

use super::{
    globe_surface_shader_set::{float_uniform, matrix_uniform, tile_uniform_map, GlobeImageryUniforms, Uniforms},
    globe_surface_tile::TerrainDataFuture,
    globe_translucency_state::localize_rectangle,
    imagery_layer::ImageryLayerHandle,
    imagery_texture_pipeline::ImageryImageUpload,
    quadtree_occluders::QuadtreeOccluders,
    quadtree_tile::{QuadtreeTile, QuadtreeTileHandle, WeakQuadtreeTileHandle},
    quadtree_tile_provider::QuadtreeTileProvider,
    tile_bounding_volume::{TileBoundingVolume, TileBoundingVolumeFrame},
    ClippingPlaneCollection, ClippingPolygonCollection, FrameState, GlobeClippingPlanesShader,
    GlobeClippingPolygonsShader, GlobeShaderFeature as Feature, GlobeShaderFeatures, GlobeSurfaceShaderOptions,
    GlobeSurfaceShaderSet, GlobeSurfaceTile, GlobeTranslucencyCommand, GlobeTranslucencyState, ImageryLayer,
    ImageryLayerCollection, ImageryState, ImageryTexturePipeline, Material, QuadtreeTileLoadState, SceneCommand,
    SceneDrawOptions, SceneMode, ShadowMode, SurfaceTerrainData, TerrainFillMesh, TerrainFillOptions, TerrainState,
    TileBoundingRegion, TileBoundingRegionOptions, TileSelectionResult,
};
use crate::{
    core::{
        math, terrain_mesh::TerrainMesh, webgl_constants as gl, BoundingSphere, Cartesian2, Cartesian3, Cartesian4,
        Cartographic, Color, Credit, Ellipsoid, EllipsoidalOccluder, Event, Intersect, MapProjection, NearFarScalar,
        NetworkTerrainData, PixelDatatype, PixelFormat, Rectangle, RuntimeError, TerrainData, TerrainMeshRequest,
        TerrainProvider, TerrainQuantization, TerrainTile, TileProviderError, TilingScheme, Visibility,
        WebMercatorProjection,
    },
    renderer::{
        ComputeCommand, ComputeEngine, Context, DrawMetadata, Gl, Pass, RenderState, SampledTexture, Sampler,
        ShaderSource, Texture, TextureOptions, UniformFrustum, UniformValue, VertexArray,
    },
};
use std::{
    cell::RefCell,
    collections::{HashMap, HashSet},
    future::poll_fn,
    rc::Rc,
    sync::Arc,
    task::Poll,
};

type TileKey = (u32, i32, i32);
type SampledTextures<G> = Vec<(String, Rc<dyn SampledTexture<G>>)>;

/// Object-safe terrain source used by the surface. A missing request is
/// retried on a later load pass.
pub trait GlobeTerrainSource {
    fn tiling_scheme(&self) -> Arc<dyn TilingScheme>;
    fn geometric_error(&self, level: u32) -> f64;
    fn request_geometry(&self, tile: TerrainTile) -> Option<TerrainDataFuture>;
    fn tile_available(&self, _tile: TerrainTile) -> Option<bool> {
        None
    }
    fn has_water_mask(&self) -> bool {
        false
    }
    fn has_vertex_normals(&self) -> bool {
        false
    }
    fn credit(&self) -> Option<&Credit> {
        None
    }
}

/// Adapts the borrowed futures of a Core terrain provider into tile-owned
/// requests. The provider is retained for the lifetime of an in-flight request.
pub struct GlobeTerrainProviderAdapter<P> {
    provider: Rc<P>,
    scheme: Arc<dyn TilingScheme>,
    credit: Option<Credit>,
}

impl<P: TerrainProvider + 'static> GlobeTerrainProviderAdapter<P> {
    pub fn new(provider: P, credit: Option<Credit>) -> Self {
        let provider = Rc::new(provider);
        // QuadtreeTile shares the scheme through Arc even for local-only schemes.
        #[allow(clippy::arc_with_non_send_sync)]
        let scheme = Arc::new(ProviderScheme(provider.clone()));
        Self { provider, scheme, credit }
    }
}

struct ProviderScheme<P>(Rc<P>);

impl<P: TerrainProvider> TilingScheme for ProviderScheme<P> {
    fn ellipsoid(&self) -> &Ellipsoid {
        self.0.tiling_scheme().ellipsoid()
    }
    fn rectangle(&self) -> &Rectangle {
        self.0.tiling_scheme().rectangle()
    }
    fn projection(&self) -> &dyn MapProjection {
        self.0.tiling_scheme().projection()
    }
    fn get_number_of_x_tiles_at_level(&self, level: u32) -> i32 {
        self.0.tiling_scheme().get_number_of_x_tiles_at_level(level)
    }
    fn get_number_of_y_tiles_at_level(&self, level: u32) -> i32 {
        self.0.tiling_scheme().get_number_of_y_tiles_at_level(level)
    }
    fn rectangle_to_native_rectangle(&self, rectangle: &Rectangle) -> Rectangle {
        self.0.tiling_scheme().rectangle_to_native_rectangle(rectangle)
    }
    fn tile_xy_to_native_rectangle(&self, x: f64, y: f64, level: u32) -> Rectangle {
        self.0.tiling_scheme().tile_xy_to_native_rectangle(x, y, level)
    }
    fn tile_xy_to_rectangle(&self, x: f64, y: f64, level: u32) -> Rectangle {
        self.0.tiling_scheme().tile_xy_to_rectangle(x, y, level)
    }
    fn position_to_tile_xy(&self, position: &Cartographic, level: u32) -> Option<Cartesian2> {
        self.0.tiling_scheme().position_to_tile_xy(position, level)
    }
}

impl<P> GlobeTerrainSource for GlobeTerrainProviderAdapter<P>
where
    P: TerrainProvider<Error = RuntimeError> + 'static,
    P::TerrainData: SurfaceTerrainData + 'static,
{
    fn tiling_scheme(&self) -> Arc<dyn TilingScheme> {
        self.scheme.clone()
    }
    fn geometric_error(&self, level: u32) -> f64 {
        self.provider.get_level_maximum_geometric_error(level)
    }
    fn request_geometry(&self, tile: TerrainTile) -> Option<TerrainDataFuture> {
        let provider = self.provider.clone();
        Some(Box::pin(async move {
            let future = poll_fn(|cx| {
                if let Some(future) = provider.request_tile_geometry(tile.x, tile.y, tile.level, None) {
                    Poll::Ready(future)
                } else {
                    cx.waker().wake_by_ref();
                    Poll::Pending
                }
            })
            .await;
            future.await.map(|data| Box::new(data) as Box<dyn SurfaceTerrainData>)
        }))
    }
    fn tile_available(&self, tile: TerrainTile) -> Option<bool> {
        self.provider.get_tile_data_available(tile.x, tile.y, tile.level)
    }
    fn has_water_mask(&self) -> bool {
        self.provider.has_water_mask()
    }
    fn has_vertex_normals(&self) -> bool {
        self.provider.has_vertex_normals()
    }
    fn credit(&self) -> Option<&Credit> {
        self.credit.as_ref()
    }
}

impl SurfaceTerrainData for NetworkTerrainData {
    fn interpolate_height(&self, rectangle: &Rectangle, longitude: f64, latitude: f64) -> Option<f64> {
        TerrainData::interpolate_height(self, rectangle, longitude, latitude)
    }
    fn is_child_available(&self, x: u32, y: u32, child_x: u32, child_y: u32) -> bool {
        TerrainData::is_child_available(self, x, y, child_x, child_y)
    }
    fn was_created_by_upsampling(&self) -> bool {
        TerrainData::was_created_by_upsampling(self)
    }
    fn create_surface_mesh(
        &mut self,
        request: TerrainMeshRequest<'_>,
    ) -> Option<Result<Arc<TerrainMesh>, RuntimeError>> {
        Some(match self {
            Self::Cesium3DTiles(data) => data.create_mesh_sync(request),
            Self::GoogleEarthEnterprise(data) => data.create_mesh_sync(request),
            Self::Heightmap { data, .. } => data.create_mesh_sync(request),
            Self::Quantized(data) => data.create_mesh_sync(request),
        })
    }
    fn upsample_surface(
        &self,
        scheme: &dyn TilingScheme,
        source: TerrainTile,
        destination: TerrainTile,
    ) -> Option<Box<dyn SurfaceTerrainData>> {
        let data = match self {
            Self::Cesium3DTiles(data) => Self::Cesium3DTiles(data.upsample_sync(scheme, source, destination)?.ok()?),
            Self::GoogleEarthEnterprise(data) => {
                Self::Quantized(data.upsample_quantized_sync(scheme, source, destination)?)
            }
            Self::Heightmap { data, credits } => {
                Self::Heightmap { data: data.upsample_sync(scheme, source, destination)?, credits: credits.clone() }
            }
            Self::Quantized(data) => Self::Quantized(data.upsample_sync(scheme, source, destination)?),
        };
        Some(Box::new(data))
    }
    fn water_mask(&self) -> Option<&[u8]> {
        TerrainData::water_mask(self)
    }
    fn credits(&self) -> Option<&[Credit]> {
        TerrainData::credits(self)
    }
}

#[derive(Clone, Debug)]
pub struct GlobeSurfaceOptions {
    pub base_color: Color,
    pub cartographic_limit_rectangle: Rectangle,
    pub enable_lighting: bool,
    pub dynamic_atmosphere_lighting: bool,
    pub dynamic_atmosphere_lighting_from_sun: bool,
    pub show_ground_atmosphere: bool,
    pub atmosphere_light_intensity: f64,
    pub atmosphere_rayleigh_coefficient: Cartesian3,
    pub atmosphere_mie_coefficient: Cartesian3,
    pub atmosphere_rayleigh_scale_height: f64,
    pub atmosphere_mie_scale_height: f64,
    pub atmosphere_mie_anisotropy: f64,
    pub lighting_fade_distance: [f64; 2],
    pub night_fade_distance: [f64; 2],
    pub atmosphere_hsb_shift: Cartesian3,
    pub show_water_effect: bool,
    pub shadows: ShadowMode,
    pub fill_highlight_color: Option<Color>,
    pub show_skirts: bool,
    pub back_face_culling: bool,
    pub vertex_shadow_darkness: f64,
    pub lambert_diffuse_multiplier: f64,
    pub underground_color: Option<Color>,
    pub underground_color_alpha_by_distance: Option<NearFarScalar>,
}

impl GlobeSurfaceOptions {
    pub fn new(ellipsoid: Ellipsoid) -> Self {
        let half_circumference = std::f64::consts::PI * ellipsoid.minimum_radius;
        Self {
            base_color: Color::new(0.0, 0.0, 0.5, 1.0),
            cartographic_limit_rectangle: Rectangle::MAX_VALUE,
            enable_lighting: false,
            dynamic_atmosphere_lighting: true,
            dynamic_atmosphere_lighting_from_sun: false,
            show_ground_atmosphere: ellipsoid == Ellipsoid::WGS84,
            atmosphere_light_intensity: 10.0,
            atmosphere_rayleigh_coefficient: Cartesian3::new(5.5e-6, 13.0e-6, 28.4e-6),
            atmosphere_mie_coefficient: Cartesian3::new(21e-6, 21e-6, 21e-6),
            atmosphere_rayleigh_scale_height: 10000.0,
            atmosphere_mie_scale_height: 3200.0,
            atmosphere_mie_anisotropy: 0.9,
            lighting_fade_distance: [half_circumference * 0.5, half_circumference],
            night_fade_distance: [half_circumference * 0.5, half_circumference * 2.5],
            atmosphere_hsb_shift: Cartesian3::ZERO,
            show_water_effect: true,
            shadows: ShadowMode::ReceiveOnly,
            fill_highlight_color: None,
            show_skirts: true,
            back_face_culling: true,
            vertex_shadow_darkness: 0.3,
            lambert_diffuse_multiplier: 0.9,
            underground_color: Some(Color::BLACK),
            underground_color_alpha_by_distance: Some(NearFarScalar::new(
                ellipsoid.maximum_radius / 1000.0,
                0.0,
                ellipsoid.maximum_radius / 5.0,
                1.0,
            )),
        }
    }
}

struct Fill<G: Gl> {
    terrain: TerrainFillMesh,
    vertices: Rc<VertexArray<G>>,
}

struct PolygonTextures<G: Gl> {
    extents: Rc<Texture<G>>,
    distance: Rc<Texture<G>>,
    source: super::clipping_polygon_collection::PackedClippingPolygons,
}

pub struct GlobeSurfaceTileProvider<G: Gl> {
    context: Context<G>,
    terrain: Rc<dyn GlobeTerrainSource>,
    pub imagery_layers: ImageryLayerCollection,
    pub imagery_layers_updated_event: Event<()>,
    pub options: GlobeSurfaceOptions,
    pub shader_set: GlobeSurfaceShaderSet<G>,
    pub translucency_state: GlobeTranslucencyState,
    pub clipping_planes: Option<ClippingPlaneCollection>,
    pub clipping_polygons: Option<ClippingPolygonCollection>,
    pub material: Option<Material>,
    pub ocean_normal_map: Option<Rc<Texture<G>>>,
    /// Optional textures from the current globe classification and depth passes.
    pub classification_texture: Option<Rc<Texture<G>>>,
    scene_classification_texture: Option<Rc<Texture<G>>>,
    polygon_textures: Option<PolygonTextures<G>>,
    image_upload: Option<ImageryImageUpload<G>>,
    compute: Rc<RefCell<ComputeEngine<G>>>,
    errors: Event<TileProviderError<String>>,
    draw_error: Option<RuntimeError>,
    tiles: HashMap<TileKey, WeakQuadtreeTileHandle>,
    fills: HashMap<TileKey, Fill<G>>,
    layer_signature: Vec<(u64, bool, bool)>,
    texture_limit: usize,
    previous_selection_frame: Option<u32>,
    destroyed: bool,
}

impl<G: Gl + 'static> GlobeSurfaceTileProvider<G> {
    pub fn new(context: &Context<G>, terrain: Rc<dyn GlobeTerrainSource>) -> Self {
        let options = GlobeSurfaceOptions::new(*terrain.tiling_scheme().ellipsoid());
        Self {
            context: context.clone(),
            terrain,
            options,
            imagery_layers: ImageryLayerCollection::new(),
            imagery_layers_updated_event: Event::new(),
            shader_set: GlobeSurfaceShaderSet::new(context),
            translucency_state: GlobeTranslucencyState::default(),
            clipping_planes: None,
            clipping_polygons: None,
            material: None,
            ocean_normal_map: None,
            classification_texture: None,
            scene_classification_texture: None,
            polygon_textures: None,
            image_upload: None,
            compute: Rc::new(RefCell::new(ComputeEngine::new(context))),
            errors: Event::new(),
            draw_error: None,
            tiles: HashMap::new(),
            fills: HashMap::new(),
            layer_signature: Vec::new(),
            texture_limit: 0,
            previous_selection_frame: None,
            destroyed: false,
        }
    }

    pub fn terrain_provider(&self) -> &Rc<dyn GlobeTerrainSource> {
        &self.terrain
    }
    pub fn error_event_mut(&mut self) -> &mut Event<TileProviderError<String>> {
        &mut self.errors
    }

    pub(crate) fn replace_terrain_provider(&mut self, terrain: Rc<dyn GlobeTerrainSource>) {
        self.terrain = terrain;
        self.tiles.clear();
        self.fills.clear();
    }

    pub(crate) fn loaded_tiles(&self) -> Vec<QuadtreeTileHandle> {
        self.tiles.values().filter_map(WeakQuadtreeTileHandle::upgrade).collect()
    }

    pub(crate) fn refresh_exaggerated_tile(&self, tile: &QuadtreeTileHandle) -> Result<(), RuntimeError> {
        let mut tile = tile.borrow_mut();
        let ellipsoid = *tile.tiling_scheme.ellipsoid();
        let Some(surface) = surface_mut(&mut tile) else {
            return Ok(());
        };
        let reupload = {
            let mut mesh = surface.mesh.borrow_mut();
            let Some(mesh) = mesh.as_mut() else {
                return Ok(());
            };
            let previous = mesh.encoding.has_geodetic_surface_normals;
            let current = mesh.encoding.exaggeration != 1.0;
            if previous != current {
                let mut vertices = vec![
                    0.0;
                    mesh.vertices.len() / mesh.stride
                        * if current { mesh.stride + 3 } else { mesh.stride - 3 }
                ];
                if current {
                    mesh.encoding.add_geodetic_surface_normals(&mesh.vertices, &mut vertices, &ellipsoid);
                } else {
                    mesh.encoding.remove_geodetic_surface_normals(&mesh.vertices, &mut vertices);
                }
                mesh.vertices = vertices;
                mesh.stride = mesh.encoding.stride;
            }
            previous != current
        };
        if reupload {
            surface.create_vertex_array(&self.context)?;
        }
        Ok(())
    }

    pub fn set_image_upload(&mut self, upload: ImageryImageUpload<G>) {
        self.image_upload = Some(upload);
    }

    pub(crate) fn take_draw_error(&mut self) -> Result<(), RuntimeError> {
        self.draw_error.take().map_or(Ok(()), Err)
    }

    pub fn update_imagery_layers(&mut self) {
        self.imagery_layers.update();
        let signature: Vec<_> = self
            .imagery_layers
            .layers()
            .iter()
            .map(|handle| {
                let layer = handle.borrow();
                (layer.id(), layer.show, layer.ready())
            })
            .collect();
        if self.layer_signature == signature {
            return;
        }
        for handle in self.imagery_layers.layers() {
            let mut layer = handle.borrow_mut();
            if !layer.has_texture_host() {
                let pipeline = self.image_upload.as_ref().map_or_else(
                    || ImageryTexturePipeline::new(&self.context),
                    |upload| ImageryTexturePipeline::with_image_upload(&self.context, upload.clone()),
                );
                layer.set_texture_host(Rc::new(pipeline));
            }
        }
        self.layer_signature = signature;
        self.tiles.retain(|_, tile| tile.strong_count() != 0);
        for tile in self.tiles.values().filter_map(|tile| tile.upgrade()) {
            let mut tile = tile.borrow_mut();
            let error = self.terrain.geometric_error(tile.level);
            let (rectangle, level) = (tile.rectangle, tile.level);
            if let Some(surface) = surface_mut(&mut tile) {
                if sync_imagery(surface, self.imagery_layers.layers(), rectangle, level, error) {
                    tile.state = QuadtreeTileLoadState::Loading;
                }
            }
        }
        self.imagery_layers_updated_event.raise_event(&());
    }

    pub fn prepare_render(&mut self, frame: &mut FrameState) -> Result<(), RuntimeError> {
        self.texture_limit = self.context.limits()?.maximum_texture_image_units as usize;
        self.update_imagery_layers();
        self.prepare_classification(frame)?;
        if let Some(planes) = self.clipping_planes.as_mut().filter(|planes| planes.enabled) {
            planes.update(&self.context)?;
        }
        let mut compute = Vec::new();
        self.imagery_layers.queue_reprojection_commands(&mut compute)?;
        if let Some(command) = self.prepare_polygons()? {
            compute.push(command);
        }
        for mut command in compute {
            let engine = self.compute.clone();
            let command = RefCell::new(Some({
                command.persists = true;
                command
            }));
            frame.command_list.push(SceneCommand::new(
                "globe-compute",
                Pass::Compute,
                Rc::new(move |_| {
                    if let Some(mut command) = command.borrow_mut().take() {
                        command.execute(&mut engine.borrow_mut())?;
                    }
                    Ok(())
                }),
            ));
        }
        if let Some(material) = &mut self.material {
            material.update_textures()?;
        }
        let material = self.material.as_ref().filter(|material| {
            self.terrain.has_vertex_normals()
                || !(material.shader_source.contains("slope") || material.shader_source.contains("normalEC"))
        });
        self.shader_set.set_material(material.map(|material| material.shader_source.clone()))?;
        if let Some(credit) = self.terrain.credit() {
            push_credit(frame, credit);
        }
        for layer in self.imagery_layers.layers() {
            if let Some(credit) = layer.borrow().imagery_provider().and_then(|provider| provider.credit().cloned()) {
                push_credit(frame, &credit);
            }
        }
        Ok(())
    }

    fn prepare_classification(&mut self, frame: &mut FrameState) -> Result<(), RuntimeError> {
        self.scene_classification_texture = None;
        frame.globe_scene_classification = false;
        if !self.translucency_state.translucent()
            || frame.passes.depth
            || !frame.command_list.iter().any(|command| command.pass == Pass::TerrainClassification)
        {
            return Ok(());
        }
        let Some(resources) = frame
            .globe_view_resources
            .as_ref()
            .and_then(|resource| resource.get::<Rc<RefCell<super::ViewResources<G>>>>())
        else {
            return Ok(());
        };
        let mut resources = resources.borrow_mut();
        let buffers = &mut resources.globe_translucency_framebuffer;
        let viewport = crate::renderer::Viewport {
            x: 0,
            y: 0,
            width: frame.drawing_buffer_width,
            height: frame.drawing_buffer_height,
        };
        buffers.update_and_clear(
            frame.high_dynamic_range,
            viewport,
            &self.context,
            &crate::renderer::PassState::default(),
        )?;
        self.scene_classification_texture = buffers.classification_texture()?.cloned();
        frame.globe_scene_classification = true;
        Ok(())
    }

    fn prepare_polygons(&mut self) -> Result<Option<ComputeCommand<G>>, RuntimeError> {
        let Some(polygons) =
            self.clipping_polygons.as_mut().filter(|p| p.enabled && !p.is_empty() && self.context.webgl2())
        else {
            self.polygon_textures = None;
            return Ok(None);
        };
        let packed = polygons.pack();
        if self.polygon_textures.as_ref().is_some_and(|textures| textures.source == packed) {
            return Ok(None);
        }
        let maximum = self.context.limits()?.maximum_texture_size;
        let [extent_width, extent_height] = polygons.extents_texture_resolution(maximum, None);
        let [polygon_width, polygon_height] = polygons.polygons_texture_resolution(maximum, None);
        let extents = float_texture(&self.context, extent_width, extent_height, PixelFormat::Rgba, &packed.extents)?;
        let positions = float_texture(&self.context, polygon_width, polygon_height, PixelFormat::Rg, &packed.polygons)?;
        let [width, height] = polygons.clipping_distance_texture_resolution(maximum);
        // R8 retains the sign of the normalized distance and is color
        // renderable in WebGL2 without a float color-buffer extension.
        let distance = Rc::new(Texture::new(
            &self.context,
            TextureOptions { pixel_format: PixelFormat::Red, flip_y: false, ..TextureOptions::rgba(width, height) },
            None,
        )?);
        let lengths = [polygons.length() as i32, polygons.extents_count() as i32];
        let command = ComputeCommand {
            fragment_shader_source: Some(ShaderSource::new(crate::shaders::root::POLYGON_SIGNED_DISTANCE_FS.source)),
            uniform_map: Some(Rc::new(move || {
                Ok(vec![
                    ("u_polygonsLength".into(), UniformValue::Int(vec![lengths[0]])),
                    ("u_extentsLength".into(), UniformValue::Int(vec![lengths[1]])),
                ])
            })),
            textures: vec![("u_polygonTexture".into(), positions), ("u_extentsTexture".into(), extents.clone())],
            output_texture: Some(distance.clone()),
            ..ComputeCommand::default()
        };
        self.polygon_textures = Some(PolygonTextures { extents, distance, source: packed });
        Ok(Some(command))
    }

    fn underground_visible(&self, frame: &FrameState) -> bool {
        frame.camera_underground
            || self.translucency_state.translucent()
            || !self.options.back_face_culling
                && (self.clipping_planes.as_ref().is_some_and(|p| p.enabled)
                    || self.clipping_polygons.as_ref().is_some_and(|p| p.enabled)
                    || self.options.cartographic_limit_rectangle != Rectangle::MAX_VALUE)
    }

    fn load_surface(&mut self, frame: &FrameState, handle: &QuadtreeTileHandle) -> Result<(), RuntimeError> {
        let mut tile = handle.borrow_mut();
        let identity = tile_identity(&tile);
        let scheme = tile.tiling_scheme.clone();
        let parent = tile.parent.as_ref().and_then(|p| p.upgrade());
        let available = self.terrain.tile_available(identity).or_else(|| {
            let parent = parent.as_ref()?.borrow();
            let data = surface(&parent)?.terrain_data.as_ref()?;
            Some(data.is_child_available(parent.x as u32, parent.y as u32, identity.x, identity.y))
        });
        GlobeSurfaceTile::initialize(&mut tile, available);
        self.tiles.insert(tile_key(&tile), Rc::downgrade(handle));
        let mut data = tile.data.take().expect("surface tile was initialized");
        let surface = data.as_any_mut().downcast_mut::<GlobeSurfaceTile>().expect("provider owns tile data");
        let result = self.advance_tile(frame, &mut tile, surface, parent, scheme.as_ref(), identity);
        tile.data = Some(data);
        result
    }

    fn advance_tile(
        &mut self,
        frame: &FrameState,
        tile: &mut QuadtreeTile,
        surface: &mut GlobeSurfaceTile,
        parent: Option<QuadtreeTileHandle>,
        scheme: &dyn TilingScheme,
        identity: TerrainTile,
    ) -> Result<(), RuntimeError> {
        if surface.terrain_state == TerrainState::Unloaded {
            if let Some(future) = self.terrain.request_geometry(identity) {
                surface.request_terrain_data(future);
            } else if let Some(request) = &frame.render_request {
                request.request_render();
            }
        }
        if surface.terrain_state == TerrainState::Receiving {
            let waker = frame.render_request.as_ref().map(super::SceneRenderRequest::waker);
            if let Err(error) = surface.poll_terrain_data_with_waker(waker.as_ref().unwrap_or(std::task::Waker::noop()))
            {
                let options = crate::core::tile_provider_error::TileProviderErrorOptions {
                    provider: "GlobeSurfaceTileProvider".to_owned(),
                    message: error.to_string(),
                    x: Some(identity.x),
                    y: Some(identity.y),
                    level: Some(identity.level),
                    error: Some(error.to_string()),
                };
                self.errors.raise_event(&TileProviderError::new(options));
            }
        }
        if surface.terrain_state == TerrainState::Failed {
            let Some(parent) = parent.as_ref() else {
                tile.state = QuadtreeTileLoadState::Failed;
                return Ok(());
            };
            let parent = parent.borrow();
            if let Some(parent_data) = self::surface(&parent).and_then(|parent| parent.terrain_data.as_ref()) {
                surface.terrain_data = parent_data.upsample_surface(scheme, tile_identity(&parent), identity);
                if surface.terrain_data.is_some() {
                    surface.terrain_state = TerrainState::Received;
                } else if parent.state == QuadtreeTileLoadState::Done {
                    tile.state = QuadtreeTileLoadState::Failed;
                    return Ok(());
                }
            } else if parent.state == QuadtreeTileLoadState::Failed {
                tile.state = QuadtreeTileLoadState::Failed;
                return Ok(());
            }
        }
        if surface.terrain_state == TerrainState::Received {
            let mut request = TerrainMeshRequest::new(scheme, identity);
            request.exaggeration = frame.vertical_exaggeration;
            request.exaggeration_relative_height = frame.vertical_exaggeration_relative_height;
            let data = surface.terrain_data.as_mut().expect("received terrain");
            if let Some(mesh) = data.create_surface_mesh(request) {
                surface.set_mesh((*mesh?).clone());
                if let Some(bytes) =
                    surface.terrain_data.as_ref().and_then(|data| data.water_mask()).map(<[u8]>::to_vec)
                {
                    if !bytes.is_empty() && (bytes.len() > 1 || bytes[0] > 0) {
                        let size = bytes.len().isqrt() as u32;
                        surface.upload_water_mask(&self.context, size, size, &bytes)?;
                    }
                }
            }
        }
        if surface.terrain_state == TerrainState::Transformed {
            surface.create_vertex_array(&self.context)?;
            let mesh = surface.mesh.borrow();
            if let Some(mesh) = mesh.as_ref() {
                let (min, max) = mesh_heights(mesh, frame);
                surface.tile_bounding_region = Some(region(tile, min, max));
                surface.bounding_volume_is_from_mesh = true;
                surface.occludee_point_in_scaled_space = mesh
                    .occludee_point_in_scaled_space
                    .or_else(|| {
                        let center = mesh.oriented_bounding_box?.center;
                        let ellipsoid = tile.tiling_scheme.ellipsoid();
                        let rectangle = tile.rectangle;
                        let corners = [
                            (rectangle.west, rectangle.south),
                            (rectangle.east, rectangle.south),
                            (rectangle.west, rectangle.north),
                            (rectangle.east, rectangle.north),
                        ]
                        .map(|(longitude, latitude)| {
                            ellipsoid.cartographic_to_cartesian(&Cartographic::new(longitude, latitude, max))
                        });
                        EllipsoidalOccluder::new(ellipsoid, None)
                            .compute_horizon_culling_point_possibly_under_ellipsoid(center, &corners, Some(min))
                    })
                    .unwrap_or(Cartesian3::ZERO);
            }
        }
        if surface.terrain_state == TerrainState::Ready {
            let upsampled = surface.terrain_data.as_ref().is_some_and(|data| data.was_created_by_upsampling());
            if upsampled && !surface.has_water_mask_texture {
                let mut ancestor = parent;
                while let Some(handle) = ancestor {
                    let tile_ancestor = handle.borrow();
                    if let Some(data) = self::surface(&tile_ancestor) {
                        if surface.inherit_water_mask_from_ancestor::<G>(
                            &tile.rectangle,
                            data,
                            &tile_ancestor.rectangle,
                        ) {
                            break;
                        }
                    }
                    ancestor = tile_ancestor.parent.as_ref().and_then(|p| p.upgrade());
                }
            }
            tile.upsampled_from_parent = upsampled;
            let was_renderable = tile.renderable;
            tile.renderable = true;
            let ready = if tile.last_selection_result != TileSelectionResult::CulledButNeeded
                && localize_rectangle(&tile.rectangle, &self.options.cartographic_limit_rectangle).is_some()
            {
                sync_imagery(
                    surface,
                    self.imagery_layers.layers(),
                    tile.rectangle,
                    tile.level,
                    self.terrain.geometric_error(tile.level),
                );
                surface.process_imagery_with_collection(tile, &self.imagery_layers, Some(frame), false)
            } else {
                self.imagery_layers.is_empty()
            };
            tile.renderable |= was_renderable;
            if ready {
                tile.state = QuadtreeTileLoadState::Done;
            }
            if let Some(request) = &frame.render_request {
                // Terrain completion and imagery transitions may have advanced
                // synchronously, after the current frame selected its tiles.
                if ready || tile.renderable != was_renderable {
                    request.request_render();
                }
            }
        }
        Ok(())
    }
}

fn surface(tile: &QuadtreeTile) -> Option<&GlobeSurfaceTile> {
    tile.data.as_ref()?.as_any().downcast_ref()
}

fn surface_mut(tile: &mut QuadtreeTile) -> Option<&mut GlobeSurfaceTile> {
    tile.data.as_mut()?.as_any_mut().downcast_mut()
}

fn tile_identity(tile: &QuadtreeTile) -> TerrainTile {
    TerrainTile { x: tile.x as u32, y: tile.y as u32, level: tile.level }
}

fn tile_key(tile: &QuadtreeTile) -> TileKey {
    (tile.level, tile.x, tile.y)
}

fn push_credit(frame: &mut FrameState, credit: &Credit) {
    if !frame.credits.contains(credit) {
        frame.credits.push(credit.clone());
    }
}

fn sync_imagery(
    surface: &mut GlobeSurfaceTile,
    layers: &[ImageryLayerHandle],
    rectangle: Rectangle,
    level: u32,
    error: f64,
) -> bool {
    let visible: HashSet<_> = layers
        .iter()
        .filter_map(|layer| {
            let layer = layer.borrow();
            layer.show.then_some(layer.id())
        })
        .collect();
    surface.imagery.retain_mut(|tile| {
        let keep = tile.layer_id().is_some_and(|id| visible.contains(&id))
            && !tile.loading_imagery.as_ref().is_some_and(|imagery| {
                imagery.borrow().state == ImageryState::Placeholder
                    && tile.imagery_layer.as_ref().is_some_and(|layer| layer.borrow().ready())
            });
        if !keep {
            tile.free_resources();
        }
        keep
    });
    let mut added = false;
    for layer in layers {
        let (show, id) = {
            let layer = layer.borrow();
            (layer.show, layer.id())
        };
        if show && !surface.imagery.iter().any(|tile| tile.layer_id() == Some(id)) {
            let skeletons = ImageryLayer::create_tile_imagery_skeletons_for_handle(layer, &rectangle, level, error);
            added |= !skeletons.is_empty();
            surface.imagery.extend(skeletons);
        }
    }
    surface.imagery.sort_by_key(|tile| tile.imagery_layer.as_ref().map_or(-1, |layer| layer.borrow().layer_index()));
    added
}

fn float_texture<G: Gl>(
    context: &Context<G>,
    width: u32,
    height: u32,
    format: PixelFormat,
    values: &[f32],
) -> Result<Rc<Texture<G>>, RuntimeError> {
    let components = if format == PixelFormat::Rg { 2 } else { 4 };
    let mut bytes: Vec<_> = values.iter().flat_map(|value| value.to_le_bytes()).collect();
    bytes.resize(width as usize * height as usize * components * 4, 0);
    Ok(Rc::new(Texture::new(
        context,
        TextureOptions {
            pixel_format: format,
            pixel_datatype: PixelDatatype::Float,
            flip_y: false,
            sampler: Sampler::NEAREST,
            ..TextureOptions::rgba(width, height)
        },
        Some(&bytes),
    )?))
}

fn mesh_heights(mesh: &TerrainMesh, frame: &FrameState) -> (f64, f64) {
    let exaggerate = |h| {
        crate::core::vertical_exaggeration::get_height(
            h,
            frame.vertical_exaggeration,
            frame.vertical_exaggeration_relative_height,
        )
    };
    let a = exaggerate(mesh.minimum_height);
    let b = exaggerate(mesh.maximum_height);
    (a.min(b), a.max(b))
}

fn region(tile: &QuadtreeTile, min: f64, max: f64) -> TileBoundingRegion {
    TileBoundingRegion::new(TileBoundingRegionOptions {
        minimum_height: Some(min),
        maximum_height: Some(max),
        ellipsoid: Some(*tile.tiling_scheme.ellipsoid()),
        ..TileBoundingRegionOptions::new(tile.rectangle)
    })
}

fn terrain_range(tile: &QuadtreeTile, frame: &FrameState) -> Option<(f64, f64, bool)> {
    let heights = |tile: &QuadtreeTile| {
        let surface = surface(tile)?;
        let mesh = surface.mesh.borrow();
        mesh.as_ref().map(|mesh| mesh_heights(mesh, frame))
    };
    if let Some((min, max)) = heights(tile) {
        return Some((min, max, true));
    }
    let mut parent = tile.parent.as_ref().and_then(|p| p.upgrade());
    while let Some(handle) = parent {
        let ancestor = handle.borrow();
        if let Some((min, max)) = heights(&ancestor) {
            return Some((min, max, false));
        }
        parent = ancestor.parent.as_ref().and_then(|p| p.upgrade());
    }
    None
}

pub(crate) fn tile_sphere(rectangle: &Rectangle, mesh: &TerrainMesh, frame: &FrameState) -> BoundingSphere {
    let (min, max) = mesh_heights(mesh, frame);
    if frame.mode == SceneMode::Scene3D {
        if frame.vertical_exaggeration == 1.0 {
            return mesh.bounding_sphere_3d;
        }
        return crate::core::OrientedBoundingBox::from_rectangle(
            rectangle,
            min,
            max,
            frame.uniform.map_projection.ellipsoid(),
        )
        .as_ref()
        .map_or(mesh.bounding_sphere_3d, BoundingSphere::from_oriented_bounding_box);
    }
    let sphere = projected_sphere(rectangle, min, max, frame);
    if frame.mode == SceneMode::Morphing {
        sphere.union(&mesh.bounding_sphere_3d)
    } else {
        sphere
    }
}

fn projected_sphere(rectangle: &Rectangle, min: f64, max: f64, frame: &FrameState) -> BoundingSphere {
    let mut sphere = BoundingSphere::from_rectangle_with_heights_2d(
        rectangle,
        Some(frame.uniform.map_projection.as_ref()),
        min,
        max,
    );
    let c = sphere.center;
    sphere.center = Cartesian3::new(c.z, c.x, c.y);
    sphere
}

impl<G: Gl + 'static> QuadtreeTileProvider for GlobeSurfaceTileProvider<G> {
    fn tiling_scheme(&self) -> Arc<dyn TilingScheme> {
        self.terrain.tiling_scheme()
    }
    fn ready(&self) -> bool {
        !self.destroyed
    }
    fn error_event(&self) -> Option<&Event<TileProviderError<String>>> {
        Some(&self.errors)
    }
    fn get_level_maximum_geometric_error(&self, level: u32) -> f64 {
        self.terrain.geometric_error(level)
    }

    fn update(&mut self, frame: &FrameState) {
        let waker = frame.render_request.as_ref().map(super::SceneRenderRequest::waker);
        let mut task = std::task::Context::from_waker(waker.as_ref().unwrap_or(std::task::Waker::noop()));
        self.imagery_layers.poll_providers(&mut task);
        self.update_imagery_layers();
    }

    fn begin_update(&mut self, frame: &FrameState) {
        self.previous_selection_frame = frame.frame_number.checked_sub(1);
    }

    fn load_tile(&mut self, frame: &FrameState, tile: &QuadtreeTileHandle) {
        if let Err(error) = self.load_surface(frame, tile) {
            self.draw_error.get_or_insert(error);
        }
    }

    fn compute_distance_to_tile(&self, tile: &QuadtreeTile, frame_state: &FrameState) -> f64 {
        let Some((mut min, mut max, own)) = terrain_range(tile, frame_state) else { return 9_999_999_999.0 };
        let camera = &frame_state.uniform.camera;
        let Some(cartographic) = camera.cartographic else { return 9_999_999_999.0 };
        if !own {
            if (cartographic.height - min).abs() > (cartographic.height - max).abs() {
                max = min;
            } else {
                min = max;
            }
        }
        region(tile, min, max).distance_to_camera(&TileBoundingVolumeFrame {
            mode: frame_state.mode,
            camera_position_wc: camera.position,
            camera_position_cartographic: cartographic,
            map_projection: frame_state.uniform.map_projection.as_ref(),
        })
    }

    fn compute_tile_visibility(
        &self,
        tile: &QuadtreeTile,
        frame: &FrameState,
        occluders: Option<&QuadtreeOccluders>,
    ) -> Visibility {
        if localize_rectangle(&tile.rectangle, &self.options.cartographic_limit_rectangle).is_none() {
            return Visibility::None;
        }
        let underground = self.underground_visible(frame);
        let Some((min, max, _)) = terrain_range(tile, frame) else { return Visibility::Partial };
        if !underground
            && frame.fog.enabled
            && math::fog(self.compute_distance_to_tile(tile, frame), frame.fog.density) >= 1.0
        {
            return Visibility::None;
        }
        let region = region(tile, min, max);
        let projected;
        let mut volume: &dyn crate::core::culling_volume::BoundingVolume = if let Some(obb) = region.bounding_volume() {
            obb
        } else if let Some(sphere) = region.bounding_sphere_opt() {
            sphere
        } else {
            return Visibility::Partial;
        };
        if frame.mode != SceneMode::Scene3D {
            let sphere = projected_sphere(&tile.rectangle, min, max, frame);
            projected = if frame.mode == SceneMode::Morphing {
                region.bounding_sphere_opt().map_or(sphere, |region| sphere.union(region))
            } else {
                sphere
            };
            volume = &projected;
        }
        if let Some(planes) = self.clipping_planes.as_ref().filter(|p| p.enabled) {
            if matches!(planes.compute_intersection(volume, None), Ok(Intersect::Outside)) {
                return Visibility::None;
            }
        }
        let visible = frame.culling_volume.compute_visibility(volume);
        let ortho = matches!(
            frame.uniform.camera.frustum,
            UniformFrustum::Orthographic(_) | UniformFrustum::OrthographicOffCenter(_)
        );
        if visible != Intersect::Outside
            && !underground
            && !ortho
            && frame.mode == SceneMode::Scene3D
            && occluders.zip(surface(tile)).is_some_and(|(occluder, surface)| {
                surface.occludee_point_in_scaled_space != Cartesian3::ZERO
                    && !occluder.ellipsoid().is_scaled_space_point_visible_possibly_under_ellipsoid(
                        surface.occludee_point_in_scaled_space,
                        Some(min),
                    )
            })
        {
            return Visibility::None;
        }
        match visible {
            Intersect::Inside => Visibility::Full,
            Intersect::Intersecting => Visibility::Partial,
            Intersect::Outside => Visibility::None,
        }
    }

    fn can_refine(&self, tile: &QuadtreeTile) -> bool {
        surface(tile).is_some_and(|surface| surface.terrain_data.is_some())
            || self
                .terrain
                .tile_available(TerrainTile { x: tile.x as u32 * 2, y: tile.y as u32 * 2, level: tile.level + 1 })
                .is_some()
    }

    fn can_render_without_losing_detail(&self, tile: &QuadtreeTile) -> bool {
        let parent = surface(tile);
        let parent_ready = parent.is_some_and(|data| data.terrain_state == TerrainState::Ready);
        let waiting: HashSet<_> = parent
            .into_iter()
            .flat_map(|data| data.imagery.iter())
            .filter(|i| imagery_pending(i))
            .filter_map(|i| i.layer_id())
            .collect();
        let mut descendants = children(tile).collect::<Vec<_>>();
        while let Some(handle) = descendants.pop() {
            let descendant = handle.borrow();
            if descendant.last_selection_result_frame != self.previous_selection_frame {
                continue;
            }
            match descendant.last_selection_result {
                TileSelectionResult::Refined => descendants.extend(children(&descendant)),
                TileSelectionResult::Rendered => {
                    if let Some(data) = surface(&descendant) {
                        if !parent_ready && data.terrain_state == TerrainState::Ready {
                            return false;
                        }
                        if data.imagery.iter().any(|i| {
                            !imagery_pending(i)
                                && (parent.is_none() || i.layer_id().is_some_and(|id| waiting.contains(&id)))
                        }) {
                            return false;
                        }
                    }
                }
                _ => {}
            }
        }
        true
    }

    fn compute_tile_load_priority(&self, tile: &QuadtreeTile, frame: &FrameState) -> f64 {
        let Some((min, max, _)) = terrain_range(tile, frame) else { return 0.0 };
        let world = region(tile, min, max);
        let Some(bounds) = world.bounding_volume() else { return 0.0 };
        let delta = bounds.center.subtract(frame.uniform.camera.position);
        if delta.magnitude() < 1.0e-5 {
            return 0.0;
        }
        (1.0 - delta.normalize().dot(frame.uniform.camera.direction)) * tile.distance
    }

    fn render_tiles(&mut self, tiles: &[QuadtreeTileHandle], frame: &mut FrameState) {
        if let Err(error) = self.build_fills(tiles, frame) {
            self.draw_error.get_or_insert(error);
        }
        for tile in tiles {
            self.show_tile_this_frame(tile, frame);
        }
    }

    fn show_tile_this_frame(&mut self, tile: &QuadtreeTileHandle, frame: &mut FrameState) {
        let tile = tile.borrow();
        let fill = self.fills.remove(&tile_key(&tile));
        if let Err(error) = self.draw_tile(&tile, fill.as_ref(), frame) {
            self.draw_error.get_or_insert(error);
        }
        if let Some(fill) = fill {
            self.fills.insert(tile_key(&tile), fill);
        }
    }

    fn cancel_reprojections(&mut self) {
        self.imagery_layers.cancel_reprojections();
    }

    fn destroy(&mut self) {
        if self.destroyed {
            return;
        }
        self.tiles.clear();
        self.fills.clear();
        self.polygon_textures = None;
        self.ocean_normal_map = None;
        self.classification_texture = None;
        self.scene_classification_texture = None;
        self.material = None;
        if let Some(mut polygons) = self.clipping_polygons.take() {
            polygons.destroy();
        }
        let results = [
            self.clipping_planes.take().map_or(Ok(()), |mut planes| planes.destroy()),
            self.imagery_layers.destroy(),
            self.shader_set.destroy(),
            self.compute.borrow_mut().destroy(),
        ];
        for error in results.into_iter().filter_map(Result::err) {
            self.draw_error.get_or_insert(error);
        }
        self.destroyed = true;
    }

    fn is_destroyed(&self) -> bool {
        self.destroyed
    }
}

fn imagery_pending(imagery: &super::TileImagery) -> bool {
    imagery
        .loading_imagery
        .as_ref()
        .is_some_and(|i| !matches!(i.borrow().state, ImageryState::Failed | ImageryState::Invalid))
}

fn children(tile: &QuadtreeTile) -> impl Iterator<Item = QuadtreeTileHandle> {
    [
        tile.northwest_child.clone(),
        tile.northeast_child.clone(),
        tile.southwest_child.clone(),
        tile.southeast_child.clone(),
    ]
    .into_iter()
    .flatten()
}

impl<G: Gl + 'static> GlobeSurfaceTileProvider<G> {
    fn build_fills(&mut self, tiles: &[QuadtreeTileHandle], frame: &FrameState) -> Result<(), RuntimeError> {
        self.fills.clear();
        let mut missing = Vec::new();
        let mut real = Vec::new();
        for handle in tiles {
            let tile = handle.borrow();
            if surface(&tile).is_some_and(GlobeSurfaceTile::has_rendered_mesh) {
                real.push(tile);
            } else {
                missing.push(tile);
            }
        }
        if missing.is_empty() {
            return Ok(());
        }
        let mesh_guards: Vec<_> =
            real.iter().filter_map(|tile| surface(tile)).map(|surface| surface.mesh.borrow()).collect();
        let meshes: Vec<_> = mesh_guards.iter().filter_map(|m| m.as_ref()).collect();
        let mut generated: Vec<(TileKey, Fill<G>)> = Vec::new();
        while !missing.is_empty() {
            let neighbors: Vec<_> =
                meshes.iter().copied().chain(generated.iter().map(|(_, fill)| &fill.terrain.mesh)).collect();
            let index = missing
                .iter()
                .position(|tile| {
                    neighbors.iter().any(|mesh| TerrainFillMesh::shares_edge(&tile.rectangle, &mesh.rectangle))
                })
                .unwrap_or(0);
            let tile = missing.remove(index);
            // Use unexaggerated ancestor heights. Fill encoding applies the
            // same frame exaggeration as an ordinary mesh.
            let parent = tile.parent.as_ref().and_then(|p| p.upgrade());
            let (min, max) = parent
                .as_ref()
                .and_then(|parent| {
                    let parent = parent.borrow();
                    let mesh = surface(&parent)?.mesh.borrow();
                    mesh.as_ref().map(|mesh| (mesh.minimum_height, mesh.maximum_height))
                })
                .unwrap_or((0.0, 0.0));
            let fill = TerrainFillMesh::new(
                tile.rectangle,
                tile.tiling_scheme.ellipsoid(),
                TerrainFillOptions {
                    minimum_height: min,
                    maximum_height: max,
                    geometric_error: self.terrain.geometric_error(tile.level),
                    exaggeration: frame.vertical_exaggeration,
                    exaggeration_relative_height: frame.vertical_exaggeration_relative_height,
                },
                &neighbors,
            );
            let vertices = GlobeSurfaceTile::upload_mesh(&self.context, &fill.mesh)?;
            generated.push((tile_key(&tile), Fill { terrain: fill, vertices }));
        }
        self.fills.extend(generated);
        Ok(())
    }

    pub fn fill_mesh(&self, tile: &QuadtreeTile) -> Option<&TerrainFillMesh> {
        self.fills.get(&tile_key(tile)).map(|fill| &fill.terrain)
    }

    fn draw_tile(
        &mut self,
        tile: &QuadtreeTile,
        fill: Option<&Fill<G>>,
        frame: &mut FrameState,
    ) -> Result<(), RuntimeError> {
        let Some(limit) = localize_rectangle(&tile.rectangle, &self.options.cartographic_limit_rectangle) else {
            return Ok(());
        };
        let surface = surface(tile);
        let real_mesh = surface.map(|s| s.mesh.borrow());
        let (mesh, vertex_array) = if let Some(fill) = fill {
            (&fill.terrain.mesh, fill.vertices.clone())
        } else if let Some((mesh, vertex_array)) =
            real_mesh.as_ref().and_then(|mesh| mesh.as_ref()).zip(surface.and_then(GlobeSurfaceTile::vertex_array::<G>))
        {
            (mesh, vertex_array)
        } else {
            return Ok(());
        };
        let translucent_rectangle = self.translucency_state.tile_rectangle(&tile.rectangle);
        let translucent = translucent_rectangle.is_some();
        let ground = self.options.show_ground_atmosphere && frame.mode == SceneMode::Scene3D;
        let fog = frame.fog.enabled
            && frame.fog.renderable
            && !frame.camera_underground
            && math::fog(tile.distance, frame.fog.density) > 1.0e-3;
        let underground = frame.mode == SceneMode::Scene3D
            && self.underground_visible(frame)
            && self.options.underground_color.is_some_and(|c| c.alpha > 0.0)
            && self
                .options
                .underground_color_alpha_by_distance
                .is_some_and(|s| s.near_value > 0.0 || s.far_value > 0.0);
        let water = self
            .terrain
            .has_water_mask()
            .then(|| surface.and_then(GlobeSurfaceTile::water_mask_texture::<G>))
            .flatten();
        let reflective = water.is_some() && self.options.show_water_effect;
        let waves = reflective && self.ocean_normal_map.is_some();

        let (min, max) = mesh_heights(mesh, frame);
        let sphere = tile_sphere(&tile.rectangle, mesh, frame);
        frame.minimum_terrain_height = frame.minimum_terrain_height.min(min);
        if let Some(data) = surface.and_then(|surface| surface.terrain_data.as_ref()) {
            for credit in data.credits().into_iter().flatten() {
                push_credit(frame, credit);
            }
        }

        let projection = frame.uniform.map_projection.as_ref();
        let web_mercator = frame.mode != SceneMode::Scene3D && !projection.is_geographic();
        let mut center = mesh.center;
        let mut projected = Cartesian4::ZERO;
        if frame.mode != SceneMode::Scene3D {
            let southwest = projection.project(&tile.rectangle.southwest());
            let northeast = projection.project(&tile.rectangle.northeast());
            projected = Cartesian4::new(southwest.x, southwest.y, northeast.x, northeast.y);
            if frame.mode != SceneMode::Morphing {
                center = Cartesian3::new(0.0, (projected.z + projected.x) * 0.5, (projected.w + projected.y) * 0.5);
                projected = Cartesian4::new(
                    projected.x - center.y,
                    projected.y - center.z,
                    projected.z - center.y,
                    projected.w - center.z,
                );
            }
            if frame.mode == SceneMode::Scene2D && mesh.encoding.quantization == TerrainQuantization::Bits12 {
                let x = (projected.z - projected.x) * 0.5 / 4095.0;
                let y = (projected.w - projected.y) * 0.5 / 4095.0;
                projected = Cartesian4::new(projected.x - x, projected.y - y, projected.z + x, projected.w + y);
            }
        }

        let mut common_textures: SampledTextures<G> = Vec::new();
        let mut clipping_matrix = None;
        let mut clip_color = Cartesian4::ZERO;
        let mut clipping_planes = None;
        if let Some((planes, texture)) = self
            .clipping_planes
            .as_ref()
            .filter(|p| p.enabled && !p.is_empty())
            .and_then(|p| p.texture::<G>().map(|t| (p, t)))
        {
            if planes.compute_intersection(&sphere, None)? != Intersect::Inside {
                clipping_matrix = Some(planes.model_matrix);
                clip_color = Cartesian4::new(
                    planes.edge_color.red,
                    planes.edge_color.green,
                    planes.edge_color.blue,
                    planes.edge_width,
                );
                clipping_planes = Some(GlobeClippingPlanesShader {
                    length: planes.len(),
                    union: planes.union_clipping_regions,
                    floating_point: texture.pixel_datatype() == PixelDatatype::Float,
                    texture_size: [texture.width(), texture.height()],
                });
                common_textures.push(("u_clippingPlanes".into(), texture));
            }
        }
        let mut clipping_polygons = None;
        if let Some((polygons, textures)) =
            self.clipping_polygons.as_ref().zip(self.polygon_textures.as_ref()).filter(|(polygons, _)| {
                polygons.compute_intersection_with_bounding_volume(Some(tile.rectangle)) != Intersect::Outside
            })
        {
            clipping_polygons = Some(GlobeClippingPolygonsShader {
                extents_count: polygons.extents_count(),
                inverse: polygons.inverse,
            });
            common_textures.push(("u_clippingExtents".into(), textures.extents.clone()));
            common_textures.push(("u_clippingDistance".into(), textures.distance.clone()));
        }
        if waves {
            common_textures.push(("u_oceanNormalMap".into(), self.ocean_normal_map.clone().unwrap()));
        }
        if let Some(water) = water.as_ref() {
            common_textures.push(("u_waterMask".into(), water.clone()));
        }
        let material = self.material.as_ref().filter(|m| {
            self.terrain.has_vertex_normals()
                || !(m.shader_source.contains("slope") || m.shader_source.contains("normalEC"))
        });
        if let Some(material) = material {
            common_textures.extend(material.resolve_sampled_textures(&self.context)?);
        }
        let material_uniforms = material.map(Material::uniform_values);
        let material_map =
            material_uniforms.map(|uniforms| Rc::new(move || Ok(uniforms.clone())) as crate::renderer::UniformMap);
        let map = tile_uniform_map(self.context.clone(), center, clipping_matrix, material_map);

        let mut features = GlobeShaderFeatures::default();
        for (feature, value) in [
            (Feature::WaterMask, water.is_some()),
            (Feature::ReflectiveOcean, reflective),
            (Feature::OceanWaves, waves),
            (Feature::Lighting, self.options.enable_lighting),
            (Feature::VertexNormals, mesh.encoding.has_vertex_normals),
            (Feature::DynamicLighting, self.options.dynamic_atmosphere_lighting),
            (Feature::SunLighting, self.options.dynamic_atmosphere_lighting_from_sun),
            (Feature::GroundAtmosphere, ground),
            (
                Feature::PerFragmentAtmosphere,
                ground && frame.uniform.camera.position.magnitude() > self.options.night_fade_distance[0],
            ),
            (Feature::WebMercator, web_mercator),
            (Feature::Fog, fog),
            (Feature::TileLimit, limit != full_tile()),
            (Feature::Quantization, mesh.encoding.quantization == TerrainQuantization::Bits12),
            (Feature::GeodeticNormals, mesh.encoding.has_geodetic_surface_normals),
            (Feature::Exaggeration, frame.vertical_exaggeration != 1.0),
            (
                Feature::ColorCorrect,
                (fog || ground) && self.options.atmosphere_hsb_shift.abs().maximum_component() > 1.0e-7,
            ),
            (Feature::HighlightFill, fill.is_some() && self.options.fill_highlight_color.is_some()),
            (Feature::Underground, underground),
            (Feature::Translucent, translucent),
        ] {
            features.set(feature, value);
        }

        let mut uniform_values =
            self.tile_uniforms(tile, surface, mesh, frame, limit, translucent_rectangle, projected, clip_color);
        let budget = self.texture_limit.saturating_sub(
            common_textures.len()
                + usize::from(frame.shadow_state.shadows_enabled)
                + if translucent { self.translucency_state.number_of_texture_uniforms } else { 0 },
        );
        let imagery = collect_imagery::<G>(tile, surface, frame);
        if !imagery.is_empty() && budget == 0 {
            return Err(RuntimeError::new("No texture units remain for globe imagery"));
        }
        let count = if self.options.show_skirts && !frame.camera_underground && !translucent {
            mesh.indices.len()
        } else {
            mesh.index_count_without_skirts.unwrap_or(mesh.indices.len())
        };
        let metadata = DrawMetadata {
            cull: false,
            occlude: false,
            bounding_volume: Some(sphere),
            oriented_bounding_box: (frame.mode == SceneMode::Scene3D)
                .then(|| {
                    crate::core::OrientedBoundingBox::from_rectangle(
                        &tile.rectangle,
                        min,
                        max,
                        tile.tiling_scheme.ellipsoid(),
                    )
                })
                .flatten(),
            owner: Some(format!("globe/{}/{}/{}", tile.level, tile.x, tile.y)),
            cast_shadows: self.options.shadows.cast_shadows() && !translucent,
            receive_shadows: self.options.shadows.receive_shadows() && !translucent,
            ..DrawMetadata::default()
        };
        let picking = frame.passes.pick || frame.passes.pick_voxel;
        if imagery.is_empty() {
            uniform_values.push(color_uniform("u_initialColor", self.options.base_color));
            self.emit_batch(
                frame,
                metadata,
                Batch {
                    vertices: vertex_array,
                    count,
                    features,
                    clipping_planes,
                    clipping_polygons,
                    uniform_values,
                    textures: common_textures,
                    map,
                    blend: false,
                    picking,
                    translucent,
                    texture_count: 0,
                    attributes: mesh.encoding.get_attribute_locations(),
                },
            )?;
        } else {
            for (i, imagery) in imagery.chunks(budget).enumerate() {
                if picking && i > 0 {
                    break;
                }
                let mut features = features;
                let mut uniforms = uniform_values.clone();
                let mut textures = common_textures.clone();
                let adjustments: Vec<_> = imagery.iter().map(|(adjustments, _)| *adjustments).collect();
                GlobeImageryUniforms::append_batch(&adjustments, &mut features, &mut uniforms);
                textures.extend(imagery.iter().enumerate().map(|(index, (_, texture))| {
                    (format!("u_dayTextures[{index}]"), texture.clone() as Rc<dyn SampledTexture<G>>)
                }));
                uniforms.push(color_uniform(
                    "u_initialColor",
                    if i == 0 { self.options.base_color } else { Color::TRANSPARENT },
                ));
                self.emit_batch(
                    frame,
                    metadata.clone(),
                    Batch {
                        vertices: vertex_array.clone(),
                        count,
                        features,
                        clipping_planes,
                        clipping_polygons,
                        uniform_values: uniforms,
                        textures,
                        map: map.clone(),
                        blend: i > 0,
                        picking,
                        translucent,
                        texture_count: imagery.len(),
                        attributes: mesh.encoding.get_attribute_locations(),
                    },
                )?;
            }
        }
        Ok(())
    }
}

struct Batch<G: Gl> {
    vertices: Rc<VertexArray<G>>,
    count: usize,
    features: GlobeShaderFeatures,
    clipping_planes: Option<GlobeClippingPlanesShader>,
    clipping_polygons: Option<GlobeClippingPolygonsShader>,
    uniform_values: Uniforms,
    textures: SampledTextures<G>,
    map: crate::renderer::UniformMap,
    blend: bool,
    picking: bool,
    translucent: bool,
    texture_count: usize,
    attributes: [(&'static str, u32); 3],
}

impl<G: Gl + 'static> GlobeSurfaceTileProvider<G> {
    fn emit_batch(
        &mut self,
        frame: &mut FrameState,
        metadata: DrawMetadata,
        batch: Batch<G>,
    ) -> Result<(), RuntimeError> {
        let options = GlobeSurfaceShaderOptions {
            mode: frame.mode,
            texture_count: batch.texture_count,
            features: batch.features,
            clipping_planes: batch.clipping_planes,
            clipping_polygons: batch.clipping_polygons,
            attributes: batch.attributes,
        };
        let program = self.shader_set.get_shader_program(&options)?;
        let mut state = RenderState::default();
        state.cull.enabled = self.options.back_face_culling && !frame.camera_underground && !batch.translucent;
        state.depth_test.enabled = true;
        state.depth_test.func = if batch.blend { gl::LEQUAL } else { gl::LESS };
        if batch.blend {
            state.blending = super::blending_state::alpha_blend();
        }
        let types = if batch.translucent {
            self.translucency_state.command_types(batch.blend, batch.picking)
        } else {
            Vec::new()
        };
        if types.is_empty() {
            let draw = batch.draw(program, state);
            frame.command_list.push(SceneCommand::from_draw(
                metadata.owner.clone().unwrap(),
                self.context.clone(),
                Pass::Globe,
                metadata,
                draw,
            ));
            return Ok(());
        }
        let classification = self.classification_texture.as_ref().or(self.scene_classification_texture.as_ref());
        for mut kind in types {
            if frame.globe_view_resources.is_none() {
                kind = match kind {
                    GlobeTranslucencyCommand::ManualDepthFront => GlobeTranslucencyCommand::TranslucentFront,
                    GlobeTranslucencyCommand::ManualDepthBack => GlobeTranslucencyCommand::TranslucentBack,
                    kind => kind,
                };
            }
            // Translucent globe faces pass in front of or behind classifications
            // using the same surface geometry and uniforms.
            if kind.is_pick() && classification.is_none() {
                continue;
            }
            let derived = self.shader_set.get_derived_program(&program, kind, classification.is_some())?;
            let mut state = kind.render_state(state.clone());
            if kind.is_translucent() || kind.is_pick() {
                state.depth_test.func = gl::LEQUAL;
            }
            let mut draw = batch.draw(derived, state);
            if let Some(texture) = classification.filter(|_| kind.is_translucent() || kind.is_pick()) {
                draw.textures.push(("u_classificationTexture".into(), texture.clone()));
            }
            let pass = if kind.is_translucent() || kind.is_pick() { Pass::Translucent } else { Pass::Globe };
            let mut command = SceneCommand::from_draw(
                format!("{}:{kind:?}", metadata.owner.as_deref().unwrap()),
                self.context.clone(),
                pass,
                metadata.clone(),
                draw,
            );
            command.globe_translucency = Some(kind);
            if kind.is_pick() {
                let execute = command.execute;
                command.execute = Rc::new(move |mut submission| {
                    submission.draw_mode.pick = false;
                    execute(submission)
                });
            }
            frame.command_list.push(command);
        }
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    fn tile_uniforms(
        &self,
        tile: &QuadtreeTile,
        surface: Option<&GlobeSurfaceTile>,
        mesh: &TerrainMesh,
        frame: &FrameState,
        limit: Cartesian4,
        translucency: Option<Cartesian4>,
        projected: Cartesian4,
        clip: Cartesian4,
    ) -> Uniforms {
        let o = &self.options;
        let mercator = WebMercatorProjection::geodetic_latitude_to_mercator_angle;
        let south_mercator = mercator(tile.rectangle.south);
        let mercator_height = mercator(tile.rectangle.north) - south_mercator;
        let ocean_specular = if frame.mode != SceneMode::Scene3D {
            0.0
        } else if o.show_ground_atmosphere {
            0.4
        } else {
            0.5
        };
        vec![
            float_uniform("u_zoomedOutOceanSpecularIntensity", &[ocean_specular]),
            float_uniform("u_atmosphereLightIntensity", &[o.atmosphere_light_intensity]),
            vec3_uniform("u_atmosphereRayleighCoefficient", o.atmosphere_rayleigh_coefficient),
            vec3_uniform("u_atmosphereMieCoefficient", o.atmosphere_mie_coefficient),
            float_uniform("u_atmosphereRayleighScaleHeight", &[o.atmosphere_rayleigh_scale_height]),
            float_uniform("u_atmosphereMieScaleHeight", &[o.atmosphere_mie_scale_height]),
            float_uniform("u_atmosphereMieAnisotropy", &[o.atmosphere_mie_anisotropy]),
            float_uniform("u_lightingFadeDistance", &o.lighting_fade_distance),
            float_uniform("u_nightFadeDistance", &o.night_fade_distance),
            vec3_uniform("u_center3D", mesh.center),
            float_uniform(
                "u_verticalExaggerationAndRelativeHeight",
                &[frame.vertical_exaggeration, frame.vertical_exaggeration_relative_height],
            ),
            vec4_uniform("u_tileRectangle", projected),
            float_uniform("u_southAndNorthLatitude", &[tile.rectangle.south, tile.rectangle.north]),
            float_uniform(
                "u_southMercatorYAndOneOverHeight",
                &[south_mercator, if mercator_height == 0.0 { 0.0 } else { 1.0 / mercator_height }],
            ),
            vec4_uniform(
                "u_waterMaskTranslationAndScale",
                surface.map_or(full_tile(), |s| s.water_mask_translation_and_scale),
            ),
            float_uniform(
                "u_minMaxHeight",
                &[
                    mesh.encoding.minimum_height.unwrap_or(mesh.minimum_height),
                    mesh.encoding.maximum_height.unwrap_or(mesh.maximum_height),
                ],
            ),
            matrix_uniform("u_scaleAndBias", mesh.encoding.matrix.unwrap_or(crate::core::Matrix4::IDENTITY)),
            vec4_uniform("u_cartographicLimitRectangle", limit),
            vec4_uniform("u_clippingPlanesEdgeStyle", clip),
            float_uniform("u_minimumBrightness", &[frame.fog.minimum_brightness]),
            vec3_uniform("u_hsbShift", o.atmosphere_hsb_shift),
            color_uniform("u_fillHighlightColor", o.fill_highlight_color.unwrap_or(Color::TRANSPARENT)),
            scalar_uniform("u_frontFaceAlphaByDistance", self.translucency_state.front_face_alpha_by_distance),
            scalar_uniform("u_backFaceAlphaByDistance", self.translucency_state.back_face_alpha_by_distance),
            vec4_uniform("u_translucencyRectangle", translucency.unwrap_or_else(full_tile)),
            color_uniform("u_undergroundColor", o.underground_color.unwrap_or(Color::TRANSPARENT)),
            scalar_uniform(
                "u_undergroundColorAlphaByDistance",
                o.underground_color_alpha_by_distance.unwrap_or_default(),
            ),
            float_uniform("u_lambertDiffuseMultiplier", &[o.lambert_diffuse_multiplier]),
            float_uniform("u_vertexShadowDarkness", &[o.vertex_shadow_darkness]),
        ]
    }
}

impl<G: Gl + 'static> Batch<G> {
    fn draw(&self, program: Rc<crate::renderer::ShaderProgram<G>>, state: RenderState) -> SceneDrawOptions<G> {
        let mut draw = SceneDrawOptions::new(self.vertices.clone(), program);
        draw.state = state;
        draw.count = Some(self.count);
        draw.uniforms = self.uniform_values.clone();
        draw.uniform_map = Some(self.map.clone());
        draw.textures = self.textures.clone();
        draw
    }
}

fn collect_imagery<G: Gl + 'static>(
    tile: &QuadtreeTile,
    surface: Option<&GlobeSurfaceTile>,
    frame: &mut FrameState,
) -> Vec<(GlobeImageryUniforms, Rc<Texture<G>>)> {
    surface
        .into_iter()
        .flat_map(|s| &s.imagery)
        .filter_map(|association| {
            let layer = association.imagery_layer.as_ref()?.borrow();
            if !layer.show || layer.alpha == 0.0 {
                return None;
            }
            let imagery = association.ready_imagery.as_ref()?.borrow();
            let resource =
                if association.use_web_mercator_t { &imagery.texture_web_mercator } else { &imagery.texture };
            let texture = resource.as_ref()?.get::<Rc<Texture<G>>>()?.clone();
            for credit in imagery.credits.iter().flatten() {
                push_credit(frame, credit);
            }
            let translation = association
                .texture_translation_and_scale
                .or_else(|| layer.calculate_texture_translation_and_scale_for_tile(&tile.rectangle, association))?;
            let adjustments = GlobeImageryUniforms {
                translation,
                rectangle: association.texture_coordinate_rectangle,
                mercator: association.use_web_mercator_t,
                alpha: layer.alpha,
                night_alpha: layer.night_alpha,
                day_alpha: layer.day_alpha,
                brightness: layer.brightness,
                contrast: layer.contrast,
                hue: layer.hue,
                saturation: layer.saturation,
                one_over_gamma: 1.0 / layer.gamma,
                split: layer.split_direction as i32 as f64,
                cutout: layer.cutout_rectangle.as_ref().and_then(|r| localize_rectangle(&tile.rectangle, r)),
                color_to_alpha: layer
                    .color_to_alpha
                    .filter(|_| layer.color_to_alpha_threshold > 0.0)
                    .map(|color| Cartesian4::new(color.red, color.green, color.blue, layer.color_to_alpha_threshold)),
            };
            Some((adjustments, texture))
        })
        .collect()
}

fn full_tile() -> Cartesian4 {
    Cartesian4::new(0.0, 0.0, 1.0, 1.0)
}
fn vec3_uniform(name: &str, v: Cartesian3) -> (String, UniformValue) {
    float_uniform(name, &[v.x, v.y, v.z])
}
fn vec4_uniform(name: &str, v: Cartesian4) -> (String, UniformValue) {
    float_uniform(name, &[v.x, v.y, v.z, v.w])
}
fn color_uniform(name: &str, v: Color) -> (String, UniformValue) {
    float_uniform(name, &[v.red, v.green, v.blue, v.alpha])
}
fn scalar_uniform(name: &str, v: NearFarScalar) -> (String, UniformValue) {
    float_uniform(name, &[v.near, v.near_value, v.far, v.far_value])
}
