use cesium_rs::renderer::ShaderSource;
use cesium_rs::scene::shadow_map_shader::{receive_fragment, ReceiveBias, ReceiveOptions};

#[test]
fn shadow_receive_matches_all_pinned_variants() {
    let cases: serde_json::Value =
        serde_json::from_str(include_str!("data/model-effects/shadow-receive.json")).unwrap();
    for case in cases.as_array().unwrap() {
        let f: Vec<bool> = case["f"].as_array().unwrap().iter().map(|v| v.as_bool().unwrap()).collect();
        let variant = case["variant"].as_u64().unwrap();
        let source = format!(
            "{}{}void main() {{ out_FragColor = vec4(1.0); }}",
            if variant & 1 != 0 { "in vec3 v_positionEC;" } else { "" },
            if variant & 2 != 0 { "in vec3 v_normalEC;" } else { "" }
        );
        let options = ReceiveOptions {
            uses_depth_texture: f[0],
            polygon_offset_supported: f[1],
            point_light: f[2],
            spot_light: f[3],
            cascades: f[4],
            debug_cascade_colors: f[5],
            soft_shadows: f[6],
            cast_shadows: true,
            terrain: variant == 3,
            terrain_normal: variant & 2 != 0,
            bias: ReceiveBias {
                normal_shading: f[7],
                normal_shading_smooth: if f[8] { 0.1 } else { 0.0 },
                normal_offset: f[9],
            },
        };
        let result = receive_fragment(&ShaderSource::new(source), &options);
        assert_eq!(serde_json::to_value(&result.defines).unwrap(), case["defines"], "{case}");
        let encoded = serde_json::to_string(&result.sources).unwrap();
        let hash = encoded.bytes().fold(0xcbf29ce484222325u64, |h, b| (h ^ u64::from(b)).wrapping_mul(0x100000001b3));
        assert_eq!(format!("{hash:x}"), case["hash"].as_str().unwrap(), "{case}");
    }
}

fn poll_ready<T>(future: &mut (impl std::future::Future<Output = T> + Unpin)) -> T {
    let waker = std::task::Waker::noop();
    let mut cx = std::task::Context::from_waker(waker);
    match std::pin::Pin::new(future).poll(&mut cx) {
        std::task::Poll::Ready(v) => v,
        _ => panic!("native worker should finish on poll"),
    }
}

#[test]
fn native_gaussian_coordinators_execute_real_algorithms() {
    use cesium_rs::scene::{
        gaussian_splat_sorter::GaussianSplatSorter, gaussian_splat_texture_generator::GaussianSplatTextureGenerator,
    };
    let sorter = GaussianSplatSorter::native(2);
    let generator = GaussianSplatTextureGenerator::native(2);
    let mut cx = std::task::Context::from_waker(std::task::Waker::noop());
    assert!(matches!(sorter.poll_initialization(&mut cx), std::task::Poll::Ready(Ok(true))));
    assert!(matches!(generator.poll_initialization(&mut cx), std::task::Poll::Ready(Ok(true))));
    let mut task = sorter.radix_sort_indexes(serde_json::json!({"primitive":{"positions":[0,0,3,0,0,1,0,0,2],"modelView":[1,0,0,0,0,1,0,0,0,0,1,0,0,0,0,1],"count":3},"sortType":"Index"})).unwrap().unwrap();
    assert_eq!(poll_ready(&mut task).unwrap(), serde_json::json!([1, 2, 0]));
    let mut task = generator.generate_from_attributes(serde_json::json!({"attributes":{"positions":[1,2,3],"scales":[1,1,1],"rotations":[0,0,0,1],"colors":[255,0,128,255]},"count":1})).unwrap().unwrap();
    let result = poll_ready(&mut task).unwrap();
    assert_eq!(result["width"], 2048);
    assert_eq!(result["height"], 1);
    assert_eq!(result["data"][0], 1f32.to_bits());
    assert_eq!(result["data"][7], u32::from_le_bytes([255, 0, 128, 255]));
}

