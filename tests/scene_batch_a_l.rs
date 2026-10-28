#![recursion_limit = "256"]
use cesium_rs::core::{
    Cartesian3, Cartographic, Ellipsoid, Geometry, GeometryAttributes, JulianDate, PrimitiveType,
    RuntimeError, TimeStandard,
};
use cesium_rs::renderer::{Gl, GlValue, UniformFrame};
use cesium_rs::scene::camera::{Camera, CameraOptions};
use cesium_rs::scene::frame_state::FrameState;
use cesium_rs::scene::{
    compute_depth_quad, create_tangent_space_debug_primitive, BrdfLutGenerator,
    CreateTangentSpaceDebugPrimitiveOptions, DebugCameraPrimitive, DebugCameraPrimitiveOptions,
    DebugModelMatrixPrimitive, DebugModelMatrixPrimitiveOptions, DepthPlane, Empty3DTileContent,
    GaussianSplatRenderResources, GaussianSplatSorter, GaussianSplatTextureGenerator,
    JobScheduler, JobType,
};

#[derive(Default)]
struct TestGl;
impl Gl for TestGl {
    type Object = u32;
    fn drawing_buffer_size(&self) -> Result<[u32; 2], RuntimeError> {
        Ok([256, 256])
    }
    fn invoke(&self, method: &str, _: &[GlValue<u32>]) -> Result<GlValue<u32>, RuntimeError> {
        if method.starts_with("create") {
            Ok(GlValue::Object(1))
        } else {
            Ok(GlValue::Null)
        }
    }
}

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
fn test_job_scheduler_priorities_and_execution() {
    let mut scheduler = JobScheduler::default();
    assert_eq!(scheduler.total_budget(), 50.0);
    assert_eq!(scheduler.total_used_this_frame(), 0.0);

    let mut executed = 0;
    let did_run = scheduler.execute(|| executed += 1, JobType::Program);
    assert!(did_run);
    assert_eq!(executed, 1);

    scheduler.disable_this_frame();
    let did_not_run = scheduler.execute(|| executed += 1, JobType::Program);
    assert!(!did_not_run);
    assert_eq!(executed, 1);

    scheduler.reset_budgets();
    assert_eq!(scheduler.total_used_this_frame(), 0.0);
}

#[test]
fn test_empty_3d_tile_content() {
    let content = Empty3DTileContent::new();
    assert_eq!(content.features_length(), 0);
    assert_eq!(content.points_length(), 0);
    assert_eq!(content.triangles_length(), 0);
    assert_eq!(content.geometry_byte_length(), 0);
    assert_eq!(content.textures_byte_length(), 0);
    assert!(!content.has_property(0, "height"));
    assert!(content.get_feature(0).is_none());
    assert!(content.ready());
    assert!(!content.is_destroyed());
}

#[test]
fn test_gaussian_splat_sorter() {
    let sorter = GaussianSplatSorter::default();
    assert_eq!(GaussianSplatSorter::max_sorting_concurrency(4), 3);
    assert_eq!(GaussianSplatSorter::WORKER_NAME, "gaussianSplatSorter");
    assert_eq!(
        GaussianSplatSorter::WASM_BINARY_FILE,
        "ThirdParty/wasm_splats_bg.wasm"
    );
    assert!(!sorter.is_ready());
}

#[test]
fn test_gaussian_splat_render_resources() {
    let resources = GaussianSplatRenderResources::new(true);
    assert!(resources.render_state.depth_test.enabled);
    assert!(!resources.has_skip_level_of_detail);
}

#[test]
fn test_gaussian_splat_texture_generator() {
    let generator = GaussianSplatTextureGenerator::default();
    assert_eq!(GaussianSplatTextureGenerator::max_sorting_concurrency(4), 3);
    assert_eq!(
        GaussianSplatTextureGenerator::WORKER_NAME,
        "gaussianSplatTextureGenerator"
    );
    assert_eq!(
        GaussianSplatTextureGenerator::WASM_BINARY_FILE,
        "ThirdParty/wasm_splats_bg.wasm"
    );
    assert!(!generator.is_ready());
}

#[test]
fn test_brdf_lut_generator_lifecycle() {
    let mut generator = BrdfLutGenerator::<TestGl>::new();
    assert!(generator.color_texture().is_none());
    assert_eq!(BrdfLutGenerator::<TestGl>::WIDTH, 256);
    assert_eq!(BrdfLutGenerator::<TestGl>::HEIGHT, 256);
    assert!(!generator.is_destroyed());

    generator.destroy();
    assert!(generator.is_destroyed());
}

#[test]
fn test_depth_plane_computations() {
    let ellipsoid = Ellipsoid::WGS84;
    let mut camera = Camera::new(CameraOptions::new(1024.0, 768.0));
    let pos = ellipsoid
        .cartographic_to_cartesian(&Cartographic::from_degrees(0.0, 0.0, 10_000_000.0));
    camera.position = pos;
    camera.direction = Cartesian3::new(-1.0, 0.0, 0.0);
    camera.up = Cartesian3::UNIT_Z;
    camera.right = Cartesian3::UNIT_Y;

    let quad = compute_depth_quad(&ellipsoid, &mut camera);
    assert_eq!(quad.len(), 12);
    for val in quad {
        assert!(val.is_finite());
    }

    let mut dp = DepthPlane::new(100.0);
    assert_eq!(dp.ellipsoid_offset, 100.0);
    assert!(!dp.is_destroyed());
    dp.destroy();
    assert!(dp.is_destroyed());
}

#[test]
fn test_debug_model_matrix_primitive() {
    let options = DebugModelMatrixPrimitiveOptions::default();
    let mut primitive = DebugModelMatrixPrimitive::new(options);
    assert_eq!(primitive.length, 10_000_000.0);
    assert!(primitive.show);
    assert!(!primitive.is_destroyed());

    let mut frame_state = make_dummy_frame_state();
    let _ = primitive.update(&mut frame_state);

    primitive.destroy();
    assert!(primitive.is_destroyed());
}

#[test]
fn test_debug_camera_primitive() {
    let options = DebugCameraPrimitiveOptions::default();
    let mut primitive = DebugCameraPrimitive::new(options);
    assert!(primitive.show);
    assert!(!primitive.is_destroyed());

    let mut camera = Camera::new(CameraOptions::new(800.0, 600.0));
    let mut frame_state = make_dummy_frame_state();
    let _ = primitive.update(&mut camera, &mut frame_state);

    primitive.destroy();
    assert!(primitive.is_destroyed());
}

#[test]
fn test_create_tangent_space_debug_primitive() {
    let options = CreateTangentSpaceDebugPrimitiveOptions::new(Geometry::new(
        GeometryAttributes::default(),
        None,
        PrimitiveType::Triangles,
    ));
    let primitive = create_tangent_space_debug_primitive(options);
    assert!(primitive.is_none());
}
