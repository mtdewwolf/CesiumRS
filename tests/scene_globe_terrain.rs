use cesium_rs::{
    core::{webgl_constants as gl, *},
    renderer::{Context, ContextCapabilities, Gl, GlValue, Pass, UniformFrame},
    scene::{
        globe_surface_tile::TerrainDataFuture, quadtree_tile::QuadtreeTile,
        quadtree_tile_provider::QuadtreeTileProvider, *,
    },
};
use std::{
    cell::{Cell, RefCell},
    future::poll_fn,
    rc::Rc,
    sync::{Arc, Mutex},
    task::Poll,
};

#[derive(Default)]
struct RecordingGl {
    next: Cell<u32>,
    calls: RefCell<Vec<(String, Vec<GlValue<u32>>)>>,
    texture_units: u32,
}

impl RecordingGl {
    fn count(&self, name: &str) -> usize {
        self.calls.borrow().iter().filter(|(method, _)| method == name).count()
    }
}

impl Gl for RecordingGl {
    type Object = u32;

    fn drawing_buffer_size(&self) -> Result<[u32; 2], RuntimeError> {
        Ok([64, 64])
    }

    fn invoke(&self, name: &str, args: &[GlValue<u32>]) -> Result<GlValue<u32>, RuntimeError> {
        self.calls.borrow_mut().push((name.into(), args.to_vec()));
        if name.starts_with("create") {
            self.next.set(self.next.get() + 1);
            return Ok(GlValue::Object(self.next.get()));
        }
        Ok(match (name, args) {
            ("getParameter", [GlValue::Number(p)]) => match *p as u32 {
                gl::MAX_VIEWPORT_DIMS | gl::ALIASED_LINE_WIDTH_RANGE | gl::ALIASED_POINT_SIZE_RANGE => {
                    GlValue::Numbers(vec![1.0, 64.0])
                }
                gl::MAX_TEXTURE_IMAGE_UNITS => GlValue::Number(self.texture_units.max(2) as f64),
                gl::MAX_TEXTURE_SIZE => GlValue::Number(64.0),
                gl::MAX_SAMPLES => GlValue::Number(1.0),
                gl::STENCIL_BITS => GlValue::Number(8.0),
                _ => GlValue::Number(16.0),
            },
            ("getProgramParameter", [_, GlValue::Number(p)]) if *p as u32 == gl::LINK_STATUS => GlValue::Bool(true),
            ("getShaderParameter", _) => GlValue::Bool(true),
            ("checkFramebufferStatus", _) => GlValue::Number(gl::FRAMEBUFFER_COMPLETE as f64),
            ("getUniformLocation", _) => GlValue::Object(1),
            _ => GlValue::Null,
        })
    }
}

fn epoch() -> JulianDate {
    JulianDate::new(2_451_545.0, 0.0, TimeStandard::Tai)
}

fn render_frame() -> FrameState {
    let mut camera = Camera::new(CameraOptions::new(64.0, 64.0));
    camera.position = Cartesian3::new(0.0, -25_000_000.0, 0.0);
    camera.direction = Cartesian3::UNIT_Y;
    camera.up = Cartesian3::UNIT_Z;
    camera.right = Cartesian3::UNIT_X;
    let mut frame =
        FrameState::new(UniformFrame::new(camera.uniform_camera(), epoch()), CullingVolume::new(vec![]), [64, 64]);
    frame.passes.render = true;
    frame.context_capabilities = ContextCapabilities { webgl2: true, depth_texture: true, ..Default::default() };
    frame
}

fn render_globe(globe: &mut Globe<RecordingGl>, frame: &mut FrameState) {
    frame.frame_number += 1;
    frame.command_list.clear();
    globe.update(frame).unwrap();
    globe.begin_frame(frame).unwrap();
    globe.render(frame).unwrap();
    globe.end_frame(frame).unwrap();
}

fn surface(tile: &QuadtreeTile) -> &GlobeSurfaceTile {
    tile.data.as_ref().unwrap().as_any().downcast_ref().unwrap()
}

#[test]
fn globe_drives_load_selection_height_pick_and_destroy() {
    let context = Context::new_webgl2(RecordingGl::default());
    let mut globe = Globe::new(&context, GlobeOptions::default());
    let mut frame = render_frame();
    assert!(globe.tiles_loaded());
    assert_eq!(globe.ocean_normal_map_url(), Some(DEFAULT_OCEAN_NORMAL_MAP_URL));
    assert_eq!(globe.surface_options().base_color, Color::new(0.0, 0.0, 0.5, 1.0));
    assert!(globe.surface_options().show_ground_atmosphere);

    let progress = Arc::new(Mutex::new(Vec::new()));
    let values = progress.clone();
    globe.tile_load_progress_event().add_event_listener(move |n| values.lock().unwrap().push(*n));
    assert_eq!(globe.get_height(&Cartographic::ZERO), None);
    for _ in 0..3 {
        render_globe(&mut globe, &mut frame);
    }
    assert!(globe.tiles_loaded());
    let rendered = globe.rendered_surface_tiles();
    assert!(!rendered.is_empty());
    let root = rendered[0].clone();
    let vertex = {
        let tile = root.borrow();
        let data = surface(&tile);
        assert_eq!(data.terrain_state, TerrainState::Ready);
        let mesh = data.mesh.borrow();
        let mesh = mesh.as_ref().unwrap();
        mesh.encoding.decode_position(&mesh.vertices, 7 * 16 + 7).unwrap()
    };
    let query = globe.ellipsoid().cartesian_to_cartographic(vertex).unwrap();
    assert!((globe.get_height(&query).unwrap() - query.height).abs() < 1.0);
    let ray = Ray::new(vertex.multiply_by_scalar(2.0), vertex.negate());
    assert!(globe.pick(&ray, &frame).unwrap().distance(vertex) < 1.0);
    assert!(frame.command_list.iter().any(|command| command.pass == Pass::Globe && command.metadata.receive_shadows));
    assert!(progress.lock().unwrap().iter().any(|n| *n > 0));

    globe.show = false;
    render_globe(&mut globe, &mut frame);
    assert!(frame.command_list.is_empty());
    globe.destroy().unwrap();
    assert!(globe.is_destroyed());
    assert_eq!(root.borrow().state, QuadtreeTileLoadState::Start);
    assert_eq!(globe.get_height(&query), None);
    globe.destroy().unwrap();
}