#[test]
fn ibl_tracks_frame_environment_and_loaded_resources() {
    use cesium_rs::{
        core::Cartesian2,
        scene::{frame_state::FrameResource, image_based_lighting::ImageBasedLighting, FrameState},
    };
    let mut camera = cesium_rs::scene::Camera::new(cesium_rs::scene::CameraOptions::new(8., 8.));
    let uniform = camera.uniform_camera();
    let culling = camera.frustum.compute_culling_volume(uniform.position, uniform.direction, uniform.up);
    let uniform = cesium_rs::renderer::UniformFrame::new(
        uniform,
        cesium_rs::core::JulianDate::new(2451545., 0., cesium_rs::core::TimeStandard::Tai),
    );
    let mut frame = FrameState::new(uniform, culling, [8, 8]);
    let mut ibl = ImageBasedLighting::new();
    frame.spherical_harmonic_coefficients = Some(vec![]);
    frame.specular_environment_maps = Some(FrameResource::new("default atlas"));
    ibl.update(&mut frame);
    assert!(ibl.should_regenerate_shaders());
    assert!(ibl.use_default_spherical_harmonics());
    assert!(ibl.use_default_specular_maps());
    ibl.update(&mut frame);
    assert!(ibl.should_regenerate_shaders());
    frame.frame_number += 1;
    ibl.update(&mut frame);
    assert!(!ibl.should_regenerate_shaders());
    ibl.specular_environment_maps = Some("custom.ktx2".into());
    frame.frame_number += 1;
    ibl.update(&mut frame);
    assert!(!ibl.use_specular_environment_maps(), "a URL is not an uploaded texture");
    ibl.set_specular_environment_map("custom.ktx2".into(), FrameResource::new("uploaded atlas"));
    frame.frame_number += 1;
    ibl.update(&mut frame);
    assert!(ibl.use_specular_environment_maps());
    assert!(ibl.should_regenerate_shaders());
    ibl.specular_environment_maps = Some("replacement.ktx2".into());
    frame.frame_number += 1;
    ibl.update(&mut frame);
    assert!(ibl.specular_environment_map().is_none());
    assert!(ibl.should_regenerate_shaders());
    ibl.set_image_based_lighting_factor(Cartesian2::new(0., 0.)).unwrap();
    frame.frame_number += 1;
    ibl.update(&mut frame);
    assert!(!ibl.is_enabled());
    assert!(ibl.should_regenerate_shaders());
    ibl.destroy();
    assert!(ibl.is_destroyed());
}

#[test]
fn classification_shader_dependencies_and_projection_match_pinned_engine() {
    use cesium_rs::{
        core::GeographicProjection,
        scene::{PerInstanceColorAppearance, PerInstanceColorAppearanceOptions, ShadowVolumeAppearance},
    };
    let cases: serde_json::Value = serde_json::from_str(include_str!("data/model-effects/shadow-volume.json")).unwrap();
    for case in cases.as_array().unwrap() {
        let mask = case["mask"].as_u64().unwrap();
        let appearance = ShadowVolumeAppearance::new(
            mask & 1 != 0,
            mask & 2 != 0,
            Box::new(PerInstanceColorAppearance::new(PerInstanceColorAppearanceOptions {
                flat: Some(mask & 4 != 0),
                ..Default::default()
            })),
        );
        let cv = mask & 8 != 0;
        for (key, source) in
            [("color", appearance.create_fragment_shader(cv)), ("pick", appearance.create_pick_fragment_shader(cv))]
        {
            let defines: Vec<_> = source
                .lines()
                .take_while(|l| l.starts_with("#define "))
                .map(|l| l.trim_start_matches("#define "))
                .collect();
            assert_eq!(serde_json::json!(defines), case[key], "{case}");
        }
        let projection = GeographicProjection::default();
        let vs = appearance.create_vertex_shader(&["EXTERNAL".into()], "void main() {}", cv, &projection);
        let pick = appearance.create_pick_vertex_shader(&["EXTERNAL".into()], "void main() {}", cv, &projection);
        assert_eq!(serde_json::json!(vs.defines), case["vertex"], "{case}");
        assert_eq!(serde_json::json!(pick.defines), case["pickVertex"], "{case}");
    }
}

#[test]
fn gaussian_initialization_false_is_a_persistent_error() {
    use cesium_rs::{
        core::{task_processor::*, RuntimeError},
        scene::{
            gaussian_splat_sorter::GaussianSplatSorter, gaussian_splat_texture_generator::GaussianSplatTextureGenerator,
        },
    };
    use serde_json::Value;
    struct Reject;
    impl TaskWorker<Value, Value> for Reject {
        fn run(&self, _: u64, _: Value) -> TaskFuture<Value> {
            panic!("failed initialization must never schedule work")
        }
        fn init_web_assembly_module(&self, _: WebAssemblyOptions) -> TaskFuture<Value> {
            Box::pin(async { Ok(Value::Bool(false)) })
        }
        fn cancel_task(&self, _: u64) {}
        fn terminate(&self) {}
    }
    let worker = std::rc::Rc::new(Reject);
    let sorter = GaussianSplatSorter::new(Some(worker.clone()), 4);
    let generator = GaussianSplatTextureGenerator::new(Some(worker), 4);
    let mut cx = std::task::Context::from_waker(std::task::Waker::noop());
    let is_error = |v: std::task::Poll<Result<bool, RuntimeError>>| matches!(v, std::task::Poll::Ready(Err(_)));
    assert!(is_error(sorter.poll_initialization(&mut cx)));
    assert!(is_error(generator.poll_initialization(&mut cx)));
    assert!(sorter.radix_sort_indexes(Value::Null).is_err());
    assert!(generator.generate_from_attributes(Value::Null).is_err());
}
