//! Replay the pinned CesiumJS atmosphere, sun, environment-map, panorama,
//! provider, specular cube map and classification CPU cases in
//! `tests/data/scene-atmosphere-environment.json`.

mod common;

use cesium_rs::{
    core::{webgl_constants as gl, *},
    renderer::{
        self, Context, Gl, GlValue, RenderState, ShaderProgram, ShaderProgramOptions, ShaderSource, UniformCamera,
        UniformFrame, VertexArray,
    },
    scene::{
        classification_color_state, classification_pick_state, polyline_render_state, stencil_depth_state,
        ClassificationPrimitive, ClassificationPrimitiveOptions, ClassificationType, CubeMapPanorama,
        CubeMapPanoramaOptions, CubeMapSources, DynamicEnvironmentMapManager, DynamicEnvironmentMapManagerOptions,
        FrameResource, FrameState, GoogleStreetViewCubeMapPanoramaProvider,
        GoogleStreetViewCubeMapPanoramaProviderOptions, GroundPolylinePrimitive, GroundPolylinePrimitiveOptions,
        GroundPrimitive, GroundPrimitiveOptions, InvertClassification, SceneMode, ShadowMap, ShadowMapFrustum,
        ShadowMapLightCamera, SkyAtmosphere, Sun, SunPostProcess,
    },
    workers::GeometryDescription,
};
use common::{m3, n};
use serde_json::Value;
use std::{
    cell::Cell,
    future::Future,
    rc::Rc,
    sync::Arc,
    task::{Context as TaskContext, Poll, Wake, Waker},
};

struct ThreadWake(std::thread::Thread);
impl Wake for ThreadWake {
    fn wake(self: Arc<Self>) {
        self.0.unpark();
    }
}
fn number(args: &[GlValue<u32>], i: usize) -> f64 {
    match args.get(i) {
        Some(GlValue::Number(n)) => *n,
        _ => 0.0,
    }
}
fn block_on<T>(future: impl Future<Output = T>) -> T {
    let waker = Waker::from(Arc::new(ThreadWake(std::thread::current())));
    let mut cx = TaskContext::from_waker(&waker);
    let mut future = std::pin::pin!(future);
    loop {
        match future.as_mut().poll(&mut cx) {
            Poll::Ready(value) => return value,
            Poll::Pending => std::thread::park(),
        }
    }
}

#[derive(Default)]
struct EnvGl {
    next: Cell<u32>,
    size: Cell<Option<[u32; 2]>>,
    irradiance_pixels: std::cell::RefCell<Vec<u8>>,
}
impl Gl for EnvGl {
    type Object = u32;
    fn drawing_buffer_size(&self) -> Result<[u32; 2], RuntimeError> {
        Ok(self.size.get().unwrap_or([64, 64]))
    }
    fn invoke(&self, method: &str, args: &[GlValue<u32>]) -> Result<GlValue<u32>, RuntimeError> {
        if method.starts_with("create") {
            let next = self.next.get() + 1;
            self.next.set(next);
            return Ok(GlValue::Object(next));
        }
        Ok(match method {
            "getParameter" => match number(args, 0) as u32 {
                gl::MAX_VIEWPORT_DIMS | gl::ALIASED_LINE_WIDTH_RANGE | gl::ALIASED_POINT_SIZE_RANGE => {
                    GlValue::Numbers(vec![1.0, 16384.0])
                }
                _ => GlValue::Number(16384.0),
            },
            "getExtension" => GlValue::Object(1),
            "readPixels" => GlValue::Bytes(self.irradiance_pixels.borrow().clone()),
            "getShaderParameter" | "getProgramParameter" => GlValue::Bool(true),
            "checkFramebufferStatus" => GlValue::Number(f64::from(gl::FRAMEBUFFER_COMPLETE)),
            _ => GlValue::Null,
        })
    }
}

fn fixture() -> Value {
    serde_json::from_str(include_str!("data/scene-atmosphere-environment.json")).unwrap()
}

fn close(a: f64, b: f64) -> bool {
    (a - b).abs() <= 1e-12 * a.abs().max(b.abs()).max(1.0)
}
fn close_slice(actual: &[f64], expected: &Value) {
    let expected = expected.as_array().unwrap();
    assert_eq!(actual.len(), expected.len());
    for (a, e) in actual.iter().zip(expected) {
        assert!(close(*a, n(e)), "{a} != {}", n(e));
    }
}

fn encode_state(state: &RenderState) -> Value {
    serde_json::json!({
        "cull": {"enabled": state.cull.enabled, "face": state.cull.face},
        "depthTest": {"enabled": state.depth_test.enabled, "func": state.depth_test.func},
        "depthMask": state.depth_mask,
        "colorMask": {"red": state.color_mask.red, "green": state.color_mask.green, "blue": state.color_mask.blue, "alpha": state.color_mask.alpha},
        "polygonOffset": {"enabled": state.polygon_offset.enabled, "factor": state.polygon_offset.factor, "units": state.polygon_offset.units},
        "blending": {
            "enabled": state.blending.enabled,
            "rgb": state.blending.function_source_rgb,
            "alpha": state.blending.function_source_alpha,
            "drgb": state.blending.function_destination_rgb,
            "dalpha": state.blending.function_destination_alpha,
            "eq": state.blending.equation_rgb,
            "eqa": state.blending.equation_alpha,
        },
        "stencilTest": {
            "enabled": state.stencil_test.enabled,
            "frontFunction": state.stencil_test.front_function,
            "backFunction": state.stencil_test.back_function,
            "reference": state.stencil_test.reference,
            "mask": state.stencil_test.mask,
            "frontOperation": {"fail": state.stencil_test.front_operation.fail, "zFail": state.stencil_test.front_operation.z_fail, "zPass": state.stencil_test.front_operation.z_pass},
            "backOperation": {"fail": state.stencil_test.back_operation.fail, "zFail": state.stencil_test.back_operation.z_fail, "zPass": state.stencil_test.back_operation.z_pass},
        },
        "stencilMask": state.stencil_mask,
    })
}

fn assert_json_eq(actual: &Value, expected: &Value) {
    match (actual, expected) {
        (Value::Number(a), Value::Number(e)) => {
            assert!(
                (a.as_f64().unwrap() - e.as_f64().unwrap()).abs() <= 1e-12,
                "number {a} != {e} in {actual:?} vs {expected:?}"
            )
        }
        (Value::Array(a), Value::Array(e)) => {
            assert_eq!(a.len(), e.len(), "{actual} != {expected}");
            for (a, e) in a.iter().zip(e) {
                assert_json_eq(a, e);
            }
        }
        (Value::Object(a), Value::Object(e)) => {
            assert_eq!(a.len(), e.len(), "{actual} != {expected}");
            for (key, a) in a {
                assert_json_eq(a, &e[key]);
            }
        }
        _ => assert_eq!(actual, expected),
    }
}

fn context() -> Context<EnvGl> {
    Context::new_webgl2(EnvGl::default())
}

#[test]
fn sky_atmosphere_matches_reference() {
    let fixture = fixture();
    let mut sky = SkyAtmosphere::new(context(), Some(Ellipsoid::new(1000.5, 1200.5, 900.5)));
    let before = &fixture["skyAtmosphere"]["before"];
    assert_eq!(sky.show, before["show"].as_bool().unwrap());
    assert_eq!(sky.per_fragment_atmosphere, before["perFragmentAtmosphere"].as_bool().unwrap());
    close_slice(
        &[sky.ellipsoid().radii.x, sky.ellipsoid().radii.y, sky.ellipsoid().radii.z],
        &before["ellipsoidRadii"],
    );
    let radii = sky.radii_and_dynamic_atmosphere_color();
    close_slice(&[radii.x, radii.y, radii.z], &before["radiiAndColor"]);
    assert_eq!(sky.atmosphere_light_intensity, n(&before["lightIntensity"]));
    assert_eq!(sky.atmosphere_mie_anisotropy, n(&before["mieAnisotropy"]));
    sky.set_dynamic_lighting(cesium_rs::scene::DynamicAtmosphereLightingType::Sunlight);
    assert_eq!(sky.radii_and_dynamic_atmosphere_color().z, 2.0);
    let default = SkyAtmosphere::new(context(), None);
    let radii = default.radii_and_dynamic_atmosphere_color();
    close_slice(&[radii.x, radii.y, radii.z], &fixture["skyAtmosphere"]["defaultRadiiAndColor"]);
    close_slice(&default.scale_matrix().0, &fixture["skyAtmosphere"]["defaultScale"]);
}