#[test]
fn shader_cache_distinguishes_flags_counts_clipping_and_material() {
    let context = Context::new_webgl2(RecordingGl::default());
    let mut set = GlobeSurfaceShaderSet::new(&context);
    let mut features = GlobeShaderFeatures::default();
    for feature in [
        GlobeShaderFeature::Alpha,
        GlobeShaderFeature::DayNightAlpha,
        GlobeShaderFeature::Cutout,
        GlobeShaderFeature::Quantization,
        GlobeShaderFeature::WebMercator,
        GlobeShaderFeature::Translucent,
    ] {
        features.set(feature, true);
    }
    assert_ne!(features.key(SceneMode::ColumbusView) & (1 << 32), 0);
    let mut options = GlobeSurfaceShaderOptions {
        mode: SceneMode::ColumbusView,
        texture_count: 2,
        features,
        clipping_planes: None,
        clipping_polygons: None,
        attributes: [("compressed0", 0), ("compressed1", 1), ("geodeticSurfaceNormal", 2)],
    };
    let first = set.get_shader_program(&options).unwrap();
    assert!(Rc::ptr_eq(&first, &set.get_shader_program(&options).unwrap()));
    assert!(first.fragment_shader_source().contains("#define APPLY_DAY_NIGHT_ALPHA"));
    assert!(first.fragment_shader_source().contains("u_dayTextures[1]"));
    assert!(first.vertex_shader_source().contains("return getPositionColumbusViewMode"));
    assert!(first.vertex_shader_source().contains("return get2DMercatorYPositionFraction"));
    assert!(first.vertex_shader_source().contains("#define QUANTIZATION_BITS12"));

    options.clipping_planes =
        Some(GlobeClippingPlanesShader { length: 2, union: true, floating_point: true, texture_size: [2, 2] });
    let clipped = set.get_shader_program(&options).unwrap();
    assert!(!Rc::ptr_eq(&first, &clipped));
    assert!(clipped.fragment_shader_source().contains("#define ENABLE_CLIPPING_PLANES"));
    let derived = set.get_derived_program(&clipped, GlobeTranslucencyCommand::ManualDepthBack, true).unwrap();
    assert!(derived.fragment_shader_source().contains("czm_globe_translucency_main()"));
    assert!(derived.fragment_shader_source().contains("czm_globeDepthTexture"));
    assert!(Rc::ptr_eq(
        &derived,
        &set.get_derived_program(&clipped, GlobeTranslucencyCommand::ManualDepthBack, true).unwrap()
    ));

    set.set_material(Some(
        "czm_material czm_getMaterial(czm_materialInput i) { return czm_getDefaultMaterial(i); }".into(),
    ))
    .unwrap();
    let material = set.get_shader_program(&options).unwrap();
    assert!(material.fragment_shader_source().contains("#define APPLY_MATERIAL"));
    assert_eq!(set.number_of_shaders(), 1);
    set.destroy().unwrap();
    assert!(first.is_destroyed());
    assert!(set.get_shader_program(&options).is_err());
}

#[test]
fn translucency_scales_faces_localizes_dateline_and_selects_draw_order() {
    use GlobeTranslucencyCommand::*;
    let mut config = GlobeTranslucency::new();
    config.enabled = true;
    config.set_front_face_alpha(0.5).unwrap();
    config.set_front_face_alpha_by_distance(Some(NearFarScalar::new(10.0, 0.2, 200.0, 0.8))).unwrap();
    config.set_rectangle(Some(Rectangle::from_degrees(175.0, -30.0, -175.0, 30.0)));
    let mut state = GlobeTranslucencyState::default();
    let mut frame = GlobeTranslucencyFrame {
        show: true,
        base_alpha: 1.0,
        camera_underground: false,
        depth_test_against_terrain: false,
        depth_texture: true,
        mode: SceneMode::Scene3D,
    };
    state.update(&config, frame);
    assert_eq!(state.front_face_alpha_by_distance, NearFarScalar::new(10.0, 0.1, 200.0, 0.4));
    assert_eq!(state.number_of_texture_uniforms, 2);
    assert!(state.environment_visible && !state.sun_visible_through_globe && !state.use_depth_plane);
    assert_eq!(state.command_types(false, false), vec![DepthFront, OpaqueBack, ManualDepthFront]);
    let eastern = state.tile_rectangle(&Rectangle::from_degrees(170.0, -30.0, 180.0, 30.0)).unwrap();
    let western = state.tile_rectangle(&Rectangle::from_degrees(-180.0, -30.0, -170.0, 30.0)).unwrap();
    assert!((eastern.x - 0.5).abs() < 1.0e-8 && eastern.z == 1.0);
    assert!(western.x == 0.0 && (western.z - 0.5).abs() < 1.0e-8);
    assert!(state.tile_rectangle(&Rectangle::from_degrees(-10.0, -10.0, 10.0, 10.0)).is_none());

    config.set_back_face_alpha(0.5).unwrap();
    state.update(&config, frame);
    assert!(state.sun_visible_through_globe);
    assert_eq!(state.command_types(false, false), vec![DepthBoth, TranslucentBack, TranslucentFront]);
    frame.camera_underground = true;
    state.update(&config, frame);
    assert_eq!(state.command_types(false, true), vec![DepthBoth, PickFront, PickBack]);
    assert_eq!(state.command_types(true, false), vec![TranslucentFront, TranslucentBack]);
    frame.mode = SceneMode::Scene2D;
    state.update(&config, frame);
    assert_eq!(state.command_types(false, false), vec![DepthFront, TranslucentFront]);
    config.enabled = false;
    state.update(&config, frame);
    assert!(state.command_types(false, false).is_empty());
    assert!(!state.environment_visible);
}

