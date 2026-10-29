#![recursion_limit = "256"]
use cesium_rs::core::{
    BoundingRectangle, Cartesian2, Cartesian3, Color, Ellipsoid, JulianDate, Matrix4, NearFarScalar,
    Rectangle, TimeStandard,
};
use cesium_rs::renderer::UniformFrame;
use cesium_rs::scene::camera::{Camera, CameraOptions};
use cesium_rs::scene::frame_state::FrameState;
use cesium_rs::scene::{
    BillboardLoadState, BillboardTexture, Cesium3DTileContentFactory, Cesium3DTileContentType,
    DebugInspector, EllipsoidPrimitive, EllipsoidPrimitiveOptions, EquirectangularPanorama,
    EquirectangularPanoramaOptions, FrameRateMonitor, FrameRateMonitorOptions, GlobeTranslucency,
    ImageBasedLighting, IonImageryExternalType, IonImageryProviderFactory, Moon, MoonOptions,
    ScenePrimitive,
};
use serde_json::json;

fn make_dummy_frame_state() -> FrameState {
    let mut camera = Camera::new(CameraOptions::new(800.0, 600.0));
    let uniform_camera = camera.uniform_camera();
    let culling = camera.frustum.compute_culling_volume(
        uniform_camera.position,
        uniform_camera.direction,
        uniform_camera.up,
    );
    FrameState::new(
        UniformFrame::new(
            uniform_camera,
            JulianDate::new(2451545.0, 0.0, TimeStandard::Tai),
        ),
        culling,
        [800, 600],
    )
}

#[test]
fn test_billboard_texture_lifecycle() {
    let mut texture = BillboardTexture::new();
    assert_eq!(texture.load_state(), BillboardLoadState::None);
    assert!(!texture.ready());
    assert!(!texture.has_image());
    assert_eq!(texture.index(), -1);
    assert!(texture.id().is_none());
    assert!(texture.width().is_none());
    assert!(texture.height().is_none());
    assert!(!texture.has_subregion());

    texture.set_loading("icon_pin");
    assert_eq!(texture.load_state(), BillboardLoadState::Loading);
    assert!(texture.has_image());
    assert!(!texture.ready());
    assert_eq!(texture.id(), Some("icon_pin"));

    texture.set_loaded("icon_pin", 2, 64.0, 32.0);
    assert_eq!(texture.load_state(), BillboardLoadState::Loaded);
    assert!(texture.ready());
    assert_eq!(texture.index(), 2);
    assert_eq!(texture.width(), Some(64.0));
    assert_eq!(texture.height(), Some(32.0));
    assert!(texture.dirty);

    // Subregion assignment
    let sub = BoundingRectangle::new(16.0, 8.0, 32.0, 16.0);
    texture.set_image_subregion(3, sub);
    assert!(texture.has_subregion());
    assert_eq!(texture.width(), Some(32.0));
    assert_eq!(texture.height(), Some(16.0));
    // Clone behavior
    let mut clone = BillboardTexture::new();
    clone.clone_from_source(&texture);
    assert_eq!(clone.id(), Some("icon_pin"));
    assert_eq!(clone.index(), 2);
    assert!(clone.ready());

    // Error and failure transitions
    texture.set_error("Network timeout");
    assert_eq!(texture.load_state(), BillboardLoadState::Error);
    assert_eq!(texture.load_error(), Some("Network timeout"));

    texture.set_failed();
    assert_eq!(texture.load_state(), BillboardLoadState::Failed);
    assert_eq!(texture.index(), -1);
}