#[test]
fn sun_matches_reference() {
    let fixture = fixture();
    let mut sun = Sun::new(context());
    assert!(sun.show);
    assert_eq!(sun.glow_factor(), 1.0);
    sun.set_glow_factor(-2.0);
    assert_eq!(sun.glow_factor(), n(&fixture["sun"]["clamped"]));
    sun.set_glow_factor(2.0);
    assert_eq!(sun.glow_factor(), n(&fixture["sun"]["after"]["glowFactor"]));
    assert!(sun.texture().is_none());
    assert!(sun.compute_command().is_none());
}

#[test]
fn sun_post_process_stages_match_reference() {
    let fixture = fixture();
    let post = SunPostProcess::new(&context()).unwrap();
    assert_eq!(post.len() as u64, fixture["sunPostProcess"]["length"].as_u64().unwrap());
    for (i, expected) in fixture["sunPostProcess"]["stages"].as_array().unwrap().iter().enumerate() {
        let stage = post.get(i).unwrap();
        assert_eq!(stage.texture_scale(), n(&expected["textureScale"]));
        assert_eq!(stage.force_power_of_two(), expected["forcePowerOfTwo"].as_bool().unwrap());
        assert_eq!(stage.sample_mode().value() as u64, expected["sampleMode"].as_u64().unwrap());
        let mut names: Vec<_> = stage.uniforms().keys().cloned().collect();
        names.sort();
        assert_eq!(
            names,
            expected["uniformNames"]
                .as_array()
                .unwrap()
                .iter()
                .map(|v| v.as_str().unwrap().to_string())
                .collect::<Vec<_>>()
        );
    }
}

#[test]
fn cube_map_panorama_matches_reference() {
    let fixture = fixture();
    let candidate = &fixture["cubeMapPanorama"];
    let sources = CubeMapSources::Urls([
        "px.png".into(),
        "nx.png".into(),
        "py.png".into(),
        "ny.png".into(),
        "pz.png".into(),
        "nz.png".into(),
    ]);
    let panorama = CubeMapPanorama::new(
        context(),
        CubeMapPanoramaOptions {
            sources,
            transform: Some(m3(&candidate["transform"])),
            show: Some(false),
            credit: Some(Credit::new(candidate["credit"].as_str().unwrap(), true)),
            return_command: Some(false),
        },
    );
    assert_eq!(panorama.show, candidate["show"].as_bool().unwrap());
    close_slice(&panorama.transform().unwrap().0, &candidate["transform"]);
    assert_eq!(panorama.credit().unwrap().html(), candidate["credit"].as_str().unwrap());
    assert!(panorama.cube_map().is_none());
}

#[test]
fn google_provider_urls_match_reference() {
    let fixture = fixture();
    let expected = &fixture["google"];
    let provider = GoogleStreetViewCubeMapPanoramaProvider::new(GoogleStreetViewCubeMapPanoramaProviderOptions {
        key: Some("KEY".into()),
        url: Some("https://example.com/streetview".into()),
        tile_size: Some(512),
        credit: None,
    })
    .unwrap();
    assert_eq!(provider.key(), Some("KEY"));
    assert_eq!(provider.tile_size(), 512);
    assert_eq!(provider.build_face_url(0, 0, 512, "PANO", None), expected["faceUrls"][0].as_str().unwrap());
    assert_eq!(provider.build_face_url(-90, -90, 512, "PANO", Some("SIG")), expected["faceUrls"][1].as_str().unwrap());
    let cartographic = Cartographic::from_degrees(12.5, -3.25, 0.0);
    let parsed = cesium_rs::scene::GoogleStreetViewPanoramaData::parse_metadata(
        &serde_json::json!({"location": {"lng": 12.5, "lat": -3.25}}),
    )
    .unwrap();
    assert!(close(parsed.longitude, cartographic.longitude));
    assert!(close(parsed.latitude, cartographic.latitude));
    let missing = GoogleStreetViewCubeMapPanoramaProvider::from_url(Default::default());
    match block_on(missing) {
        Ok(_) => panic!("missing key must fail"),
        Err(error) => assert!(error.message.contains("required"), "{}", error.message),
    }
}

#[test]
fn specular_environment_cube_map_matches_reference() {
    let fixture = fixture();
    let expected = &fixture["specular"];
    let mut map = cesium_rs::scene::specular_environment_cube_map::SpecularEnvironmentCubeMap::<EnvGl>::new(
        expected["url"].as_str().unwrap(),
    );
    assert_eq!(map.url(), expected["url"].as_str().unwrap());
    assert!(!map.ready());
    assert!(!map.is_loading());
    assert!(map.maximum_mipmap_level().is_none());
    let supported = renderer::ContextCapabilities {
        color_buffer_half_float: true,
        half_floating_point_texture: true,
        floating_point_texture: false,
        color_buffer_float: false,
        supports_texture_lod: true,
        ..Default::default()
    };
    assert_eq!(
        cesium_rs::scene::specular_environment_cube_map::SpecularEnvironmentCubeMap::<EnvGl>::is_supported(&supported),
        expected["supported"].as_bool().unwrap()
    );
    let unsupported = renderer::ContextCapabilities { supports_texture_lod: false, ..supported };
    assert_eq!(
        cesium_rs::scene::specular_environment_cube_map::SpecularEnvironmentCubeMap::<EnvGl>::is_supported(
            &unsupported
        ),
        expected["unsupported"].as_bool().unwrap()
    );
    let errors = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let counter = errors.clone();
    map.error_event().add_event_listener(move |_| {
        counter.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    });
    map.report_load_error(RuntimeError::new("failed"));
    assert_eq!(errors.load(std::sync::atomic::Ordering::Relaxed), 1);
}