#[test]
fn fill_copies_neighbor_vertices_and_interpolates_coarser_edge_endpoints() {
    let ellipsoid = Ellipsoid::WGS84;
    let left = Rectangle::new(-0.2, -0.2, 0.0, 0.2);
    let source = TerrainFillMesh::new(
        left,
        &ellipsoid,
        TerrainFillOptions { minimum_height: 150.0, maximum_height: 150.0, geometric_error: 1.0, ..Default::default() },
        &[],
    );
    assert_eq!(source.mesh.vertex_count_without_skirts, 81);
    let rectangle = Rectangle::new(0.0, -0.12, 0.1, 0.12);
    let fill = TerrainFillMesh::new(
        rectangle,
        &ellipsoid,
        TerrainFillOptions { geometric_error: 1.0, exaggeration: 2.0, ..Default::default() },
        &[&source.mesh],
    );
    let edge = &fill.mesh.edges.west_indices_south_to_north;
    assert_eq!(edge.len(), 7);
    assert!(fill.mesh.encoding.has_geodetic_surface_normals);
    assert_eq!(fill.mesh.index_count_without_skirts, Some(fill.mesh.indices.len()));
    for &id in edge {
        let height = fill.mesh.encoding.decode_height(&fill.mesh.vertices, id as usize).unwrap();
        assert_eq!(height, 150.0);
    }
    let p = |mesh: &TerrainMesh, id: u32| mesh.encoding.decode_position(&mesh.vertices, id as usize).unwrap();
    let coarse = &source.mesh.edges.east_indices_north_to_south;
    // The southern endpoint is between the source samples at -0.15 and -0.10.
    let expected =
        p(&source.mesh, coarse[7]).multiply_by_scalar(0.4).add(p(&source.mesh, coarse[6]).multiply_by_scalar(0.6));
    assert!(p(&fill.mesh, edge[0]).distance(expected) < 0.2);
    assert!((fill.height_at(&Cartographic::new(0.03, 0.0, 0.0)).unwrap() - 150.0).abs() < 1.0e-8);
    assert_eq!(fill.height_at(&Cartographic::new(-0.1, 0.0, 0.0)), None);
}

struct TerrainSource {
    scheme: Arc<dyn TilingScheme>,
    requests: Rc<Cell<usize>>,
    height: u8,
}

impl GlobeTerrainSource for TerrainSource {
    fn tiling_scheme(&self) -> Arc<dyn TilingScheme> {
        self.scheme.clone()
    }
    fn geometric_error(&self, level: u32) -> f64 {
        10.0 * 2.0_f64.powf(-(level as f64))
    }
    fn tile_available(&self, tile: TerrainTile) -> Option<bool> {
        Some(tile.level == 0)
    }
    fn has_water_mask(&self) -> bool {
        true
    }
    fn request_geometry(&self, _: TerrainTile) -> Option<TerrainDataFuture> {
        self.requests.set(self.requests.get() + 1);
        let height = self.height;
        let mut first_poll = true;
        Some(Box::pin(poll_fn(move |_| {
            if std::mem::take(&mut first_poll) {
                return Poll::Pending;
            }
            let mut options = HeightmapTerrainDataOptions::new(TypedArray::Uint8(vec![height; 16]), 4, 4);
            options.water_mask = Some(vec![255]);
            Poll::Ready(Ok(Box::new(HeightmapTerrainData::new(options).unwrap()) as Box<dyn SurfaceTerrainData>))
        })))
    }
}