#[test]
fn test_cesium_3d_tile_content_factory() {
    assert_eq!(
        Cesium3DTileContentFactory::find("b3dm"),
        Some(Cesium3DTileContentType::Batched3DModel)
    );
    assert_eq!(
        Cesium3DTileContentFactory::find("i3dm"),
        Some(Cesium3DTileContentType::Instanced3DModel)
    );
    assert_eq!(
        Cesium3DTileContentFactory::find("pnts"),
        Some(Cesium3DTileContentType::PointCloud)
    );
    assert_eq!(
        Cesium3DTileContentFactory::find("cmpt"),
        Some(Cesium3DTileContentType::Composite)
    );
    assert_eq!(
        Cesium3DTileContentFactory::find("geom"),
        Some(Cesium3DTileContentType::Geometry)
    );
    assert_eq!(
        Cesium3DTileContentFactory::find("vctr"),
        Some(Cesium3DTileContentType::Vector)
    );
    assert_eq!(
        Cesium3DTileContentFactory::find("subt"),
        Some(Cesium3DTileContentType::ImplicitSubtree)
    );
    assert_eq!(
        Cesium3DTileContentFactory::find("subtreeJson"),
        Some(Cesium3DTileContentType::ImplicitSubtreeJson)
    );
    assert_eq!(
        Cesium3DTileContentFactory::find("externalTileset"),
        Some(Cesium3DTileContentType::ExternalTileset)
    );
    assert_eq!(
        Cesium3DTileContentFactory::find("glb"),
        Some(Cesium3DTileContentType::GltfBinary)
    );
    assert_eq!(
        Cesium3DTileContentFactory::find("gltf"),
        Some(Cesium3DTileContentType::Gltf)
    );
    assert_eq!(
        Cesium3DTileContentFactory::find("geoJson"),
        Some(Cesium3DTileContentType::GeoJson)
    );

    assert!(Cesium3DTileContentFactory::is_binary("b3dm"));
    assert!(Cesium3DTileContentFactory::is_binary("glb"));
    assert!(!Cesium3DTileContentFactory::is_binary("gltf"));
    assert!(!Cesium3DTileContentFactory::is_binary("externalTileset"));

    // Payload byte classification
    let b3dm_header = b"b3dm\x01\x00\x00\x00";
    assert_eq!(
        Cesium3DTileContentFactory::classify(b3dm_header).unwrap(),
        Cesium3DTileContentType::Batched3DModel
    );

    let glb_bytes = b"glTF\x02\x00\x00\x00\x40\x00\x00\x00";
    assert_eq!(
        Cesium3DTileContentFactory::classify(glb_bytes).unwrap(),
        Cesium3DTileContentType::GltfBinary
    );

    let tileset_json = br#"{"root":{"boundingVolume":{"box":[0,0,0,1,0,0,0,1,0,0,0,1]}}}"#;
    assert_eq!(
        Cesium3DTileContentFactory::classify(tileset_json).unwrap(),
        Cesium3DTileContentType::ExternalTileset
    );

    let subtree_json = br#"{"buffers":[],"bufferViews":[],"tileAvailability":{"bitstream":0}}"#;
    assert_eq!(
        Cesium3DTileContentFactory::classify(subtree_json).unwrap(),
        Cesium3DTileContentType::ImplicitSubtreeJson
    );
}

#[test]
fn test_debug_inspector_shader_and_frustum_colors() {
    assert_eq!(DebugInspector::compute_frustum_color(0), Color::new(0.0, 0.0, 0.0, 1.0));
    assert_eq!(DebugInspector::compute_frustum_color(1), Color::new(1.0, 0.0, 0.0, 1.0));
    assert_eq!(DebugInspector::compute_frustum_color(2), Color::new(0.0, 1.0, 0.0, 1.0));
    assert_eq!(DebugInspector::compute_frustum_color(4), Color::new(0.0, 0.0, 1.0, 1.0));
    assert_eq!(DebugInspector::compute_frustum_color(7), Color::new(1.0, 1.0, 1.0, 1.0));

    let simple_fs = "void main() { out_FragColor = vec4(1.0); }";
    let rewritten = DebugInspector::rewrite_fragment_shader_for_debug(simple_fs);
    assert!(rewritten.contains("void czm_Debug_main()"));
    assert!(rewritten.contains("debugShowCommandsColor"));
    assert!(rewritten.contains("debugShowFrustumsColor"));
    assert!(rewritten.contains("out_FragColor.rgb *= debugShowCommandsColor;"));

    let mrt_fs = "void main() { out_FragData_0 = vec4(1.0); out_FragData_1 = vec4(0.5); }";
    let mrt_rewritten = DebugInspector::rewrite_fragment_shader_for_debug(mrt_fs);
    assert!(mrt_rewritten.contains("out_FragData_0.rgb *= debugShowCommandsColor;"));
    assert!(mrt_rewritten.contains("out_FragData_1.rgb *= debugShowCommandsColor;"));

    let mut inspector = DebugInspector::new();
    let cached = inspector.get_or_create_debug_shader("shader_1", simple_fs);
    assert!(cached.contains("czm_Debug_main"));
    inspector.clear();
}