#[test]
fn dynamic_environment_manager_matches_reference() {
    let fixture = fixture();
    let defaults = DynamicEnvironmentMapManager::<EnvGl>::new(Default::default());
    let expected = &fixture["dynamic"]["defaults"];
    assert_eq!(defaults.enabled, expected["enabled"].as_bool().unwrap());
    assert_eq!(defaults.should_update, expected["shouldUpdate"].as_bool().unwrap());
    assert_eq!(defaults.maximum_seconds_difference, n(&expected["maximumSecondsDifference"]));
    assert_eq!(defaults.maximum_position_epsilon, n(&expected["maximumPositionEpsilon"]));
    assert_eq!(defaults.atmosphere_scattering_intensity, n(&expected["atmosphereScatteringIntensity"]));
    assert_eq!(defaults.gamma, n(&expected["gamma"]));
    assert_eq!(defaults.brightness, n(&expected["brightness"]));
    assert_eq!(defaults.saturation, n(&expected["saturation"]));
    assert_eq!(defaults.ground_albedo, n(&expected["groundAlbedo"]));
    close_slice(
        &[
            defaults.ground_color.red,
            defaults.ground_color.green,
            defaults.ground_color.blue,
            defaults.ground_color.alpha,
        ],
        &expected["groundColor"],
    );
    assert_eq!(defaults.maximum_mipmap_level() as u64, expected["mipmapLevels"].as_u64().unwrap());
    assert_eq!(defaults.spherical_harmonic_coefficients()[0].x, 0.35449);
    assert!(defaults.position().is_none());

    let custom = DynamicEnvironmentMapManager::<EnvGl>::new(DynamicEnvironmentMapManagerOptions {
        mipmap_levels: Some(5),
        maximum_seconds_difference: Some(60.0),
        maximum_position_epsilon: Some(12.5),
        atmosphere_scattering_intensity: Some(3.0),
        gamma: Some(1.2),
        brightness: Some(0.8),
        saturation: Some(1.1),
        ground_color: Some(Color::from_css_color_string("#203b34").unwrap()),
        ground_albedo: Some(0.4),
        ..Default::default()
    });
    let expected = &fixture["dynamic"]["custom"];
    assert_eq!(custom.maximum_mipmap_level() as u64, expected["mipmapLevels"].as_u64().unwrap());
    close_slice(
        &[custom.ground_color.red, custom.ground_color.green, custom.ground_color.blue, custom.ground_color.alpha],
        &expected["groundColor"],
    );
    assert_eq!(
        DynamicEnvironmentMapManager::<EnvGl>::is_dynamic_update_supported(&renderer::ContextCapabilities {
            half_floating_point_texture: true,
            ..Default::default()
        }),
        fixture["dynamic"]["dynamicUpdateSupported"][0].as_bool().unwrap()
    );
    assert_eq!(
        DynamicEnvironmentMapManager::<EnvGl>::is_dynamic_update_supported(&renderer::ContextCapabilities {
            color_buffer_float: true,
            ..Default::default()
        }),
        fixture["dynamic"]["dynamicUpdateSupported"][1].as_bool().unwrap()
    );
    assert_eq!(
        DynamicEnvironmentMapManager::<EnvGl>::is_dynamic_update_supported(&renderer::ContextCapabilities::default()),
        fixture["dynamic"]["dynamicUpdateSupported"][2].as_bool().unwrap()
    );
}

#[test]
fn invert_classification_matches_reference() {
    let fixture = fixture();
    let expected = &fixture["invert"];
    let invert = InvertClassification::<EnvGl>::new().unwrap();
    assert!(invert.unclassified_command().is_none());
    let capabilities =
        renderer::ContextCapabilities { depth_texture: true, fragment_depth: true, ..Default::default() };
    assert_eq!(
        InvertClassification::<EnvGl>::is_translucency_supported(&capabilities),
        expected["translucencySupported"][0].as_bool().unwrap()
    );
    let capabilities =
        renderer::ContextCapabilities { depth_texture: true, fragment_depth: false, ..Default::default() };
    assert_eq!(
        InvertClassification::<EnvGl>::is_translucency_supported(&capabilities),
        expected["translucencySupported"][1].as_bool().unwrap()
    );
}

#[test]
fn classification_and_ground_render_states_match_reference() {
    let fixture = fixture();
    let expected = &fixture["classification"];
    assert_json_eq(&encode_state(&stencil_depth_state(true, false)), &expected["stencilTerrain"]);
    assert_json_eq(&encode_state(&stencil_depth_state(true, true)), &expected["stencilTiles"]);
    assert_json_eq(&encode_state(&stencil_depth_state(false, false)), &expected["stencilDisabled"]);
    assert_json_eq(&encode_state(&classification_color_state(true)), &expected["color"]);
    assert_json_eq(&encode_state(&classification_color_state(false)), &expected["colorDisabled"]);
    assert_json_eq(&encode_state(&classification_pick_state()), &expected["pick"]);
    let ground = &fixture["groundPolyline"];
    assert_json_eq(&encode_state(&polyline_render_state(SceneMode::Scene3D, false)), &ground["terrain"]);
    assert_json_eq(&encode_state(&polyline_render_state(SceneMode::Scene3D, true)), &ground["tiles"]);
    assert_json_eq(&encode_state(&polyline_render_state(SceneMode::Morphing, false)), &ground["morph"]);
}

#[test]
fn primitive_defaults_match_reference() {
    let fixture = fixture();
    let expected = &fixture["defaults"];
    let classification = ClassificationPrimitive::<EnvGl>::new(ClassificationPrimitiveOptions::default());
    assert_eq!(expected["classification"]["show"].as_bool().unwrap(), classification.show());
    assert_eq!(classification.classification_type, ClassificationType::Both);
    assert!(expected["classification"]["isSupported"].as_bool().unwrap());
    let ground = GroundPrimitive::<EnvGl>::new(GroundPrimitiveOptions::default());
    assert_eq!(ground.classification_type(), ClassificationType::Both);
    assert!(ground.show());
    let polyline = GroundPolylinePrimitive::<EnvGl>::new(GroundPolylinePrimitiveOptions::default());
    assert_eq!(polyline.classification_type, ClassificationType::Both);
    assert!(!polyline.debug_show_shadow_volume);
}

#[test]
fn ground_heights_and_bounds_match_reference() {
    let fixture = fixture();
    let expected = &fixture["ground"];
    let rectangle = common::rect(&expected["rectangle"]);
    let mut geometry_options = RectangleGeometryOptions::new(rectangle);
    geometry_options.height = 0.0;
    let description = GeometryDescription::RectangleGeometry(RectangleGeometry::new(geometry_options));
    let mut options = ClassificationPrimitiveOptions::default();
    options.primitive.geometry_instances = vec![GeometryInstance {
        geometry: None,
        model_matrix: Matrix4::IDENTITY,
        id: Some("rect".into()),
        attributes: vec![(
            "color".into(),
            GeometryInstanceAttribute::new(
                ComponentDatatype::UnsignedByte,
                4,
                Some(true),
                TypedArray::Uint8(vec![255, 0, 0, 255]),
            ),
        )],
        west_hemisphere_geometry: None,
        east_hemisphere_geometry: None,
    }];
    options.primitive.geometry_descriptions.insert(0, description);
    let mut ground = GroundPrimitive::<EnvGl>::new(options);
    let uniform = UniformFrame::new(
        UniformCamera {
            view: Matrix4::IDENTITY,
            inverse_view: Matrix4::IDENTITY,
            position: Cartesian3::new(7000000.0, 0.0, 0.0),
            direction: -Cartesian3::UNIT_X,
            right: -Cartesian3::UNIT_Y,
            up: Cartesian3::UNIT_Z,
            cartographic: None,
            frustum: renderer::UniformFrustum::Perspective(PerspectiveFrustum::new(1.0, 1.0)),
        },
        JulianDate::default(),
    );
    let mut frame = FrameState::new(
        uniform,
        CullingVolume::from_bounding_sphere(&BoundingSphere::new(Cartesian3::ZERO, 1.0)),
        [64, 64],
    );
    frame.context = Some(FrameResource::new(context()));
    frame.context_capabilities.depth_texture = true;
    frame.passes.render = true;
    ground.update(&mut frame).unwrap();
    let heights = ground.ground_heights().unwrap();
    assert!(close(heights[0], n(&expected["heights"]["minimumTerrainHeight"])), "{heights:?}");
    assert!(close(heights[1], n(&expected["heights"]["maximumTerrainHeight"])), "{heights:?}");
    let sphere = ground.get_bounding_sphere("rect").unwrap();
    close_slice(&[sphere.center.x, sphere.center.y, sphere.center.z, sphere.radius], &expected["obbSphere"]);
}

fn minimal_frame(context: &Context<EnvGl>) -> FrameState {
    let uniform = UniformFrame::new(
        UniformCamera {
            view: Matrix4::IDENTITY,
            inverse_view: Matrix4::IDENTITY,
            position: Cartesian3::new(7000000.0, 0.0, 0.0),
            direction: -Cartesian3::UNIT_X,
            right: -Cartesian3::UNIT_Y,
            up: Cartesian3::UNIT_Z,
            cartographic: None,
            frustum: renderer::UniformFrustum::Perspective(PerspectiveFrustum::new(1.0, 1.0)),
        },
        JulianDate::default(),
    );
    let mut frame = FrameState::new(
        uniform,
        CullingVolume::from_bounding_sphere(&BoundingSphere::new(Cartesian3::ZERO, 1.0)),
        [64, 64],
    );
    frame.context = Some(FrameResource::new(context.clone()));
    frame.passes.render = true;
    frame
}