#[test]
fn provider_polls_requests_upsamples_unavailable_children_and_inherits_water() {
    let context = Context::new_webgl2(RecordingGl::default());
    let mut frame = render_frame();
    let requests = Rc::new(Cell::new(0));
    let terrain = Rc::new(TerrainSource {
        scheme: Arc::new(GeographicTilingScheme::default()),
        requests: requests.clone(),
        height: 19,
    });
    let mut globe = Globe::with_terrain_provider(&context, GlobeOptions::default(), terrain.clone());
    let root = QuadtreeTile::create_level_zero_tiles(terrain.tiling_scheme()).remove(0);
    let provider = &mut globe.surface.tile_provider;
    provider.load_tile(&frame, &root);
    assert_eq!(surface(&root.borrow()).terrain_state, TerrainState::Receiving);
    assert!(!root.borrow().eligible_for_unloading());
    provider.load_tile(&frame, &root);
    assert_eq!(root.borrow().state, QuadtreeTileLoadState::Done);
    assert!(surface(&root.borrow()).water_mask_texture::<RecordingGl>().is_some());
    let child = QuadtreeTile::northwest_child(&root);
    provider.load_tile(&frame, &child);
    assert!(child.borrow().upsampled_from_parent);
    assert!(child.borrow().renderable);
    let child_tile = child.borrow();
    let parent_mask = surface(&root.borrow()).water_mask_texture::<RecordingGl>().unwrap();
    let child_mask = surface(&child_tile).water_mask_texture::<RecordingGl>().unwrap();
    assert!(Rc::ptr_eq(&parent_mask, &child_mask));
    assert_eq!(surface(&child_tile).water_mask_translation_and_scale, Cartesian4::new(0.0, 0.5, 0.5, 0.5));
    drop(child_tile);
    assert_eq!(requests.get(), 1);

    provider.options.cartographic_limit_rectangle = Rectangle::from_degrees(40.0, -20.0, 70.0, 20.0);
    assert_eq!(provider.compute_tile_visibility(&root.borrow(), &frame, None), Visibility::None);
    let events = Arc::new(Mutex::new(0));
    let count = events.clone();
    globe.terrain_provider_changed.add_event_listener(move |_| *count.lock().unwrap() += 1);
    let replacement = Rc::new(GlobeTerrainProviderAdapter::new(EllipsoidTerrainProvider::default(), None));
    globe.set_terrain_provider(replacement.clone());
    globe.set_terrain_provider(replacement);
    assert_eq!(*events.lock().unwrap(), 1);
    assert!(!globe.tiles_loaded());
    render_globe(&mut globe, &mut frame);
    assert!(!globe.surface.tiles_invalidated);
}

struct ImagerySource {
    scheme: GeographicTilingScheme,
    errors: ImageryErrorEvent,
}

impl ImageryProvider for ImagerySource {
    fn rectangle(&self) -> &Rectangle {
        self.scheme.rectangle()
    }
    fn tile_width(&self) -> f64 {
        16.0
    }
    fn tile_height(&self) -> f64 {
        16.0
    }
    fn maximum_level(&self) -> Option<u32> {
        Some(0)
    }
    fn tiling_scheme(&self) -> &dyn TilingScheme {
        &self.scheme
    }
    fn error_event(&self) -> &ImageryErrorEvent {
        &self.errors
    }
    fn request_image(&self, _: i32, _: i32, _: u32) -> Option<ImageryFuture> {
        Some(Box::pin(async {
            Ok(ImageryImage::Image(FrameResource::new(ImageryPixels { width: 2, height: 2, bytes: vec![128; 16] })))
        }))
    }
}

#[test]
fn provider_batches_imagery_and_binds_layer_uniforms_on_real_commands() {
    // Three texture units, one of which the shadow map reserves, so three
    // imagery layers batch into commands with two and one textures.
    let context = Context::new_webgl2(RecordingGl { texture_units: 3, ..Default::default() });
    let mut frame = render_frame();
    let mut globe = Globe::new(&context, GlobeOptions::default());
    let events = Arc::new(Mutex::new(0));
    let count = events.clone();
    globe.imagery_layers_updated_event().add_event_listener(move |_| *count.lock().unwrap() += 1);
    for i in 0..3 {
        #[allow(clippy::arc_with_non_send_sync)]
        let provider =
            Arc::new(ImagerySource { scheme: GeographicTilingScheme::default(), errors: Default::default() });
        let layer = ImageryLayer::new(
            Some(provider),
            ImageryLayerOptions { brightness: Some(1.0 + i as f64 * 0.2), day_alpha: Some(0.8), ..Default::default() },
        )
        .into_handle();
        globe.imagery_layers_mut().add(layer, None).unwrap();
    }
    for _ in 0..3 {
        render_globe(&mut globe, &mut frame);
    }
    let renderable_tiles = globe.rendered_surface_tiles().len();
    assert!(renderable_tiles > 0);
    assert_eq!(frame.command_list.iter().filter(|command| command.pass == Pass::Globe).count(), renderable_tiles * 2);
    assert_eq!(*events.lock().unwrap(), 1);

    let mut backend = RendererSceneBackend::new(context.clone());
    backend.begin_frame(&frame).unwrap();
    for command in &frame.command_list {
        (command.execute)(SceneSubmission {
            pass: command.pass,
            frustum: 0,
            near: 1.0,
            far: 100_000_000.0,
            draw_mode: SceneDrawMode::default(),
            oit: None,
            metadata_pick: None,
            framebuffer: None,
            viewport: None,
            scissor_test: None,
            blending_enabled: None,
            metadata: command.metadata.clone(),
            clipping_planes: None,
            tileset_draw: None,
        })
        .unwrap();
    }
    let host = context.host();
    assert_eq!(host.count("drawElements"), renderable_tiles * 2);
    let calls = host.calls.borrow();
    let sources: Vec<_> = calls
        .iter()
        .filter_map(|(name, args)| match (name.as_str(), args.as_slice()) {
            ("shaderSource", [_, GlValue::String(source)]) => Some(source.as_str()),
            _ => None,
        })
        .collect();
    assert!(sources
        .iter()
        .any(|source| source.contains("#define TEXTURE_UNITS 2") && source.contains("#define APPLY_DAY_NIGHT_ALPHA")));
    assert!(sources
        .iter()
        .any(|source| source.contains("#define TEXTURE_UNITS 1") && source.contains("#define APPLY_BRIGHTNESS")));
    assert!(calls.iter().any(|(name, args)| {
        name == "getUniformLocation"
            && matches!(args.last(), Some(GlValue::String(name)) if name == "u_dayTextureTranslationAndScale")
    }));
}