#[test]
fn test_ellipsoid_primitive_computations_and_lifecycle() {
    let mut primitive = EllipsoidPrimitive::new(EllipsoidPrimitiveOptions {
        center: Some(Cartesian3::new(100.0, 200.0, 300.0)),
        radii: Some(Cartesian3::new(10.0, 20.0, 40.0)),
        model_matrix: Some(Matrix4::IDENTITY),
        ..Default::default()
    });

    assert_eq!(primitive.computed_model_matrix().get_translation(), Cartesian3::new(100.0, 200.0, 300.0));
    let inv_sq = primitive.one_over_ellipsoid_radii_squared();
    assert!((inv_sq.x - 1.0 / 100.0).abs() < 1e-9);
    assert!((inv_sq.y - 1.0 / 400.0).abs() < 1e-9);
    assert!((inv_sq.z - 1.0 / 1600.0).abs() < 1e-9);

    assert_eq!(primitive.bounding_sphere().radius, 40.0);
    assert_eq!(primitive.bounding_sphere().center, Cartesian3::new(100.0, 200.0, 300.0));

    let mut frame_state = make_dummy_frame_state();
    assert!(primitive.update(&mut frame_state).is_ok());

    assert!(primitive.destroy().is_ok());
    assert!(primitive.is_destroyed());
    assert!(primitive.update(&mut frame_state).is_err());
}

#[test]
fn test_equirectangular_panorama_setup_and_transforms() {
    let options = EquirectangularPanoramaOptions {
        image: "https://example.com/pano.jpg".to_string(),
        radius: Some(50_000.0),
        transform: Some(Matrix4::from_translation(Cartesian3::new(10.0, 20.0, 30.0))),
        repeat_horizontal: Some(2.0),
        repeat_vertical: Some(1.0),
        credit: None,
        show: Some(true),
    };
    let mut panorama = EquirectangularPanorama::new(options);

    assert_eq!(panorama.radius, 50_000.0);
    assert_eq!(panorama.bounding_sphere().radius, 50_000.0);
    assert_eq!(
        panorama.bounding_sphere().center,
        Cartesian3::new(10.0, 20.0, 30.0)
    );

    let mut frame_state = make_dummy_frame_state();
    assert!(panorama.update(&mut frame_state).is_ok());

    assert!(panorama.destroy().is_ok());
    assert!(panorama.is_destroyed());
    assert!(panorama.update(&mut frame_state).is_err());
}

#[test]
fn test_frame_rate_monitor_averaging_and_events() {
    let mut monitor = FrameRateMonitor::new(FrameRateMonitorOptions {
        sampling_window: 1.0,
        quiet_period: 0.5,
        warmup_period: 1.0,
        minimum_frame_rate_during_warmup: 10.0,
        minimum_frame_rate_after_warmup: 20.0,
    });

    assert!(!monitor.is_paused());
    assert!(monitor.last_frames_per_second().is_none());

    // During quiet period (0.0 to 0.5s), frames are ignored for alerts
    monitor.update(0.0);
    monitor.update(0.2);
    monitor.update(0.4);
    assert!(monitor.pop_low_frame_rate_event().is_none());

    // Steady 30 FPS updates past quiet period
    let mut t = 0.6;
    for _ in 0..40 {
        monitor.update(t);
        t += 1.0 / 30.0;
    }

    assert!(monitor.last_frames_per_second().is_some());
    let fps = monitor.last_frames_per_second().unwrap();
    assert!((fps - 30.0).abs() < 2.0);
    assert!(!monitor.is_frame_rate_low());

    // Degrade frame rate to 2 FPS (< 10 during warmup)
    t += 0.5;
    monitor.update(t);
    t += 0.5;
    monitor.update(t);
    t += 0.5;
    monitor.update(t);

    if monitor.is_frame_rate_low() {
        let low_event = monitor.pop_low_frame_rate_event();
        assert!(low_event.is_some());
    }

    // Pause clears state
    monitor.pause();
    assert!(monitor.is_paused());
    assert!(monitor.last_frames_per_second().is_none());
    monitor.unpause();
    assert!(!monitor.is_paused());
}