fn dummy_instance() -> GeometryInstance {
    GeometryInstance {
        geometry: Some(Geometry::new(GeometryAttributes::default(), None, PrimitiveType::Triangles)),
        model_matrix: Matrix4::IDENTITY,
        id: Some("instance".into()),
        attributes: vec![(
            "color".into(),
            GeometryInstanceAttribute::new(
                ComponentDatatype::UnsignedByte,
                4,
                Some(true),
                TypedArray::Uint8(vec![255, 0, 0, 255]),
            ),
        )],
        west_hemisphere_geometry: None,
        east_hemisphere_geometry: None,
    }
}

#[test]
fn classification_validation_and_lifecycle() {
    let context = context();
    let mut frame = minimal_frame(&context);

    // Color attributes must be present and uniform.
    let mut options = ClassificationPrimitiveOptions::default();
    let mut instance = dummy_instance();
    instance.attributes.clear();
    options.primitive.geometry_instances = vec![instance];
    options.primitive.asynchronous = Some(false);
    let mut primitive = ClassificationPrimitive::<EnvGl>::new(options);
    let error = primitive.update(&mut frame).unwrap_err();
    assert_eq!(error.message, "All GeometryInstances must have the same color attribute except via GroundPrimitives");
    assert!(primitive.get_geometry_instance_attributes("instance").is_err());

    let mut options = ClassificationPrimitiveOptions::default();
    let mut first = dummy_instance();
    first.id = Some("a".into());
    let mut second = dummy_instance();
    second.id = Some("b".into());
    second.attributes[0].1 = GeometryInstanceAttribute::new(
        ComponentDatatype::UnsignedByte,
        4,
        Some(true),
        TypedArray::Uint8(vec![0, 255, 0, 255]),
    );
    options.primitive.geometry_instances = vec![first, second];
    options.primitive.asynchronous = Some(false);
    let mut primitive = ClassificationPrimitive::<EnvGl>::new(options);
    let error = primitive.update(&mut frame).unwrap_err();
    assert_eq!(error.message, "All GeometryInstances must have the same color attribute except via GroundPrimitives");
    primitive.destroy().unwrap();
    assert!(primitive.destroy().is_err());
}

#[test]
fn ground_polyline_requires_ground_polyline_geometry() {
    let context = context();
    let mut frame = minimal_frame(&context);
    let mut options = GroundPolylinePrimitiveOptions::default();
    options.primitive.geometry_instances = vec![dummy_instance()];
    options.primitive.asynchronous = Some(false);
    let mut primitive = GroundPolylinePrimitive::<EnvGl>::new(options);
    let error = primitive.update(&mut frame).unwrap_err();
    assert!(error.message.contains("GroundPolylineGeometry"));
    primitive.destroy().unwrap();
    assert!(primitive.destroy().is_err());
}

#[test]
fn atmosphere_sun_and_panorama_lifecycle() {
    let context = context();
    let frame = minimal_frame(&context);
    let mut sky = SkyAtmosphere::new(context.clone(), None);
    sky.update(&frame, Some(true)).unwrap();
    sky.destroy();
    assert!(sky.update(&frame, Some(true)).is_err());
    assert!(sky.is_destroyed());

    let mut sun = Sun::new(context.clone());
    sun.destroy();
    assert!(sun.update(&frame, renderer::Viewport { x: 0, y: 0, width: 64, height: 64 }, false).is_err());

    let mut panorama = CubeMapPanorama::<EnvGl>::new(
        context.clone(),
        CubeMapPanoramaOptions {
            sources: CubeMapSources::Urls(Default::default()),
            transform: None,
            show: Some(true),
            credit: None,
            return_command: Some(false),
        },
    );
    panorama.report_error(RuntimeError::new("load failed"));
    match panorama.update(&mut minimal_frame(&context), false) {
        Ok(_) => panic!("deferred error must be raised"),
        Err(error) => assert_eq!(error.message, "load failed"),
    }
}

#[test]
fn classification_builds_stencil_and_color_commands() {
    let context = context();
    let mut frame = minimal_frame(&context);
    let mut options = ClassificationPrimitiveOptions::default();
    let boxed = BoxGeometry::from_dimensions(Cartesian3::ONE, VertexFormat::POSITION_ONLY, None).unwrap();
    options.primitive.geometry_descriptions.insert(0, GeometryDescription::BoxGeometry(boxed));
    let mut instance = dummy_instance();
    instance.geometry = None;
    options.primitive.geometry_instances = vec![instance];
    options.primitive.asynchronous = Some(false);
    let mut primitive = ClassificationPrimitive::<EnvGl>::new(options);
    primitive.update(&mut frame).unwrap();
    assert!(primitive.is_ready());
    let passes: Vec<_> = frame.command_list.iter().map(|command| command.pass).collect();
    assert!(passes.contains(&renderer::Pass::TerrainClassification));
    assert!(passes.contains(&renderer::Pass::Cesium3DTileClassification));
    assert!(frame.command_list.iter().any(|command| command.id.contains("stencil")));
    assert!(frame.command_list.iter().any(|command| command.id.contains("color")));
    for command in &frame.command_list {
        if command.id.contains("stencil") || command.id.contains("color") {
            assert_eq!(command.metadata.owner.as_deref(), Some("ClassificationPrimitive"));
        }
    }
    primitive.destroy().unwrap();
}

#[test]
fn shadow_map_cast_and_receive_derivation() {
    let context = context();
    let light = ShadowMapLightCamera::new(
        Cartesian3::new(7000000.0, 0.0, 0.0),
        -Cartesian3::UNIT_X,
        Cartesian3::UNIT_Z,
        ShadowMapFrustum::Perspective(PerspectiveFrustum::new(1.0, 1.0)),
    )
    .unwrap();
    let mut shadow = ShadowMap::with_options(
        context.clone(),
        cesium_rs::scene::ShadowMapOptions { light_camera: light, ..Default::default() },
    )
    .unwrap();
    let frame = minimal_frame(&context);
    shadow.update(&frame).unwrap();
    let program = ShaderProgram::from_sources(
        &context,
        &ShaderProgramOptions {
            vertex_shader_source: ShaderSource::new("in vec4 position; void main() { gl_Position = position; }"),
            fragment_shader_source: ShaderSource::new("void main() { out_FragColor = vec4(1.0); }"),
            attribute_locations: std::collections::BTreeMap::from([("position".to_string(), 0)]),
        },
    )
    .unwrap();
    let vertices = Rc::new(
        VertexArray::new(&context, vec![renderer::VertexAttribute::constant(0, vec![0.0, 0.0, 0.0, 1.0])], None)
            .unwrap(),
    );
    let options = cesium_rs::scene::SceneDrawOptions::new(vertices, Rc::new(program));
    let cast = shadow.create_cast_draw(&context, &options, false, renderer::Pass::Opaque).unwrap();
    assert!(cast.program.vertex_shader_source().contains("SHADOW_MAP"));
    assert!(cast.program.fragment_shader_source().contains("out_FragColor = vec4(1.0)"));
    assert!(cast.uniforms.iter().any(|(name, _)| name == "shadowMap_matrix"));
    let receive = shadow.create_receive_draw(&context, &options, false, false, false).unwrap();
    assert!(receive.program.vertex_shader_source().contains("SHADOW_MAP"));
    assert!(receive.textures.iter().any(|(name, _)| name == "shadowMap_texture"));
    assert!(receive.uniforms.iter().any(|(name, _)| name == "shadowMap_matrix"));
}