struct ClassificationRecorder(Rc<RefCell<Vec<bool>>>);

impl ScenePrimitive for ClassificationRecorder {
    fn pre_passes_update(&mut self, _: &mut FrameState) -> Result<(), RuntimeError> {
        Ok(())
    }
    fn post_passes_update(&mut self, _: &mut FrameState) -> Result<(), RuntimeError> {
        Ok(())
    }
    fn update_for_pass(&mut self, _: &mut FrameState, _: Cesium3DTilePass) -> Result<(), RuntimeError> {
        Ok(())
    }

    fn update(&mut self, frame: &mut FrameState) -> Result<(), RuntimeError> {
        let calls = self.0.clone();
        let mut command = SceneCommand::new(
            "terrain-classifier",
            Pass::TerrainClassification,
            Rc::new(move |submission| {
                calls.borrow_mut().push(submission.framebuffer.is_some());
                Ok(())
            }),
        );
        command.metadata.bounding_volume = Some(BoundingSphere::new(Cartesian3::ZERO, Ellipsoid::WGS84.maximum_radius));
        command.metadata.cull = false;
        frame.command_list.push(command);
        Ok(())
    }

    fn destroy(&mut self) -> Result<(), RuntimeError> {
        Ok(())
    }
}

#[test]
fn scene_routes_translucent_globe_depth_and_terrain_classification() {
    let context = Context::new_webgl2(RecordingGl { texture_units: 8, ..Default::default() });
    let mut scene = Scene::new(
        RendererSceneBackend::new(context.clone()),
        CameraOptions::new(64.0, 64.0),
        SceneOptions { order_independent_translucency: false, logarithmic_depth_buffer: false, ..Default::default() },
        epoch(),
    )
    .unwrap();
    scene.rethrow_render_errors = true;
    scene.fog.enabled = false;
    scene.camera.position = Cartesian3::new(0.0, -25_000_000.0, 0.0);
    scene.camera.direction = Cartesian3::UNIT_Y;
    scene.camera.up = Cartesian3::UNIT_Z;
    scene.camera.right = Cartesian3::UNIT_X;

    let mut globe = Globe::new(&context, GlobeOptions::default());
    globe.translucency.enabled = true;
    globe.translucency.set_front_face_alpha(0.4).unwrap();
    globe.translucency.set_back_face_alpha(0.6).unwrap();
    scene.globe = Some(Box::new(globe));
    let classification = Rc::new(RefCell::new(Vec::new()));
    scene.primitives.push(Box::new(ClassificationRecorder(classification.clone())));
    for _ in 0..3 {
        scene.render(epoch()).unwrap();
    }

    let commands = &scene.frame_state.command_list;
    assert!(commands.iter().any(|command| command.globe_translucency == Some(GlobeTranslucencyCommand::DepthBoth)));
    assert!(commands
        .iter()
        .any(|command| command.globe_translucency == Some(GlobeTranslucencyCommand::TranslucentFront)));
    assert!(commands
        .iter()
        .any(|command| command.globe_translucency == Some(GlobeTranslucencyCommand::TranslucentBack)));
    assert!(!classification.borrow().is_empty());
    assert!(classification.borrow().iter().all(|has_framebuffer| *has_framebuffer));
    assert!(context.host().count("drawElements") > 0);
    scene.destroy().unwrap();
}

struct TilesetCommandStub;

impl ScenePrimitive for TilesetCommandStub {
    fn pre_passes_update(&mut self, _: &mut FrameState) -> Result<(), RuntimeError> {
        Ok(())
    }
    fn post_passes_update(&mut self, _: &mut FrameState) -> Result<(), RuntimeError> {
        Ok(())
    }
    fn update_for_pass(&mut self, _: &mut FrameState, _: Cesium3DTilePass) -> Result<(), RuntimeError> {
        Ok(())
    }
    fn update(&mut self, frame: &mut FrameState) -> Result<(), RuntimeError> {
        let mut command = SceneCommand::new("tileset-stub", Pass::Cesium3DTile, Rc::new(|_| Ok(())));
        command.metadata.bounding_volume = Some(BoundingSphere::new(Cartesian3::ZERO, Ellipsoid::WGS84.maximum_radius));
        command.metadata.cull = false;
        frame.command_list.push(command);
        Ok(())
    }
    fn destroy(&mut self) -> Result<(), RuntimeError> {
        Ok(())
    }
}