#[test]
fn test_globe_translucency_properties_and_validation() {
    let mut translucency = GlobeTranslucency::new();
    assert!(!translucency.enabled);
    assert_eq!(translucency.front_face_alpha(), 1.0);
    assert_eq!(translucency.back_face_alpha(), 1.0);
    assert_eq!(translucency.rectangle, Rectangle::MAX_VALUE);

    assert!(translucency.set_front_face_alpha(0.5).is_ok());
    assert_eq!(translucency.front_face_alpha(), 0.5);
    assert!(translucency.set_front_face_alpha(1.5).is_err());
    assert!(translucency.set_front_face_alpha(-0.1).is_err());

    assert!(translucency.set_back_face_alpha(0.25).is_ok());
    assert_eq!(translucency.back_face_alpha(), 0.25);
    assert!(translucency.set_back_face_alpha(2.0).is_err());

    assert!(translucency
        .set_front_face_alpha_by_distance(Some(NearFarScalar::new(100.0, 0.2, 1000.0, 1.0)))
        .is_ok());
    // far < near must fail
    assert!(translucency
        .set_back_face_alpha_by_distance(Some(NearFarScalar::new(1000.0, 0.2, 100.0, 1.0)))
        .is_err());
}

#[test]
fn test_image_based_lighting_lifecycle_and_dirty_flags() {
    let mut ibl = ImageBasedLighting::new();
    assert!(ibl.is_enabled());
    assert_eq!(ibl.image_based_lighting_factor(), Cartesian2::new(1.0, 1.0));
    assert_eq!(ibl.luminance_at_zenith, 0.2);
    assert!(!ibl.should_regenerate_shaders());

    let mut frame_state = make_dummy_frame_state();
    ibl.update(&mut frame_state);
    assert!(!ibl.should_regenerate_shaders());

    // Disable factor -> triggers shader regeneration flag
    ibl.set_image_based_lighting_factor(Cartesian2::new(0.0, 0.0)).unwrap();
    assert!(!ibl.is_enabled());
    ibl.update(&mut frame_state);
    assert!(ibl.should_regenerate_shaders());

    // Supply spherical harmonics coefficients
    let sh = [Cartesian3::UNIT_X; 9];
    ibl.spherical_harmonic_coefficients = Some(sh);
    ibl.update(&mut frame_state);
    assert!(ibl.should_regenerate_shaders());

    ibl.destroy();
    assert!(ibl.is_destroyed());
}