#[test]
fn truncated_ktx2_specular_chain_uploads_placeholder_mips() {
    use cesium_rs::core::compressed_texture_buffer::{CompressedTextureBuffer, CompressedTextureBufferOptions};
    use cesium_rs::core::ktx2_transcoder::{CubeFace, Ktx2Texture};
    use std::sync::Arc;

    let level: Vec<_> = CubeFace::ALL
        .into_iter()
        .map(|face| {
            (
                face,
                CompressedTextureBuffer::new(CompressedTextureBufferOptions {
                    internal_format: PixelFormat::Rgba as u32,
                    pixel_datatype: Some(PixelDatatype::UnsignedByte as u32),
                    width: 4,
                    height: 4,
                    buffer: Arc::from(vec![0u8; 4 * 4 * 4].into_boxed_slice()),
                }),
            )
        })
        .collect();
    let mut map =
        cesium_rs::scene::specular_environment_cube_map::SpecularEnvironmentCubeMap::<EnvGl>::new("truncated.ktx2");
    map.upload(&context(), Ktx2Texture::Cubemap(vec![level])).unwrap();
    assert!(map.ready());
    assert_eq!(map.maximum_mipmap_level(), Some(0));
    assert!(map.texture().is_some());
}

#[test]
fn street_view_resolves_metadata_and_panorama_requests_like_cesium() {
    use cesium_rs::scene::GoogleStreetViewLoadPanoramaOptions;
    use serde_json::json;
    use std::{cell::RefCell, collections::VecDeque};

    #[derive(Default)]
    struct MetadataTransport {
        urls: RefCell<Vec<String>>,
        responses: RefCell<VecDeque<Value>>,
    }
    impl ResourceTransport for MetadataTransport {
        fn execute(&self, request: HttpRequest) -> RequestFuture<HttpResponse> {
            self.urls.borrow_mut().push(request.url);
            let response = self.responses.borrow_mut().pop_front().expect("Unexpected metadata request");
            Box::pin(async move {
                Ok(HttpResponse { status: 200, body: serde_json::to_vec(&response).unwrap(), ..Default::default() })
            })
        }
    }

    let fixture = fixture();
    let expected = &fixture["googleStreetView"];
    let metadata = expected["metaOk"].clone();
    let mut lookup = metadata.clone();
    lookup["pano_id"] = json!("PANO_LOOKUP");
    let host = Rc::new(MetadataTransport {
        responses: RefCell::new(VecDeque::from([
            metadata.clone(),
            metadata.clone(),
            metadata,
            json!({"status": "ZERO_RESULTS"}),
            json!({"status": "NOT_FOUND"}),
            lookup,
        ])),
        ..Default::default()
    });
    let client = ResourceClient::new(host.clone());
    let provider =
        block_on(GoogleStreetViewCubeMapPanoramaProvider::from_url(GoogleStreetViewCubeMapPanoramaProviderOptions {
            key: Some("TEST_KEY".into()),
            url: Some("https://maps.googleapis.com/maps/api/streetview".into()),
            tile_size: Some(512),
            credit: None,
        }))
        .unwrap();
    let degrees = &expected["cartographicDeg"];
    let cartographic = Cartographic::from_degrees(n(&degrees[0]), n(&degrees[1]), n(&degrees[2]));
    for (radius, key) in [(None, "nearestDefault"), (Some(120.0), "nearestCustomRadius")] {
        let result = block_on(provider.get_nearest_pano_id(&client, cartographic, radius)).unwrap();
        assert_json_eq(
            &json!({"panoId": result.pano_id, "latitude": result.latitude, "longitude": result.longitude}),
            &expected[key],
        );
    }
    assert_json_eq(&block_on(provider.get_pano_id_metadata(&client, "PANO_ABC")).unwrap(), &expected["metaOk"]);
    assert_eq!(
        block_on(provider.get_nearest_pano_id(&client, cartographic, None)).unwrap_err().message,
        expected["nearestErr"]
    );
    assert_eq!(block_on(provider.get_pano_id_metadata(&client, "BAD_PANO")).unwrap_err().message, expected["metaErr"]);
    let base = GoogleStreetViewLoadPanoramaOptions {
        cartographic,
        pano_id: None,
        tile_size: None,
        signature: None,
        credit: None,
    };
    let explicit = GoogleStreetViewLoadPanoramaOptions {
        pano_id: Some("PANO_SIGNED".into()),
        tile_size: Some(256),
        signature: Some("SIG123".into()),
        ..base.clone()
    };
    for (options, prefix) in [(explicit, "explicit"), (base, "lookup")] {
        let panorama = block_on(provider.load_panorama_data(&client, options)).unwrap();
        assert_eq!(json!(panorama.faces), expected[&format!("{prefix}FaceUrls")]);
        close_slice(&panorama.transform.0, &expected[&format!("{prefix}Transform")]);
        assert_eq!(panorama.credit.unwrap().html(), expected[&format!("{prefix}Credit")]);
    }
    assert_eq!(json!(*host.urls.borrow()), expected["requestedJsonUrls"]);
    assert!(host.responses.borrow().is_empty());
    let missing = block_on(GoogleStreetViewCubeMapPanoramaProvider::from_url(Default::default()));
    assert_eq!(missing.err().unwrap().message, expected["missingKeyErr"]);
}

fn atmosphere_runtime_reference() -> Value {
    serde_json::from_str(include_str!("data/scene-atmosphere-gpu.json")).unwrap()
}

#[test]
fn sky_atmosphere_draw_variants_match_cesium() {
    use cesium_rs::scene::{DynamicAtmosphereLightingType, GlobeTranslucency};
    let reference = atmosphere_runtime_reference();
    let expected = &reference["skyAtmosphere"];
    let camera = &expected["camera"];
    let context = context();
    let mut sky = SkyAtmosphere::new(context.clone(), None);
    sky.set_dynamic_lighting(DynamicAtmosphereLightingType::Sunlight);
    for case in expected["cases"].as_array().unwrap() {
        let name = case["name"].as_str().unwrap();
        let mut frame = minimal_frame(&context);
        frame.uniform.camera.inverse_view = common::m4(&camera["inverseView"]);
        frame.mode = match name {
            "morphing" => SceneMode::Morphing,
            "columbus-view-none" => SceneMode::ColumbusView,
            _ => SceneMode::Scene3D,
        };
        let globe = (name != "no-globe").then_some(true);
        sky.per_fragment_atmosphere = name == "per-fragment";
        if name == "translucent-color-correct" {
            sky.hue_shift = 0.25;
            sky.brightness_shift = -0.1;
            let mut translucency = GlobeTranslucency::new();
            translucency.enabled = true;
            frame.globe_translucency_state = Some(FrameResource::new(translucency));
        }
        frame.passes.render = name != "pick-only-none";
        sky.show = name != "hidden-none";
        let command = sky.update(&frame, globe).unwrap();
        assert_eq!(command.is_some(), case["hasCommand"], "{name}");
        if let Some(command) = command {
            assert_eq!(command.pass, renderer::Pass::Environment);
            close_slice(&sky.model_matrix().0, &case["modelMatrix"]);
            assert_eq!(serde_json::json!(sky.shader_defines().unwrap()), case["fsDefines"], "{name}");
            let r = sky.radii_and_dynamic_atmosphere_color();
            close_slice(&[r.x, r.y, r.z], &case["uniforms"]["u_radiiAndDynamicAtmosphereColor"]);
        }
    }
}