#[test]
fn scene_propagates_clear_globe_depth_to_tileset_merge_and_viewport_origin_in_translucency() {
    let context = Context::new_webgl2(RecordingGl { texture_units: 8, ..Default::default() });
    let mut scene = Scene::new(
        RendererSceneBackend::new(context.clone()),
        CameraOptions::new(64.0, 64.0),
        SceneOptions { order_independent_translucency: false, logarithmic_depth_buffer: false, ..Default::default() },
        epoch(),
    )
    .unwrap();
    scene.rethrow_render_errors = true;
    scene.fog.enabled = false;
    scene.camera.position = Cartesian3::new(0.0, -25_000_000.0, 0.0);
    scene.camera.direction = Cartesian3::UNIT_Y;
    scene.camera.up = Cartesian3::UNIT_Z;
    scene.camera.right = Cartesian3::UNIT_X;

    let globe = Globe::new(&context, GlobeOptions::default());
    assert!(!globe.depth_test_against_terrain);
    scene.globe = Some(Box::new(globe));
    scene.primitives.push(Box::new(TilesetCommandStub));
    for _ in 0..3 {
        scene.render(epoch()).unwrap();
    }

    // With depth_test_against_terrain = false and a Cesium3DTile pass, GlobeDepth must
    // take the stencil-masked merge path (stencilFunc with EQUAL, 0x80, 0x80) instead of
    // overwriting packed terrain depth via the unmasked full-copy shortcut.
    let calls = context.host().calls.borrow();
    assert!(calls.iter().any(|(name, args)| {
        name == "stencilFunc"
            && matches!(
                args.as_slice(),
                [GlValue::Number(func), GlValue::Number(ref_val), GlValue::Number(mask)]
                    if *func as u32 == gl::EQUAL && *ref_val as u32 == 0x80 && *mask as u32 == 0x80
            )
    }));
    drop(calls);

    let derived = GlobeTranslucencyCommand::TranslucentFront.shader(
        cesium_rs::renderer::ShaderProgramOptions {
            vertex_shader_source: cesium_rs::renderer::ShaderSource::new("void main() {}"),
            fragment_shader_source: cesium_rs::renderer::ShaderSource::new(
                "void main() { out_FragColor = vec4(1.0); }",
            ),
            attribute_locations: Default::default(),
        },
        true,
    );
    let fs = derived.fragment_shader_source.sources.join("\n");
    assert!(fs.contains("(gl_FragCoord.xy - czm_viewport.xy) / czm_viewport.zw"));
    scene.destroy().unwrap();
}

fn terrain_reference() -> serde_json::Value {
    serde_json::from_str(include_str!("data/scene-globe-terrain.json")).unwrap()
}

fn reference_tuple<const N: usize>(value: &serde_json::Value) -> [f64; N] {
    assert_eq!(value.as_array().unwrap().len(), N);
    std::array::from_fn(|i| value[i].as_f64().unwrap())
}

fn reference_mode(value: &serde_json::Value) -> SceneMode {
    match value.as_str().unwrap() {
        "SCENE3D" => SceneMode::Scene3D,
        "SCENE2D" => SceneMode::Scene2D,
        "COLUMBUS_VIEW" => SceneMode::ColumbusView,
        "MORPHING" => SceneMode::Morphing,
        mode => panic!("Unexpected Scene mode {mode}"),
    }
}

#[test]
fn globe_selection_and_world_picking_match_cesium() {
    let fixture = terrain_reference();
    let expected = &fixture["globe"];
    let context = Context::new_webgl2(RecordingGl::default());
    let mut globe = Globe::new(&context, GlobeOptions::default());
    let mut frame = render_frame();
    let mut frustum = PerspectiveFrustum::new(std::f64::consts::FRAC_PI_3, 1.0);
    frustum.far = 100_000_000.0;
    let camera = &mut frame.uniform.camera;
    camera.frustum = cesium_rs::renderer::UniformFrustum::Perspective(frustum);
    frame.culling_volume = frustum.compute_culling_volume(camera.position, camera.direction, camera.up);
    assert_eq!(globe.tiles_loaded(), expected["tilesLoadedBefore"]);
    assert_eq!(globe.get_height(&Cartographic::ZERO), expected["heightBeforeLoad"].as_f64());
    for _ in 0..6 {
        render_globe(&mut globe, &mut frame);
    }
    assert_eq!(globe.tiles_loaded(), expected["tilesLoadedAfter"]);
    let rendered = globe.rendered_surface_tiles();
    assert_eq!(rendered.len(), expected["renderedTiles3D"].as_array().unwrap().len());
    for (tile, reference) in rendered.iter().zip(expected["renderedTiles3D"].as_array().unwrap()) {
        let tile = tile.borrow();
        assert_eq!(tile.level, reference["level"]);
        assert_eq!(tile.x, reference["x"]);
        assert_eq!(tile.y, reference["y"]);
    }
    assert_eq!(frame.command_list.len(), expected["commands3DCount"]);
    assert!(frame.command_list.iter().all(|command| command.pass == Pass::Globe && command.metadata.receive_shadows));
    for case in expected["pick3DCases"].as_array().unwrap() {
        let [x, y, z] = reference_tuple(&case["origin"]);
        let [dx, dy, dz] = reference_tuple(&case["direction"]);
        let ray = Ray::new(Cartesian3::new(x, y, z), Cartesian3::new(dx, dy, dz));
        let actual = globe.pick_world_coordinates(&ray, &frame, true);
        if case["worldHit"].is_null() {
            assert!(actual.is_none(), "{}", case["name"]);
        } else {
            let [x, y, z] = reference_tuple(&case["worldHit"]);
            let error = actual.unwrap().distance(Cartesian3::new(x, y, z));
            assert!(error < 0.25, "{} differed by {error} m", case["name"]);
        }
    }
    globe.show = false;
    render_globe(&mut globe, &mut frame);
    assert_eq!(frame.command_list.len(), expected["commandsWhenHidden"]);
    globe.destroy().unwrap();
    assert_eq!(globe.is_destroyed(), expected["isDestroyedAfter"]);
}