#[test]
fn test_ion_imagery_provider_factory_resolution() {
    assert!(IonImageryProviderFactory::is_supported_external_type("ARCGIS_MAPSERVER"));
    assert!(IonImageryProviderFactory::is_supported_external_type("BING"));
    assert!(IonImageryProviderFactory::is_supported_external_type("GOOGLE_EARTH"));
    assert!(IonImageryProviderFactory::is_supported_external_type("MAPBOX"));
    assert!(IonImageryProviderFactory::is_supported_external_type("SINGLE_TILE"));
    assert!(IonImageryProviderFactory::is_supported_external_type("TMS"));
    assert!(IonImageryProviderFactory::is_supported_external_type("URL_TEMPLATE"));
    assert!(IonImageryProviderFactory::is_supported_external_type("WMS"));
    assert!(IonImageryProviderFactory::is_supported_external_type("WMTS"));
    assert!(IonImageryProviderFactory::is_supported_external_type("GOOGLE_2D_MAPS"));
    assert!(IonImageryProviderFactory::is_supported_external_type("AZURE_MAPS"));
    assert!(!IonImageryProviderFactory::is_supported_external_type("UNKNOWN_PROVIDER"));

    assert_eq!(
        IonImageryExternalType::from_str_value("BING").unwrap().as_str(),
        "BING"
    );

    let external_endpoint = json!({
        "type": "IMAGERY",
        "externalType": "ARCGIS_MAPSERVER",
        "options": {
            "url": "https://services.arcgisonline.com/ArcGIS/rest/services/World_Imagery/MapServer"
        }
    });
    assert_eq!(
        IonImageryProviderFactory::external_type(&external_endpoint),
        Some("ARCGIS_MAPSERVER")
    );
    assert_eq!(
        IonImageryProviderFactory::resolve_service_url(&external_endpoint),
        Some("https://services.arcgisonline.com/ArcGIS/rest/services/World_Imagery/MapServer".to_string())
    );

    let tms_endpoint = json!({
        "type": "IMAGERY",
        "url": "https://assets.ion.cesium.com/3812/tiles"
    });
    assert_eq!(
        IonImageryProviderFactory::resolve_service_url(&tms_endpoint),
        Some("https://assets.ion.cesium.com/3812/tiles".to_string())
    );
}

#[test]
fn test_moon_astronomical_position_and_ellipsoid() {
    let moon = Moon::new(MoonOptions::default());
    assert_eq!(moon.ellipsoid.radii, Ellipsoid::MOON.radii);
    assert_eq!(moon.texture_url, "Assets/Textures/moonSmall.jpg");
    assert!(moon.show);
    assert!(moon.only_sun_lighting);

    // Compute transform at J2000 epoch
    let j2000 = JulianDate::new(2451545.0, 0.0, TimeStandard::Tai);
    let transform = moon.compute_moon_transform(&j2000);
    let position = transform.get_translation();

    // The Moon's distance from Earth's center is roughly 384,400 km (between 3.5e8 and 4.1e8 m)
    let distance = position.magnitude();
    assert!(
        distance > 3.5e8 && distance < 4.1e8,
        "Moon distance {distance} should be roughly 384,400 km"
    );

    // Update with 3D FrameState
    let mut moon_mut = Moon::new(MoonOptions::default());
    let mut frame_state = make_dummy_frame_state();
    assert!(moon_mut.update(&mut frame_state).is_ok());

    assert!(moon_mut.destroy().is_ok());
    assert!(moon_mut.is_destroyed());
    assert!(moon_mut.update(&mut frame_state).is_err());
}

#[test]
fn regression_json_classification_and_shader_main() {
    use cesium_rs::scene::{Cesium3DTileContentFactory as Factory, Cesium3DTileContentType as Kind, DebugInspector};
    for json in [br#"{"type":"Feature"}"#.as_slice(), br#"{"type":"Point"}"#] {
        assert_eq!(Factory::classify(json).unwrap(), Kind::GeoJson);
    }
    assert_eq!(Factory::classify(br#"{"voxelTable":{}}"#).unwrap(), Kind::VoxelJson);
    let shader = DebugInspector::rewrite_fragment_shader_for_debug("void main(void) { out_FragColor = vec4(1.0); }");
    assert!(shader.contains("czm_Debug_main"));
    assert_eq!(shader.matches("void main(").count(), 1);
}
#[test]
fn equirectangular_implements_panorama() {
    use cesium_rs::scene::{EquirectangularPanorama, EquirectangularPanoramaOptions, panorama::Panorama};
    let mut panorama = EquirectangularPanorama::new(EquirectangularPanoramaOptions::new("image"));
    let value: &mut dyn Panorama = &mut panorama;
    value.set_show(false);
    assert!(!value.show());
}