#[test]
fn panorama_cubemap_and_command_queue_match_cesium() {
    let reference = atmosphere_runtime_reference();
    let expected = &reference["cubeMapPanorama"];
    let context = context();
    let mut frame = minimal_frame(&context);
    let sources = || {
        CubeMapSources::Images(std::array::from_fn(|i| renderer::TextureSource::Pixels {
            width: 4,
            height: 4,
            bytes: [i as u8 * 30, 0, 255, 255].repeat(16),
        }))
    };
    let mut panorama = CubeMapPanorama::new(
        context.clone(),
        CubeMapPanoramaOptions {
            sources: sources(),
            transform: Some(m3(&expected["transform"])),
            credit: Some(Credit::new("Test Credit", true)),
            show: Some(true),
            return_command: Some(true),
        },
    );
    for hdr in [false, true] {
        let command = panorama.update(&mut frame, hdr).unwrap().unwrap();
        assert_eq!(command.pass, renderer::Pass::Environment);
    }
    let map = panorama.cube_map().unwrap();
    let texture = &expected["ldrSnap"]["cubeMap"];
    assert_eq!(map.width(), texture["width"]);
    assert_eq!(map.height(), texture["height"]);
    assert_eq!(map.pixel_format() as u32, texture["pixelFormat"]);
    assert_eq!(map.pixel_datatype() as u32, texture["pixelDatatype"]);

    let mut panorama = CubeMapPanorama::new(
        context.clone(),
        CubeMapPanoramaOptions {
            sources: sources(),
            transform: None,
            credit: Some(Credit::new("Pano Credit", true)),
            show: Some(true),
            return_command: Some(false),
        },
    );
    for prefix in ["first", "second"] {
        let mut frame = minimal_frame(&context);
        assert!(panorama.update(&mut frame, false).unwrap().is_none());
        assert_eq!(frame.panorama_command_list.len(), expected["listSnap"][format!("{prefix}ListLen")]);
        let credits: Vec<_> = frame.credits.iter().map(Credit::html).collect();
        assert_eq!(serde_json::json!(credits), expected["listSnap"][format!("{prefix}Credits")]);
    }
}

#[test]
fn specular_environment_mip_chains_match_cesium() {
    use cesium_rs::{
        core::{
            compressed_texture_buffer::CompressedTextureBufferOptions,
            ktx2_transcoder::{CubeFace, Ktx2Texture},
        },
        scene::specular_environment_cube_map::SpecularEnvironmentCubeMap,
    };
    let reference = atmosphere_runtime_reference();
    let context = context();
    for (key, levels) in [("fullChain", vec![4u32, 2, 1]), ("truncatedChain", vec![4])] {
        let expected = &reference["specularEnvironmentCubeMap"][key];
        let mut map = SpecularEnvironmentCubeMap::new("capture.ktx2");
        let mips = levels
            .into_iter()
            .map(|size| {
                CubeFace::ALL
                    .iter()
                    .map(|&face| {
                        (
                            face,
                            CompressedTextureBuffer::new(CompressedTextureBufferOptions {
                                internal_format: PixelFormat::Rgba as u32,
                                pixel_datatype: Some(PixelDatatype::UnsignedByte as u32),
                                width: size,
                                height: size,
                                buffer: vec![200u8; (size * size * 4) as usize].into(),
                            }),
                        )
                    })
                    .collect()
            })
            .collect();
        map.upload(&context, Ktx2Texture::Cubemap(mips)).unwrap();
        assert_eq!(map.ready(), expected["ready"]);
        assert_eq!(map.maximum_mipmap_level(), expected["maximumMipmapLevel"].as_u64().map(|n| n as u32));
        let texture = map.texture().unwrap();
        assert_eq!(texture.width(), expected["texture"]["width"]);
        assert_eq!(texture.height(), expected["texture"]["height"]);
        assert_eq!(texture.sampler().minification_filter as u32, expected["texture"]["sampler"]["minificationFilter"]);
        map.destroy().unwrap();
        assert_eq!(map.is_destroyed(), expected["destroyed"]);
    }
}

fn assert_classification_passes(frame: &FrameState, expected: &Value) {
    let actual = frame.command_list.iter().map(|c| c.pass as u32).collect::<Vec<_>>();
    let expected = expected.as_array().unwrap().iter().map(|c| c["pass"].as_u64().unwrap() as u32).collect::<Vec<_>>();
    assert_eq!(actual, expected);
}

#[test]
fn classification_box_render_and_pick_match_cesium() {
    let reference = atmosphere_runtime_reference();
    let context = context();
    for case in reference["classificationPrimitive"]["cases"].as_array().unwrap() {
        let name = case["name"].as_str().unwrap();
        let mut options = ClassificationPrimitiveOptions {
            classification_type: if name.starts_with("terrain-only") {
                ClassificationType::Terrain
            } else if name.starts_with("tiles-only") {
                ClassificationType::Cesium3DTile
            } else {
                ClassificationType::Both
            },
            debug_show_shadow_volume: name == "debug-shadow-volume",
            ..Default::default()
        };
        let geometry =
            BoxGeometry::from_dimensions(Cartesian3::new(10.0, 10.0, 10.0), VertexFormat::POSITION_ONLY, None).unwrap();
        let mut instance = dummy_instance();
        instance.geometry = None;
        instance.model_matrix = Matrix4::from_translation(Cartesian3::new(6378137.0, 0.0, 0.0));
        options.primitive.geometry_descriptions.insert(0, GeometryDescription::BoxGeometry(geometry));
        options.primitive.geometry_instances = vec![instance];
        options.primitive.asynchronous = Some(false);
        let mut primitive = ClassificationPrimitive::<EnvGl>::new(options);
        let mut frame = minimal_frame(&context);
        frame.passes.pick = name.ends_with("pick");
        frame.passes.render = !frame.passes.pick;
        frame.invert_classification = name == "both-invert-classification";
        primitive.update(&mut frame).unwrap();
        assert_eq!(primitive.is_ready(), case["ready"], "{name}");
        assert_classification_passes(&frame, &case["commands"]);
        primitive.destroy().unwrap();
    }
}

#[test]
fn invert_classification_resize_and_attachment_modes_match_cesium() {
    let reference = atmosphere_runtime_reference();
    let context = context();
    let mut invert = InvertClassification::new().unwrap();
    for step in reference["invertClassification"]["steps"].as_array().unwrap() {
        let opaque = step["label"] == "opaque-1x";
        let size = &step["colorDims"];
        context.host().size.set(Some([n(&size[0]) as u32, n(&size[1]) as u32]));
        let samples = n(&step["numSamples"]) as u32;
        invert.update(&context, samples, opaque, None).unwrap();
        let dimensions =
            |texture: Option<Rc<renderer::Texture<EnvGl>>>| serde_json::json!(texture.map(|t| [t.width(), t.height()]));
        assert_eq!(dimensions(invert.color_texture()), step["colorDims"]);
        assert_eq!(dimensions(invert.classified_texture()), step["classifiedDims"]);
        let program = &invert.unclassified_command().unwrap().shader_program;
        assert_eq!(program.fragment_shader_source().contains("gl_FragDepth"), step["unclassifiedFsHasDepth"]);
        assert_eq!(invert.translucent_command().is_some(), !step["translucentCmd"].is_null());
    }
    invert.destroy().unwrap();
}

#[test]
fn ground_classification_commands_match_cesium_in_3d_and_2d() {
    let reference = atmosphere_runtime_reference();
    let context = context();
    for case in reference["groundPrimitive"]["cases"].as_array().unwrap() {
        let name = case["name"].as_str().unwrap();
        if name == "spherical-material" {
            continue;
        }
        let d = &case["rectDeg"];
        let rectangle = Rectangle::from_degrees(n(&d[0]), n(&d[1]), n(&d[2]), n(&d[3]));
        let description =
            GeometryDescription::RectangleGeometry(RectangleGeometry::new(RectangleGeometryOptions::new(rectangle)));
        let mut instance = dummy_instance();
        instance.geometry = None;
        instance.id = Some(name.into());
        let mut options =
            GroundPrimitiveOptions { debug_show_shadow_volume: name == "debug-shadow-volume", ..Default::default() };
        options.primitive.geometry_instances = vec![instance];
        options.primitive.geometry_descriptions.insert(0, description);
        options.primitive.asynchronous = Some(false);
        let mut ground = GroundPrimitive::<EnvGl>::new(options);
        for (mode, key) in [(SceneMode::Scene3D, "commands3D"), (SceneMode::Scene2D, "commands2D")] {
            let mut frame = minimal_frame(&context);
            frame.mode = mode;
            frame.context_capabilities.depth_texture = true;
            frame.passes.pick = name == "planar-pick";
            frame.passes.render = !frame.passes.pick;
            if name == "planar-color-exaggerated" {
                frame.vertical_exaggeration = 2.0;
                frame.vertical_exaggeration_relative_height = 100.0;
            }
            ground.update(&mut frame).unwrap();
            assert_classification_passes(&frame, &case[key]);
            close_slice(&ground.ground_heights().unwrap(), &serde_json::json!([case["minHeight"], case["maxHeight"]]));
        }
        ground.destroy().unwrap();
    }
}