#[test]
fn globe_translucency_state_matches_cesium_across_modes() {
    use serde_json::json;
    let fixture = terrain_reference();
    for case in fixture["globeTranslucencyState"]["stateCases"].as_array().unwrap() {
        let input = &case["input"];
        let scalar = |key: &str| {
            (!input[key].is_null()).then(|| {
                let [near, near_value, far, far_value] = reference_tuple(&input[key]);
                NearFarScalar::new(near, near_value, far, far_value)
            })
        };
        let mut config = GlobeTranslucency::new();
        config.enabled = input["enabled"].as_bool().unwrap();
        config.set_front_face_alpha(input["frontFaceAlpha"].as_f64().unwrap()).unwrap();
        config.set_back_face_alpha(input["backFaceAlpha"].as_f64().unwrap()).unwrap();
        config.set_front_face_alpha_by_distance(scalar("frontFaceAlphaByDistance")).unwrap();
        config.set_back_face_alpha_by_distance(scalar("backFaceAlphaByDistance")).unwrap();
        if !input["rectangleDeg"].is_null() {
            let [west, south, east, north] = reference_tuple(&input["rectangleDeg"]);
            config.set_rectangle(Some(Rectangle::from_degrees(west, south, east, north)));
        }
        let mut state = GlobeTranslucencyState::default();
        state.update(
            &config,
            GlobeTranslucencyFrame {
                show: input["show"].as_bool().unwrap(),
                base_alpha: input["baseAlpha"].as_f64().unwrap(),
                camera_underground: input["cameraUnderground"].as_bool().unwrap(),
                depth_test_against_terrain: input["depthTestAgainstTerrain"].as_bool().unwrap(),
                depth_texture: input["depthTexture"].as_bool().unwrap(),
                mode: reference_mode(&input["mode"]),
            },
        );
        let pack = |value: NearFarScalar| [value.near, value.near_value, value.far, value.far_value];
        for (key, value) in [
            ("frontFaceAlphaByDistance", state.front_face_alpha_by_distance),
            ("backFaceAlphaByDistance", state.back_face_alpha_by_distance),
        ] {
            assert_eq!(pack(value), reference_tuple(&case[key]), "{} {key}", case["name"]);
        }
        let checks = [
            ("translucent", json!(state.translucent())),
            ("sunVisibleThroughGlobe", json!(state.sun_visible_through_globe)),
            ("environmentVisible", json!(state.environment_visible)),
            ("useDepthPlane", json!(state.use_depth_plane)),
            ("numberOfTextureUniforms", json!(state.number_of_texture_uniforms)),
        ];
        for (key, actual) in checks {
            assert_eq!(actual, case[key], "{} {key}", case["name"]);
        }
        for (key, blend, pick) in
            [("renderCommands", false, false), ("blendCommands", true, false), ("pickCommands", false, true)]
        {
            let names: Vec<_> = state.command_types(blend, pick).iter().map(|kind| format!("{kind:?}")).collect();
            assert_eq!(json!(names), case[key], "{} {key}", case["name"]);
        }
    }
}

#[test]
fn terrain_fill_heights_and_triangles_match_cesium() {
    let fixture = terrain_reference();
    for (name, heights) in [("lowDetailGrid", [150.0, 150.0]), ("highDetailIsolated", [100.0, 300.0])] {
        let case = &fixture["terrainFillMesh"][name];
        let [west, south, east, north] = reference_tuple(&case["rectangle"]);
        let rectangle = Rectangle::new(west, south, east, north);
        let fill = TerrainFillMesh::new(
            rectangle,
            &Ellipsoid::WGS84,
            TerrainFillOptions {
                minimum_height: heights[0],
                maximum_height: heights[1],
                geometric_error: case["geometricError"].as_f64().unwrap(),
                ..Default::default()
            },
            &[],
        );
        assert_eq!(fill.mesh.minimum_height, case["minimumHeight"]);
        assert_eq!(fill.mesh.maximum_height, case["maximumHeight"]);
        if let Some(vertices) = case["vertices"].as_array() {
            assert_eq!(fill.mesh.vertex_count_without_skirts, vertices.len());
            assert_eq!(fill.mesh.indices.len(), case["indexCount"]);
            for vertex in vertices {
                let [u, v] = reference_tuple(&vertex["uv"]);
                let position = Cartographic::new(west + u * (east - west), south + v * (north - south), 0.0);
                assert_eq!(fill.height_at(&position), vertex["height"].as_f64());
            }
        } else {
            assert_eq!(fill.height_at(&rectangle.center()), case["centerHeight"].as_f64());
        }
    }
}

