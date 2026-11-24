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