#[test]
fn ground_polyline_modes_and_pick_commands_match_cesium() {
    use cesium_rs::scene::{PolylineColorAppearance, PolylineMaterialAppearance};
    let reference = atmosphere_runtime_reference();
    let context = context();
    for case in reference["groundPolylinePrimitive"]["cases"].as_array().unwrap() {
        let name = case["name"].as_str().unwrap();
        let color = name == "color-terrain-debug";
        let allow_pick = name != "tiles-no-pick";
        let classification = if color {
            ClassificationType::Terrain
        } else if allow_pick {
            ClassificationType::Both
        } else {
            ClassificationType::Cesium3DTile
        };
        let positions = [(-112.134, 36.055), (-112.088, 36.098)]
            .into_iter()
            .map(|(longitude, latitude)| Cartesian3::from_degrees(longitude, latitude, 0.0, &Ellipsoid::WGS84))
            .collect();
        let mut geometry = GroundPolylineGeometry::new(positions).unwrap();
        geometry.width = 5.0;
        let mut options = GroundPolylinePrimitiveOptions {
            classification_type: classification,
            debug_show_shadow_volume: color,
            ..Default::default()
        };
        let mut instance = dummy_instance();
        instance.geometry = None;
        instance.id = Some(name.into());
        if !color {
            instance.attributes.clear();
        }
        options.primitive.geometry_instances = vec![instance];
        options.primitive.geometry_descriptions.insert(0, GeometryDescription::GroundPolylineGeometry(geometry));
        options.primitive.asynchronous = Some(false);
        options.primitive.allow_picking = Some(allow_pick);
        options.primitive.appearance = Some(if color {
            Box::new(PolylineColorAppearance::default())
        } else {
            Box::new(PolylineMaterialAppearance::default())
        });
        let mut primitive = GroundPolylinePrimitive::<EnvGl>::new(options);
        for (mode, pick, key) in [
            (SceneMode::Scene3D, false, "scene3d"),
            (SceneMode::ColumbusView, false, "columbus"),
            (SceneMode::Morphing, false, "morphing"),
            (SceneMode::Scene3D, true, "pick3d"),
            (SceneMode::Morphing, true, "pickMorph"),
        ] {
            let mut frame = minimal_frame(&context);
            frame.context_capabilities.depth_texture = true;
            frame.mode = mode;
            frame.passes.render = !pick;
            frame.passes.pick = pick;
            primitive.update(&mut frame).unwrap();
            assert_classification_passes(&frame, &case["modeResults"][key]);
        }
        assert_eq!(primitive.is_ready(), case["ready"]);
        primitive.destroy().unwrap();
    }
}

#[test]
fn sun_post_process_projection_and_targets_match_cesium() {
    let reference = atmosphere_runtime_reference();
    let expected = &reference["sunPostProcess"];
    let camera = &expected["camera"];
    let context = context();
    context.host().size.set(Some([128, 64]));
    let mut frustum = PerspectiveFrustum::new(n(&camera["fov"]), n(&camera["aspectRatio"]));
    frustum.near = n(&camera["near"]);
    frustum.far = n(&camera["far"]);
    {
        let mut uniforms = context.uniform_state();
        uniforms.update_camera(
            common::m4(&camera["view"]),
            common::m4(&camera["inverseView"]),
            common::c3(&camera["position"]),
        );
        uniforms.update_frustum(frustum.projection_matrix(), None, frustum.near, frustum.far);
        let sun = common::c3(&expected["sunPositionWC"]);
        uniforms
            .set("czm_sunPositionWC", renderer::UniformValue::Float(vec![sun.x as f32, sun.y as f32, sun.z as f32]));
    }
    let mut process = SunPostProcess::new(&context).unwrap();
    let framebuffer =
        process.update(&context, renderer::Viewport { x: 0, y: 0, width: 128, height: 64 }).unwrap().unwrap();
    let color = framebuffer.color_texture(0).unwrap();
    assert_eq!(serde_json::json!([color.width(), color.height()]), expected["sceneFboColorDims"]);
    let (center, radius) = process.sun_center_and_radius();
    let check_pixels = |actual: &[f64], expected: &Value| {
        for (a, b) in actual.iter().zip(expected.as_array().unwrap()) {
            assert!((a - n(b)).abs() < 1.0e-5, "{a} != {b}");
        }
    };
    check_pixels(&[center.x, center.y], &expected["uCenter"]);
    assert!((radius - n(&expected["uRadius"])).abs() < 1.0e-5);
    for (i, stage) in expected["stages"].as_array().unwrap().iter().enumerate() {
        let actual = process.get(i).unwrap();
        let output = actual.output_texture().unwrap();
        assert_eq!(serde_json::json!([output.width(), output.height()]), stage["outputDims"]);
        if i < 4 {
            let scissor = actual.scissor_rectangle().unwrap();
            check_pixels(&[scissor.x, scissor.y, scissor.width, scissor.height], &stage["scissorRectangle"]);
        }
    }
    process.clear(&context, &renderer::PassState::default(), Color::BLACK).unwrap();
    process.execute(&context).unwrap();
}

#[test]
fn shadow_framebuffer_layout_and_texture_kind_match_cesium() {
    use cesium_rs::scene::ShadowMapOptions;
    use renderer::TextureBinding;
    let reference = atmosphere_runtime_reference();
    let expected = &reference["shadowMap"];
    let camera = &expected["sceneCamera"];
    let context = context();
    let mut frame = minimal_frame(&context);
    let mut frustum = PerspectiveFrustum::new(n(&camera["fov"]), n(&camera["aspectRatio"]));
    frustum.near = n(&camera["near"]);
    frustum.far = n(&camera["far"]);
    frame.uniform.camera = UniformCamera {
        view: common::m4(&camera["view"]),
        inverse_view: common::m4(&camera["inverseView"]),
        position: common::c3(&camera["position"]),
        direction: common::c3(&camera["direction"]),
        right: common::c3(&camera["right"]),
        up: common::c3(&camera["up"]),
        cartographic: None,
        frustum: renderer::UniformFrustum::Perspective(frustum),
    };
    let light = &expected["lightCamera"];
    frustum.far = n(&light["far"]);
    for case in expected["cases"].as_array().unwrap() {
        let point = case["isPointLight"].as_bool().unwrap();
        let mut shadow = ShadowMap::with_options(
            context.clone(),
            ShadowMapOptions {
                light_camera: ShadowMapLightCamera::new(
                    common::c3(&light["position"]),
                    common::c3(&light["direction"]),
                    common::c3(&light["up"]),
                    ShadowMapFrustum::Perspective(frustum),
                )
                .unwrap(),
                size: Some(64),
                is_point_light: Some(point),
                point_light_radius: point.then_some(250.0),
                cascades_enabled: Some(!point && case["name"] != "spot-light"),
                soft_shadows: Some(case["name"] == "cascaded-4-soft"),
                ..Default::default()
            },
        )
        .unwrap();
        shadow.update(&frame).unwrap();
        shadow.update_pass(0).unwrap();
        assert_eq!(shadow.passes().len(), case["passCount"]);
        let texture = &case["texture"];
        let (width, height, format, datatype, cube) = match shadow.texture().unwrap() {
            TextureBinding::Texture(t) => (t.width(), t.height(), t.pixel_format(), t.pixel_datatype(), false),
            TextureBinding::CubeMap(t) => (t.width(), t.height(), t.pixel_format(), t.pixel_datatype(), true),
            TextureBinding::Volume(_) => panic!("unexpected shadow volume"),
        };
        assert_eq!(width, texture["width"]);
        assert_eq!(height, texture["height"]);
        assert_eq!(format as u32, texture["pixelFormat"]);
        assert_eq!(datatype as u32, texture["pixelDatatype"]);
        assert_eq!(cube, texture["isCubeMap"]);
        for (pass, viewport) in shadow.passes().iter().zip(case["viewports"].as_array().unwrap()) {
            let renderer::Viewport { x, y, width, height } = pass.viewport;
            assert_eq!(serde_json::json!([x, y, width, height]), *viewport);
            assert!(pass.framebuffer.is_some());
        }
    }
}