#[test]
fn globe_shader_variants_match_cesium() {
    use GlobeShaderFeature::*;
    let fixture = terrain_reference();
    let reference = &fixture["globeSurfaceShaderSet"];
    let context = Context::new_webgl2(RecordingGl::default());
    let mut shaders = GlobeSurfaceShaderSet::new(&context);
    for case in reference["cases"].as_array().unwrap().iter().chain([&reference["materialCase"]]) {
        let flags = case["flags"].as_u64().unwrap();
        let mode = reference_mode(&case["mode"]);
        let texture_count = case["numberOfDayTextures"].as_u64().unwrap() as usize;
        let mut features = GlobeShaderFeatures::default();
        for feature in [
            Brightness,
            Contrast,
            Hue,
            Saturation,
            Gamma,
            Alpha,
            WaterMask,
            ReflectiveOcean,
            OceanWaves,
            Lighting,
            DynamicLighting,
            SunLighting,
            GroundAtmosphere,
            PerFragmentAtmosphere,
            VertexNormals,
            WebMercator,
            Fog,
            Quantization,
            Split,
            ClippingPlanes,
            ClippingPolygons,
            TileLimit,
            Cutout,
            ColorCorrect,
            HighlightFill,
            ColorToAlpha,
            GeodeticNormals,
            Exaggeration,
            Underground,
            Translucent,
            DayNightAlpha,
        ] {
            features.set(feature, flags & feature as u64 != 0);
        }
        assert_eq!(features.key(mode), flags);
        let clipping_planes = features.contains(ClippingPlanes).then_some(GlobeClippingPlanesShader {
            length: 2,
            union: true,
            floating_point: true,
            texture_size: [2, 2],
        });
        let clipping_polygons = features
            .contains(ClippingPolygons)
            .then_some(GlobeClippingPolygonsShader { extents_count: 1, inverse: true });
        let options = GlobeSurfaceShaderOptions {
            mode,
            texture_count,
            features,
            clipping_planes,
            clipping_polygons,
            attributes: [
                ("position3DAndHeight", 0),
                ("textureCoordAndEncodedNormals", 1),
                ("geodeticSurfaceNormal", 2),
            ],
        };
        let material = (case["name"] == "material_color_1tex")
            .then_some("czm_material czm_getMaterial(czm_materialInput i) { return czm_getDefaultMaterial(i); }");
        let source = options.sources(material);
        for (key, stage) in [("vsDefines", &source.vertex_shader_source), ("fsDefines", &source.fragment_shader_source)]
        {
            let mut actual = stage.defines.iter().map(String::as_str).collect::<Vec<_>>();
            let mut expected = case[key].as_array().unwrap().iter().map(|v| v.as_str().unwrap()).collect::<Vec<_>>();
            actual.sort_unstable();
            expected.sort_unstable();
            assert_eq!(actual, expected, "{} {key}", case["name"]);
        }
        let vertex = source.vertex_shader_source.sources.join("\n");
        for key in ["positionModeSource", "yFractionSource"] {
            let helper = case[key].as_str().unwrap().split_once("return ").unwrap().1.split_once('(').unwrap().0;
            assert!(vertex.contains(&format!("return {helper}(")), "{} {key}", case["name"]);
        }
        shaders.set_material(material.map(str::to_owned)).unwrap();
        let first = shaders.get_shader_program(&options).unwrap();
        assert!(Rc::ptr_eq(&first, &shaders.get_shader_program(&options).unwrap()));
    }
}

#[test]
fn globe_tile_provider_visibility_and_distance_match_cesium() {
    let fixture = terrain_reference();
    let reference = &fixture["globeSurfaceTileProvider"]["visibilityAndDistance"];
    let context = Context::new_webgl2(RecordingGl::default());
    let mut globe = Globe::new(&context, GlobeOptions::default());
    let mut frame = render_frame();
    let mut frustum = PerspectiveFrustum::new(std::f64::consts::FRAC_PI_3, 1.0);
    frustum.far = 100_000_000.0;
    let camera = &mut frame.uniform.camera;
    camera.frustum = cesium_rs::renderer::UniformFrustum::Perspective(frustum);
    frame.culling_volume = frustum.compute_culling_volume(camera.position, camera.direction, camera.up);
    for _ in 0..6 {
        render_globe(&mut globe, &mut frame);
    }
    let root = globe.surface.find_tile((0, 0, 0)).unwrap();
    let root = root.borrow();
    let occluders = &globe.surface.occluders;
    let provider = &mut globe.surface.tile_provider;
    let visibility = |provider: &GlobeSurfaceTileProvider<_>, frame: &FrameState| match provider
        .compute_tile_visibility(&root, frame, Some(occluders))
    {
        Visibility::None => "NONE",
        Visibility::Partial => "PARTIAL",
        Visibility::Full => "FULL",
    };
    let distance = provider.compute_distance_to_tile(&root, &frame);
    assert!((distance - reference["distRoot0"].as_f64().unwrap()).abs() < 0.05);
    assert_eq!(provider.can_refine(&root), reference["canRefineRoot0"]);
    assert_eq!(visibility(provider, &frame), reference["visRoot0"]);
    provider.options.cartographic_limit_rectangle = Rectangle::from_degrees(40.0, -20.0, 70.0, 20.0);
    assert_eq!(visibility(provider, &frame), reference["visRoot0ExcludedByLimit"]);
    provider.options.cartographic_limit_rectangle = Rectangle::MAX_VALUE;
    frame.fog.enabled = true;
    frame.fog.density = 1.0;
    assert_eq!(visibility(provider, &frame), reference["visRoot0DenseFogAbove"]);
    frame.camera_underground = true;
    assert_eq!(visibility(provider, &frame), reference["visRoot0DenseFogUnderground"]);
}
