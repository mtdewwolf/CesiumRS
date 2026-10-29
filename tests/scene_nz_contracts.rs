use cesium_rs::{
    core::*,
    renderer::*,
    scene::{self, *},
};
use scene::{
    panorama::Panorama, panorama_provider::*, shadow_map_shader as shadow, sky_box::*,
    voxel_bounds_collection::VoxelBoundsCollection, voxel_shape::VoxelShape,
};
use serde_json::Value;
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};
fn fixture() -> Value {
    let raw: Value = serde_json::from_str(include_str!("fixtures/scene/NzContracts.json")).unwrap();
    let mut result = serde_json::json!({"shadows":[],"errors":[],"particles":[]});
    for c in raw["cases"].as_array().unwrap() {
        result[c["kind"].as_str().unwrap()].as_array_mut().unwrap().push(c.clone());
    }
    result
}

#[derive(Clone, Default)]
struct Host(Rc<RefCell<Vec<String>>>, Rc<Cell<u32>>);
impl Gl for Host {
    type Object = u32;
    fn drawing_buffer_size(&self) -> Result<[u32; 2], RuntimeError> {
        Ok([64, 64])
    }
    fn invoke(&self, name: &str, args: &[GlValue<u32>]) -> Result<GlValue<u32>, RuntimeError> {
        self.0.borrow_mut().push(name.into());
        Ok(if name.starts_with("create") {
            self.1.set(self.1.get() + 1);
            GlValue::Object(self.1.get())
        } else {
            match name {
                "getParameter" => {
                    if args.first().is_some_and(|v| matches!(v,GlValue::Number(n) if [33901.,33902.,3386.].contains(n)))
                    {
                        GlValue::Numbers(vec![1., 4096.])
                    } else {
                        GlValue::Number(4096.)
                    }
                }
                "getExtension" => GlValue::Object(0),
                "getShaderParameter" | "getProgramParameter" => GlValue::Bool(true),
                _ => GlValue::Null,
            }
        })
    }
}
fn frame() -> FrameState {
    let mut c = scene::camera::Camera::new(scene::camera::CameraOptions::new(64., 64.));
    let u = c.uniform_camera();
    let volume = c.frustum.compute_culling_volume(u.position, u.direction, u.up);
    FrameState::new(UniformFrame::new(u, JulianDate::new(2451545., 0., TimeStandard::Tai)), volume, [64, 64])
}
#[test]
fn shadow_cast_source_parity() {
    for c in fixture()["shadows"].as_array().unwrap() {
        let b = |n: &str| c[n].as_bool().unwrap();
        let input = ShaderSource {
            sources: vec![c["source"].as_str().unwrap().into()],
            defines: vec!["EXISTING".into()],
            ..Default::default()
        };
        assert_eq!(shadow::cast_keyword(b("point"), b("terrain"), b("depth"), b("opaque")), c["keyword"]);
        for (actual, key) in [
            (shadow::cast_vertex(&input, b("point"), b("terrain")), "vertex"),
            (shadow::cast_fragment(&input, b("point"), b("depth"), b("opaque")), "fragment"),
            (shadow::receive_vertex(&input, b("terrain"), true), "receive"),
        ] {
            assert_eq!(serde_json::json!(actual.sources), c[key]["sources"]);
            assert_eq!(serde_json::json!(actual.defines), c[key]["defines"]);
        }
        assert_eq!(input.defines, vec!["EXISTING"]);
    }
    assert_eq!(
        shadow::receive_keyword([false; 10]),
        "receiveShadow falsefalsefalsefalsefalsefalsefalsefalsefalsefalse"
    );
}
#[test]
fn particle_cpu_reference_trace() {
    struct Emitter;
    impl ParticleEmitter for Emitter {
        fn emit(&self, p: &mut Particle) {
            p.position = Cartesian3::ZERO;
            p.velocity = Cartesian3::UNIT_Z;
        }
    }
    let completed = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let mut s = ParticleSystem::default();
    s.emission_rate = 2.;
    s.particle_life = [1., 1.];
    s.lifetime = 2.;
    s.loop_animation = false;
    s.emitter = Some(Box::new(Emitter));
    s.bursts.push(ParticleBurst::new(scene::particle_burst::ParticleBurstOptions {
        time: Some(0.5),
        minimum: Some(2.),
        maximum: Some(2.),
    }));
    let count = completed.clone();
    s.complete.add_event_listener(move |_| {
        count.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    });
    for c in fixture()["particles"].as_array().unwrap() {
        s.update(c["time"].as_f64().unwrap(), None).unwrap();
        assert_eq!(s.current_time(), c["currentTime"]);
        assert_eq!(s.is_complete(), c["complete"]);
        assert_eq!(completed.load(std::sync::atomic::Ordering::Relaxed), c["completed"].as_u64().unwrap() as usize);
        assert_eq!(s.particles().len(), c["particles"].as_array().unwrap().len());
        for (p, e) in s.particles().iter().zip(c["particles"].as_array().unwrap()) {
            assert_eq!(p.age(), e["age"]);
            assert_eq!(p.position.z, e["z"]);
        }
    }
    s.destroy();
    assert!(s.update(4., None).is_err());
}
#[test]
fn particle_hidden_reverse_time_and_callback() {
    let mut s = ParticleSystem::default();
    s.update(0., None).unwrap();
    s.update(1., None).unwrap();
    assert_eq!(s.particles().len(), 5);
    s.show = false;
    s.update(2., None).unwrap();
    assert_eq!(s.current_time(), 1.);
    s.show = true;
    let mut count = 0;
    let mut f = |p: &mut Particle, dt: f64| {
        count += 1;
        p.mass += dt;
    };
    s.update(0.5, Some(&mut f)).unwrap();
    assert_eq!(count, 5);
    assert_eq!(s.current_time(), 1.);
    s.update(1.5, None).unwrap();
    assert_eq!(s.billboards()[0].scale, 1.);
}
#[test]
fn voxel_shape_dynamic_dispatch_matches_box() {
    let mut concrete = scene::voxel_box_shape::VoxelBoxShape::default();
    let s: &mut dyn VoxelShape = &mut concrete;
    assert!(s.update(Matrix4::IDENTITY, Cartesian3::new(-1., -1., -1.), Cartesian3::new(1., 1., 1.), None, None));
    assert_eq!(s.tile_obb(0, [0; 3]), *s.oriented_bounding_box());
    assert_eq!(s.convert_local_to_shape_uv_space(Cartesian3::ZERO), Cartesian3::new(0.5, 0.5, 0.5));
    assert!(s.update_view_transforms(Matrix3::IDENTITY).is_ok());
    assert_eq!(*s.shape_transform(), Matrix4::IDENTITY);
    assert!(s.bounding_sphere().radius > 0.);
}
#[test]
fn voxel_bounds_membership_packing_and_gpu_resize() {
    let host = Host::default();
    let context = Context::new_webgl2(host.clone());
    let mut c = VoxelBoundsCollection::<Host>::default();
    let a = ClippingPlane::handle(Cartesian3::UNIT_X, 2.);
    let b = ClippingPlane::handle(Cartesian3::UNIT_Y, 4.);
    c.add(a.clone());
    c.add(b.clone());
    assert_eq!(c.clipping_planes_state(), -2);
    c.union_clipping_regions = true;
    assert_eq!(c.clipping_planes_state(), 2);
    assert_eq!(c.packed_planes(Matrix4::IDENTITY), vec![1., 0., 0., 2., 0., 1., 0., 4.]);
    assert_eq!(c.texture_resolution(1), [1, 4]);
    c.update(&context, Matrix4::IDENTITY).unwrap();
    assert_eq!(c.texture().unwrap().dimensions(), [2, 2]);
    b.borrow_mut().set_distance(5.);
    c.update(&context, Matrix4::IDENTITY).unwrap();
    assert_eq!(c.packed_planes(Matrix4::IDENTITY)[7], 5.);
    let plane = a.borrow().as_plane();
    assert!(c.remove(&plane));
    assert_eq!(b.borrow().index, 0);
    assert_eq!(a.borrow().index, -1);
    c.remove_all();
    c.update(&context, Matrix4::IDENTITY).unwrap();
    assert!(c.texture().is_none());
    assert!(host.0.borrow().iter().any(|s| s == "texSubImage2D"));
    c.destroy().unwrap();
    assert!(c.update(&context, Matrix4::IDENTITY).is_err());
}
#[test]
fn viewport_quad_executes_and_rebuilds_on_material_identity() {
    let host = Host::default();
    let ctx = Context::new_webgl2(host.clone());
    let material = Rc::new(RefCell::new(scene::material::Material::from_type("Color")));
    let mut q = scene::viewport_quad::ViewportQuad::new(Viewport { x: 1, y: 2, width: 12, height: 8 }, material);
    assert!(q.update(&ctx, false).unwrap().is_none());
    let c = q.update(&ctx, true).unwrap().unwrap();
    assert_eq!(c.options.pass, Some(Pass::Overlay));
    assert!(c.options.render_state.blending.enabled);
    c.execute(&ctx, &PassState::default()).unwrap();
    q.rectangle.width = 20;
    assert_eq!(q.update(&ctx, true).unwrap().unwrap().options.render_state.viewport.unwrap().width, 20);
    let mut other = scene::viewport_quad::ViewportQuad::new(
        Viewport { x: 0, y: 0, width: 8, height: 8 },
        Rc::new(RefCell::new(scene::material::Material::from_type("Color"))),
    );
    other.update(&ctx, true).unwrap().unwrap().execute(&ctx, &PassState::default()).unwrap();
    q.material = Rc::new(RefCell::new(scene::material::Material::from_type("Color")));
    q.update(&ctx, true).unwrap().unwrap().execute(&ctx, &PassState::default()).unwrap();
    assert!(host.0.borrow().iter().any(|s| s == "drawElements"));
    assert!(!host.0.borrow().iter().any(|s| s == "deleteProgram"));
    q.destroy().unwrap();
    other.update(&ctx, true).unwrap().unwrap().execute(&ctx, &PassState::default()).unwrap();
    assert!(!host.0.borrow().iter().any(|s| s == "deleteProgram"));
    assert!(q.update(&ctx, true).is_err());
}
struct TestPanorama {
    show: bool,
    matrix: Matrix4,
    sources: [String; 6],
    calls: usize,
    destroyed: bool,
}
impl Panorama for TestPanorama {
    fn show(&self) -> bool {
        self.show
    }
    fn set_show(&mut self, v: bool) {
        self.show = v;
    }
    fn transform(&self) -> &Matrix4 {
        &self.matrix
    }
    fn credit(&self) -> Option<&Credit> {
        None
    }
}
impl SkyBoxPanorama for TestPanorama {
    type Command = bool;
    type Sources = [String; 6];
    fn sources(&self) -> &Self::Sources {
        &self.sources
    }
    fn set_sources(&mut self, v: Self::Sources) {
        self.sources = v;
    }
    fn update(&mut self, _: &FrameState, hdr: bool) -> Result<Option<bool>, RuntimeError> {
        self.calls += 1;
        Ok(Some(hdr))
    }
    fn destroy(&mut self) -> Result<(), RuntimeError> {
        self.destroyed = true;
        Ok(())
    }
}
struct Provider;
impl PanoramaProvider for Provider {
    type Options = ();
    type LoadOptions = ();
    type Panorama = TestPanorama;
    fn from_url(_: ()) -> PanoramaFuture<'static, Self> {
        Box::pin(async { Ok(Self) })
    }
    fn load_panorama(&mut self, _: ()) -> PanoramaFuture<'_, TestPanorama> {
        Box::pin(async {
            Ok(TestPanorama {
                show: true,
                matrix: Matrix4::IDENTITY,
                sources: SkyBox::<TestPanorama>::earth_sources("/"),
                calls: 0,
                destroyed: false,
            })
        })
    }
}
fn ready<T>(mut f: std::pin::Pin<Box<dyn std::future::Future<Output = T> + '_>>) -> T {
    let mut cx = std::task::Context::from_waker(std::task::Waker::noop());
    match f.as_mut().poll(&mut cx) {
        std::task::Poll::Ready(v) => v,
        _ => panic!("not ready"),
    }
}
#[test]
fn panorama_factory_and_skybox_mode_filtering() {
    let mut provider = ready(Provider::from_url(())).unwrap();
    let mut p = ready(provider.load_panorama(())).unwrap();
    p.set_show(false);
    assert!(!p.show());
    assert_eq!(*p.transform(), Matrix4::IDENTITY);
    assert!(p.credit().is_none());
    let mut sky = SkyBox::new(p);
    assert!(sky.sources()[1].ends_with("_mx.jpg"));
    let mut f = frame();
    f.passes.render = true;
    for mode in [SceneMode::Scene2D, SceneMode::ColumbusView] {
        f.mode = mode;
        assert_eq!(sky.update(&f, true).unwrap(), None);
    }
    f.mode = SceneMode::Scene3D;
    assert_eq!(sky.update(&f, true).unwrap(), Some(true));
    f.passes.render = false;
    assert_eq!(sky.update(&f, true).unwrap(), None);
    sky.destroy().unwrap();
    assert!(sky.is_destroyed());
    assert!(sky.update(&f, true).is_err());
}
#[test]
fn quadtree_geometric_error_reference() {
    for c in fixture()["errors"].as_array().unwrap() {
        let r = c["radius"].as_f64().unwrap();
        let scheme = GeographicTilingScheme::new(
            Ellipsoid::new(r, r, r),
            Rectangle::MAX_VALUE,
            c["tiles"].as_i64().unwrap() as i32,
            1,
        );
        let actual = scene::quadtree_tile_provider::compute_default_level_zero_maximum_geometric_error(&scheme);
        assert!((actual - c["error"].as_f64().unwrap()).abs() < 1e-10);
    }
}
struct Voxels {
    matrix: Matrix4,
    names: Vec<String>,
    defer: bool,
}
impl scene::voxel_provider::VoxelProvider for Voxels {
    fn global_transform(&self) -> &Matrix4 {
        &self.matrix
    }
    fn shape_transform(&self) -> &Matrix4 {
        &self.matrix
    }
    fn shape(&self) -> VoxelShapeType {
        VoxelShapeType::Box
    }
    fn min_bounds(&self) -> Option<Cartesian3> {
        None
    }
    fn max_bounds(&self) -> Option<Cartesian3> {
        None
    }
    fn dimensions(&self) -> Cartesian3 {
        Cartesian3::new(1., 1., 1.)
    }
    fn padding_before(&self) -> Cartesian3 {
        Cartesian3::ZERO
    }
    fn padding_after(&self) -> Cartesian3 {
        Cartesian3::ZERO
    }
    fn names(&self) -> &[String] {
        &self.names
    }
    fn types(&self) -> &[MetadataType] {
        &[MetadataType::Scalar]
    }
    fn component_types(&self) -> &[MetadataComponentType] {
        &[MetadataComponentType::Float32]
    }
    fn minimum_values(&self) -> Option<&[Vec<f64>]> {
        None
    }
    fn maximum_values(&self) -> Option<&[Vec<f64>]> {
        None
    }
    fn maximum_tile_count(&self) -> Option<usize> {
        Some(1)
    }
    fn available_levels(&self) -> Option<u32> {
        Some(1)
    }
    fn keyframe_count(&self) -> Option<u32> {
        Some(1)
    }
    fn time_interval_collection(&self) -> Option<&TimeIntervalCollection<u32>> {
        None
    }
    fn request_data(
        &mut self,
        request: scene::voxel_provider::VoxelRequest,
    ) -> Option<scene::voxel_provider::VoxelFuture<'_>> {
        if self.defer {
            return None;
        }
        Some(Box::pin(async move {
            if request.tile_level != 0 {
                return Err(RuntimeError::new("tile unavailable"));
            }
            Ok(scene::voxel_content::VoxelContent::from_metadata_array(vec![TypedArray::Float32(vec![42.])]))
        }))
    }
}
#[test]
fn voxel_provider_deferral_success_and_error() {
    use scene::voxel_provider::*;
    let mut p = Voxels { matrix: Matrix4::IDENTITY, names: vec!["density".into()], defer: true };
    let p: &mut dyn VoxelProvider = &mut p;
    assert!(p.request_data(VoxelRequest::default()).is_none());
    let mut p = Voxels { matrix: Matrix4::IDENTITY, names: vec!["density".into()], defer: false };
    let content = ready(p.request_data(VoxelRequest::default()).unwrap()).unwrap();
    assert_eq!(content.metadata(), &[TypedArray::Float32(vec![42.])]);
    assert!(ready(p.request_data(VoxelRequest { tile_level: 1, ..Default::default() }).unwrap()).is_err());
    assert_eq!(p.shape().min_bounds(), Cartesian3::new(-1., -1., -1.));
}