#[test]
fn environment_map_compute_stages_and_irradiance_readback_match_cesium() {
    use cesium_rs::scene::{Atmosphere, DynamicAtmosphereLightingType};
    let reference = atmosphere_runtime_reference();
    let expected = &reference["dynamicEnvironmentMapManager"];
    let context = context();
    *context.host().irradiance_pixels.borrow_mut() = expected["rawIrradianceFloats"]
        .as_array()
        .unwrap()
        .iter()
        .flat_map(|value| (n(value) as f32).to_le_bytes())
        .collect();
    let mut frame = minimal_frame(&context);
    frame.uniform.time = JulianDate::from_iso8601("2025-06-21T12:00:00Z").unwrap();
    frame.atmosphere =
        Some(Atmosphere { dynamic_lighting: DynamicAtmosphereLightingType::Sunlight, ..Default::default() });
    let mut manager = DynamicEnvironmentMapManager::<EnvGl>::new(DynamicEnvironmentMapManagerOptions {
        mipmap_levels: Some(3),
        brightness: Some(1.1),
        atmosphere_scattering_intensity: Some(2.5),
        ..Default::default()
    });
    manager.set_position(Some(common::c3(&expected["position"])));
    let mut engine = renderer::ComputeEngine::new(&context);
    for step in expected["frames"].as_array().unwrap() {
        manager.update(&frame).unwrap();
        let mut commands = manager.take_commands();
        assert_eq!(commands.len(), step["queuedThisFrame"], "frame {}", step["step"]);
        for (cmd, reference) in commands.iter_mut().zip(step["cmdSummaries"].as_array().unwrap()) {
            let output = cmd.output_texture.as_ref().unwrap();
            assert_eq!(serde_json::json!([output.width(), output.height()]), reference["outputDims"]);
            assert_eq!(output.pixel_datatype() as u32, reference["outputDatatype"]);
            assert_eq!(cmd.persists, reference["persists"]);
            cmd.execute(&mut engine).unwrap();
        }
        assert_eq!(manager.should_regenerate_shaders(), step["shouldRegenerateShaders"], "frame {}", step["step"]);
    }
    close_slice(&manager.radii_and_dynamic_atmosphere_color().to_array(), &expected["radiiAndDynamicAtmosphereColor"]);
    for (coefficient, expected) in
        manager.spherical_harmonic_coefficients().iter().zip(expected["shCoefficients"].as_array().unwrap())
    {
        close_slice(&[coefficient.x, coefficient.y, coefficient.z], expected);
    }
}

#[test]
fn sun_compute_and_draw_commands_match_cesium() {
    use cesium_rs::scene::{Camera, CameraOptions};
    let reference = atmosphere_runtime_reference();
    let expected = &reference["sun"];
    let context = context();
    let mut sun = Sun::new(context.clone());
    let mut engine = renderer::ComputeEngine::new(&context);

    let mut camera = Camera::new(CameraOptions::new(256.0, 128.0));
    camera.position = Cartesian3::new(0.0, -25_000_000.0, 0.0);
    camera.direction = Cartesian3::UNIT_Y;
    camera.up = Cartesian3::UNIT_Z;
    camera.right = Cartesian3::UNIT_X;

    let mut frame = minimal_frame(&context);
    frame.uniform.time = JulianDate::from_iso8601("2025-06-21T12:00:00Z").unwrap();
    frame.uniform.camera = camera.uniform_camera();
    let sun_pos = common::c3(&expected["sunPositionWC"]);
    frame.uniform.celestial_rotation = None;
    let computed_sun = cesium_rs::scene::sun::sun_position(&frame);
    // Verify celestial sun position matches the pinned date within a tiny relative tolerance.
    let diff = (computed_sun - sun_pos).magnitude();
    assert!(diff / sun_pos.magnitude() < 1.0e-5, "sun position mismatch: {computed_sun:?} vs {sun_pos:?}");

    for case in expected["cases"].as_array().unwrap() {
        let vp = case["viewport"].as_array().unwrap();
        let viewport = renderer::Viewport {
            x: vp[0].as_i64().unwrap() as i32,
            y: vp[1].as_i64().unwrap() as i32,
            width: vp[2].as_u64().unwrap() as u32,
            height: vp[3].as_u64().unwrap() as u32,
        };
        let mode = match case["mode"].as_u64().unwrap() {
            1 => SceneMode::ColumbusView,
            2 => SceneMode::Scene2D,
            _ => SceneMode::Scene3D,
        };
        let glow = n(&case["glowFactor"]);
        if (sun.glow_factor() - glow).abs() > 1.0e-12 {
            sun.set_glow_factor(glow);
        }
        frame.mode = mode;
        frame.passes.render = true;
        let use_hdr = case["useHdr"].as_bool().unwrap();
        let out = sun.update(&frame, viewport, use_hdr).unwrap();
        assert_eq!(out.is_some(), case["hasCommands"].as_bool().unwrap(), "case {}", case["name"]);
        if let Some(cmds) = out {
            let mut compute_ran = false;
            if let Some(mut compute) = cmds.compute_command {
                let uniforms = (compute.uniform_map.as_ref().unwrap())().unwrap();
                let radius_ts = match &uniforms[0].1 {
                    renderer::UniformValue::Float(v) => f64::from(v[0]),
                    other => panic!("unexpected u_radiusTS: {other:?}"),
                };
                assert!(
                    (radius_ts - n(&case["radiusTS"])).abs() < 1.0e-6,
                    "{}: radiusTS {radius_ts} != {}",
                    case["name"],
                    case["radiusTS"]
                );
                compute.execute(&mut engine).unwrap();
                compute_ran = true;
            }
            assert_eq!(compute_ran, case["computeRan"].as_bool().unwrap(), "case {}", case["name"]);
            assert!((sun.glow_length_ts() - n(&case["glowLengthTS"])).abs() < 1.0e-9);
            assert!((sun.size() - n(&case["size"])).abs() <= 1.0, "size mismatch for {}", case["name"]);
            let tex = sun.texture().unwrap();
            assert_eq!(tex.width(), case["texture"]["width"].as_u64().unwrap() as u32);
            assert_eq!(tex.height(), case["texture"]["height"].as_u64().unwrap() as u32);
            assert_eq!(tex.pixel_format() as u32, case["texture"]["pixelFormat"].as_u64().unwrap() as u32);
            assert_eq!(tex.pixel_datatype() as u32, case["texture"]["pixelDatatype"].as_u64().unwrap() as u32);
            let sampler = tex.sampler();
            assert_eq!(
                sampler.minification_filter as u32,
                case["texture"]["sampler"]["minificationFilter"].as_u64().unwrap() as u32
            );
            assert_eq!(
                sampler.magnification_filter as u32,
                case["texture"]["sampler"]["magnificationFilter"].as_u64().unwrap() as u32
            );
            let bv = cmds.draw_command.metadata.bounding_volume.unwrap();
            assert!((bv.radius - n(&case["boundingVolume"]["radius"])).abs() < 1.0e-3);
        }
    }
    sun.destroy();
    assert!(sun.is_destroyed());
}
