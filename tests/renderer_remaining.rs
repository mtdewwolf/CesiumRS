//! Remaining Renderer resource and host-integration contracts.
mod common;
use cesium_rs::{
    core::{webgl_constants as gl, PixelFormat, RuntimeError},
    renderer::*,
};
use futures_util::FutureExt;
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
};
#[test]
fn atlas_requests_pack_resize_deduplicate_subregions_and_cancel() {
    let host = Host::default();
    let ctx = Context::new_webgl2(host.clone());
    let mut atlas = TextureAtlas::new(TextureAtlasOptions { initial_size: [4, 4], ..Default::default() }).unwrap();
    let source =
        |width, height| TextureSource::Pixels { width, height, bytes: vec![255; (width * height * 4) as usize] };
    let a = atlas.add_image("a", source(2, 2)).unwrap();
    let duplicate = atlas.add_image("a", source(1, 1)).unwrap();
    assert!(atlas.update(&ctx).unwrap());
    assert_eq!(a.clone().now_or_never().unwrap().unwrap(), 0);
    assert_eq!(duplicate.now_or_never().unwrap().unwrap(), 0);
    let old = atlas.texture().unwrap().clone();
    let guid = atlas.guid().to_owned();
    let b = atlas.add_image("b", source(5, 3)).unwrap();
    let sub = atlas.add_image_sub_region("a", cesium_rs::core::BoundingRectangle::new(1., 0., 1., 2.)).unwrap();
    assert!(atlas.update(&ctx).unwrap());
    assert_eq!(b.now_or_never().unwrap().unwrap(), 1);
    assert_eq!(sub.now_or_never().unwrap().unwrap(), 2);
    assert!(old.is_destroyed());
    assert_ne!(atlas.guid(), guid);
    let region = atlas.compute_texture_coordinates(2).unwrap();
    let parent = atlas.compute_texture_coordinates(0).unwrap();
    assert_eq!(region.x, parent.x + 1. / atlas.texture().unwrap().width() as f64);
    let pending = atlas.add_image_future("pending", std::future::pending()).unwrap();
    assert!(!atlas.update(&ctx).unwrap());
    atlas.destroy().unwrap();
    assert_eq!(pending.now_or_never().unwrap().unwrap(), -1);
}
#[test]
fn facade_writers_align_preserve_resize_and_subcommit() {
    use cesium_rs::core::ComponentDatatype as D;
    let host = Host::default();
    let ctx = Context::new_webgl2(host.clone());
    let mut a = FacadeAttribute::new(0, 3);
    a.datatype = D::UnsignedByte;
    let b = FacadeAttribute::new(1, 3);
    let mut c = FacadeAttribute::new(2, 2);
    c.datatype = D::UnsignedShort;
    let mut facade = VertexArrayFacade::new(&ctx, vec![a, b, c], 2, false).unwrap();
    let writer = facade.writer(1).unwrap();
    facade.write_with(writer, 0, &[1., 2., 3.]).unwrap();
    facade.write(0, 0, &[255., 128., 64.]).unwrap();
    facade.write(2, 0, &[1000., 2000.]).unwrap();
    facade.commit(None).unwrap();
    let va = facade.vertex_arrays().unwrap()[0].vertex_array.clone();
    assert_eq!(
        va.attributes().iter().map(|a| (a.index, a.offset, a.stride)).collect::<Vec<_>>(),
        vec![(1, 0, 20), (2, 12, 20), (0, 16, 20)]
    );
    facade.write_with(writer, 1, &[4., 5., 6.]).unwrap();
    facade.sub_commit(1, 1).unwrap();
    facade.end_sub_commits();
    let sub = host.count("bufferSubData");
    facade.commit(None).unwrap();
    assert_eq!(host.count("bufferSubData"), sub);
    facade.resize(4).unwrap();
    assert!(va.is_destroyed());
    assert!(facade.write_with(writer, 1, &[1., 2., 3.]).is_err());
    facade.write(1, 3, &[7., 8., 9.]).unwrap();
    facade.commit(None).unwrap();
    assert_eq!(&facade.cpu_buffers()[0][0..4], &1f32.to_le_bytes());
    facade.destroy().unwrap();
    assert!(facade.is_destroyed());
}
#[derive(Default)]
struct Calls {
    next: Cell<u32>,
    calls: RefCell<Vec<(String, Vec<GlValue<u32>>)>>,
    fail: RefCell<Option<String>>,
    dimensions: Cell<Option<[u32; 2]>>,
}
#[derive(Clone, Default)]
struct Host(Rc<Calls>);
impl Gl for Host {
    type Object = u32;
    fn drawing_buffer_size(&self) -> Result<[u32; 2], RuntimeError> {
        Ok(self.0.dimensions.get().unwrap_or([8, 8]))
    }
    fn invoke(&self, name: &str, args: &[GlValue<u32>]) -> Result<GlValue<u32>, RuntimeError> {
        self.0.calls.borrow_mut().push((name.into(), args.to_vec()));
        if self.0.fail.borrow().as_deref() == Some(name) {
            return Err(RuntimeError::new("injected host failure"));
        }
        if name.starts_with("create") {
            let n = self.0.next.get() + 1;
            self.0.next.set(n);
            return Ok(GlValue::Object(n));
        }
        Ok(match name {
            "getParameter" => GlValue::Number(16384.),
            "getExtension" | "getUniformLocation" => GlValue::Object(0),
            "getShaderPrecisionFormat" => GlValue::Record(serde_json::json!({"precision":23,"rangeMax":127})),
            "getShaderParameter" => GlValue::Bool(true),
            "getProgramParameter" => {
                if matches!(args.get(1),Some(GlValue::Number(n)) if *n==gl::ACTIVE_ATTRIBUTES as f64||*n==gl::ACTIVE_UNIFORMS as f64)
                {
                    GlValue::Number(0.)
                } else {
                    GlValue::Bool(true)
                }
            }
            _ => GlValue::Null,
        })
    }
}
impl Host {
    fn count(&self, name: &str) -> usize {
        self.0.calls.borrow().iter().filter(|(n, _)| n == name).count()
    }
}
#[test]
fn cube_faces_share_resource_and_explicit_destruction_invalidates_aliases() {
    let host = Host::default();
    let ctx = Context::new_webgl2(host.clone());
    let cube = CubeMap::new(&ctx, TextureOptions::rgba(2, 2), None).unwrap();
    assert_eq!(host.count("texImage2D"), 6);
    assert_eq!(cube.size_in_bytes(), 96);
    let face = cube.face(CubeFace::NegativeZ).clone();
    face.copy_from(&TextureSource::Pixels { width: 1, height: 1, bytes: vec![1, 2, 3, 4] }, 1, 0, false).unwrap();
    assert_eq!(host.count("texSubImage2D"), 1);
    cube.generate_mipmap(MipmapHint::DontCare).unwrap();
    assert_eq!(cube.size_in_bytes(), 128);
    cube.destroy().unwrap();
    assert!(face.is_destroyed());
    assert_eq!(host.count("deleteTexture"), 1);
    assert!(face
        .copy_from(&TextureSource::Pixels { width: 1, height: 1, bytes: vec![1, 2, 3, 4] }, 0, 0, false)
        .is_err());
    drop(cube);
    drop(face);
    assert_eq!(host.count("deleteTexture"), 1);
}
#[test]
fn volume_uploads_validate_ranges_and_retain_mip_storage() {
    let host = Host::default();
    let ctx = Context::new_webgl2(host.clone());
    let t = Texture3D::new(&ctx, TextureOptions::rgba(2, 2), 2, Some(&[0; 32])).unwrap();
    assert_eq!(host.count("texStorage3D"), 1);
    assert_eq!(t.size_in_bytes(), 36);
    let before = host.0.calls.borrow().len();
    assert!(t.copy_from(TextureVolume { x: 2, y: 0, z: 0, width: 1, height: 1, depth: 1 }, &[1, 2, 3, 4]).is_err());
    assert_eq!(host.0.calls.borrow().len(), before);
    t.copy_from(TextureVolume { x: 1, y: 1, z: 1, width: 1, height: 1, depth: 1 }, &[1, 2, 3, 4]).unwrap();
    t.generate_mipmap(MipmapHint::DontCare).unwrap();
    assert_eq!(t.size_in_bytes(), 36);
    let storage = host.0.calls.borrow();
    let args = &storage.iter().find(|(n, _)| n == "texStorage3D").unwrap().1;
    assert!(matches!(args[1],GlValue::Number(n) if n==2.));
}
#[test]
fn uniform_factories_validate_shapes_targets_and_share_value_caches() {
    let host = Host::default();
    let ctx = Context::new_webgl2(host.clone());
    let program = ShaderProgram::new(&ctx, "vs", "fs", &[]).unwrap();
    let float = create_uniform(&program, "value", gl::FLOAT_VEC3).unwrap();
    float.set(&UniformValue::Float(vec![1., 2., 3.])).unwrap();
    float.set(&UniformValue::Float(vec![1., 2., 3.])).unwrap();
    assert_eq!(host.count("uniform3fv"), 1);
    assert!(float.set(&UniformValue::Float(vec![1., 2.])).is_err());
    assert!(create_uniform(&program, "bad", 0).is_err());
    let array = create_uniform_array(&program, "values", gl::FLOAT_MAT3, 2).unwrap();
    array.set(&UniformValue::MatrixArray { columns: 3, rows: 3, values: vec![1.; 18] }).unwrap();
    assert_eq!(host.count("uniformMatrix3fv"), 1);
    let mut sampler = create_uniform_array(&program, "maps", gl::SAMPLER_CUBE, 2).unwrap();
    assert_eq!(sampler.set_sampler(3).unwrap(), 5);
    let cube = Rc::new(CubeMap::new(&ctx, TextureOptions::rgba(1, 1), None).unwrap());
    sampler.set_textures(vec![TextureBinding::CubeMap(cube.clone()), TextureBinding::CubeMap(cube)]).unwrap();
    let texture = Rc::new(Texture::new(&ctx, TextureOptions::rgba(1, 1), None).unwrap());
    assert!(sampler
        .set_textures(vec![TextureBinding::Texture(texture.clone()), TextureBinding::Texture(texture)])
        .is_err());
}
#[test]
fn texture_allocation_failures_cleanup_and_reject_invalid_sources() {
    let host = Host::default();
    let ctx = Context::new_webgl2(host.clone());
    assert!(CubeMap::new(&ctx, TextureOptions::rgba(2, 1), None).is_err());
    let mut bad = TextureOptions::rgba(2, 2);
    bad.pixel_format = PixelFormat::RgbDxt1;
    assert!(Texture3D::new(&ctx, bad, 2, None).is_err());
    assert_eq!(host.count("createTexture"), 0);
    *host.0.fail.borrow_mut() = Some("texImage2D".into());
    assert!(CubeMap::new(&ctx, TextureOptions::rgba(1, 1), None).is_err());
    assert_eq!(host.count("deleteTexture"), 1);
    *host.0.fail.borrow_mut() = Some("texStorage3D".into());
    assert!(Texture3D::new(&ctx, TextureOptions::rgba(1, 1), 1, Some(&[0; 4])).is_err());
    assert_eq!(host.count("deleteTexture"), 2);
}

#[test]
fn context_defaults_picks_cleanup_and_failed_destroy_retry() {
    let host = Host::default();
    let ctx = Context::new_webgl2(host.clone());
    let texture = ctx.default_texture().unwrap();
    assert!(Rc::ptr_eq(&texture, &ctx.default_texture().unwrap()));
    assert!(Rc::ptr_eq(&ctx.default_cube_map().unwrap(), &ctx.default_cube_map().unwrap()));
    let first = ctx.create_pick_id(Rc::new(String::from("first"))).unwrap();
    let second = ctx.create_pick_id(Rc::new(42u32)).unwrap();
    assert_eq!(first.key, 1);
    assert_eq!(second.key, 2);
    assert_eq!(&*ctx.pick_object::<String>(first.key).unwrap(), "first");
    first.set_object(Rc::new(String::from("updated"))).unwrap();
    assert_eq!(&*first.object_as::<String>().unwrap(), "updated");
    first.destroy();
    assert!(ctx.get_object_by_pick_color(first.key).is_none());
    let program = ShaderProgram::new(&ctx, "vs", "fs", &[]).unwrap();
    let va = VertexArray::new(&ctx, vec![VertexAttribute::constant(0, vec![0.])], None).unwrap();
    ctx.validate_shader_program.set(true);
    *host.0.fail.borrow_mut() = Some("validateProgram".into());
    assert!(DrawCommand::new(&va, &program).execute(&ctx, &PassState::default()).is_err());
    let calls = host.0.calls.borrow();
    let last = calls.last().unwrap();
    assert_eq!(last.0, "bindVertexArray");
    assert!(matches!(last.1[0], GlValue::Null));
    drop(calls);
    *host.0.fail.borrow_mut() = Some("deleteTexture".into());
    assert!(ctx.destroy().is_err());
    assert!(!ctx.is_destroyed());
    assert!(!texture.is_destroyed());
    host.0.fail.borrow_mut().take();
    ctx.destroy().unwrap();
    assert!(ctx.is_destroyed());
    assert!(texture.is_destroyed());
    assert!(second.object().is_none());
    let deletes = host.count("deleteTexture");
    drop(texture);
    drop(ctx);
    assert_eq!(host.count("deleteTexture"), deletes);
}
#[test]
fn context_cache_has_no_reference_cycles() {
    let host = Host::default();
    {
        let ctx = Context::new_webgl2(host.clone());
        ctx.default_texture().unwrap();
        ctx.default_cube_map().unwrap();
        ctx.get_viewport_quad_vertex_array().unwrap();
        ctx.with_shader_cache(|cache| {
            cache.get_shader_program(&ViewportQuad::<Host>::shader_options(ShaderSource::new("void main(){}")))?;
            Ok(())
        })
        .unwrap();
    }
    assert_eq!(host.count("createTexture"), host.count("deleteTexture"));
    assert_eq!(host.count("createBuffer"), host.count("deleteBuffer"));
    assert_eq!(host.count("createVertexArray"), host.count("deleteVertexArray"));
    assert_eq!(host.count("createProgram"), host.count("deleteProgram"));
}
#[derive(Clone, Default)]
struct Surfaces(Rc<RefCell<Vec<(TextureRegion, [u32; 2])>>>);
impl SharedSurfaceHost<Host> for Surfaces {
    type Surface = u32;
    fn same_surface(&self, a: &u32, b: &u32) -> bool {
        a == b
    }
    fn validate_surface(&self, surface: &u32) -> Result<(), RuntimeError> {
        if *surface == 0 {
            Err(RuntimeError::new("not 2D"))
        } else {
            Ok(())
        }
    }
    fn dimensions(&self, surface: &u32) -> Result<[u32; 2], RuntimeError> {
        Ok([*surface, *surface])
    }
    fn grow_offscreen(&self, _: &Context<Host>, _: [u32; 2]) -> Result<(), RuntimeError> {
        Ok(())
    }
    fn blit(&self, _: &Context<Host>, _: &u32, region: TextureRegion, size: [u32; 2]) -> Result<(), RuntimeError> {
        self.0.borrow_mut().push((region, size));
        Ok(())
    }
}
#[test]
fn shared_context_retains_cross_scene_resources_and_auto_destroys_last_lease() {
    let host = Host::default();
    let surfaces = Surfaces::default();
    let shared = SharedContext::new(Context::new_webgl2(host.clone()), surfaces.clone(), true);
    assert!(shared.create_scene_context(0).is_err());
    let a = shared.create_scene_context(4).unwrap();
    let b = shared.create_scene_context(6).unwrap();
    assert!(shared.create_scene_context(4).is_err());
    assert_eq!(shared.active_scenes(), 2);
    let buffer =
        Buffer::new(a.context().unwrap(), BufferKind::Vertex, BufferUsage::StaticDraw, BufferData::Size(16)).unwrap();
    let clone = a.context().unwrap().clone();
    a.destroy().unwrap();
    assert!(clone.drawing_buffer_size().is_err());
    assert!(!buffer.is_destroyed());
    buffer.copy_from_array_view(&[1, 2, 3, 4], 0).unwrap();
    b.begin_frame().unwrap();
    b.end_frame().unwrap();
    let blits = surfaces.0.borrow();
    assert_eq!(blits[0].0.y, 2);
    assert_eq!(blits[0].1, [6, 6]);
    drop(blits);
    *host.0.fail.borrow_mut() = Some("deleteBuffer".into());
    assert!(b.destroy().is_err());
    assert!(!shared.is_destroyed());
    host.0.fail.borrow_mut().take();
    b.destroy().unwrap();
    assert!(shared.is_destroyed());
    assert!(buffer.is_destroyed());
    let keep = SharedContext::new(Context::new_webgl2(host), Surfaces::default(), false);
    keep.create_scene_context(1).unwrap().destroy().unwrap();
    assert!(!keep.is_destroyed());
    keep.destroy().unwrap();
}
#[test]
fn webgl1_rejects_webgl2_only_resources_before_allocation() {
    let host = Host::default();
    let ctx = Context::new_webgl1(host.clone());
    assert!(Texture3D::new(&ctx, TextureOptions::rgba(1, 1), 1, None).is_err());
    assert!(Sync::new(&ctx).is_err());
    assert!(Buffer::new(&ctx, BufferKind::PixelPack, BufferUsage::StreamDraw, BufferData::Size(4)).is_err());
    assert_eq!(host.count("createTexture"), 0);
    assert_eq!(host.count("createBuffer"), 0);
    let mut options = TextureOptions::rgba(1, 1);
    options.pixel_format = PixelFormat::Luminance;
    Texture::new(&ctx, options, Some(&[128])).unwrap();
}

#[test]
fn reference_scene_uniforms() {
    use cesium_rs::core::*;
    use common::*;
    run(
        "renderer",
        "RendererCompletion",
        |_| Tol { rel: 2e-6, abs: 2e-6, vector: false },
        |f, args| {
            assert_eq!(f, "frameUniform");
            let c = &args[0];
            let mode = c["mode"].as_u64().unwrap_or(3) as u32;
            let ellipsoid =
                if c["sphere"].as_bool().unwrap_or(false) { Ellipsoid::UNIT_SPHERE } else { Ellipsoid::WGS84 };
            let cartographic = Cartographic::new(0.4, 0.2, 1000.);
            let position = if mode == 3 {
                ellipsoid.cartographic_to_cartesian(&cartographic)
            } else {
                Cartesian3::new(100000., 200000., 300000.)
            };
            let direction = if mode == 3 { position.negate().normalize() } else { Cartesian3::new(-1., 0., 0.) };
            let right = direction.cross(Cartesian3::UNIT_Z).normalize();
            let up = right.cross(direction);
            let view = Matrix4::compute_view(position, direction, up, right);
            let mut perspective = PerspectiveFrustum::new(1.1, 4. / 3.);
            perspective.near = 2.;
            perspective.far = 12345.;
            perspective.x_offset = 0.2;
            perspective.y_offset = -0.1;
            let mut ortho = OrthographicFrustum::new(100., 4. / 3.);
            ortho.near = 2.;
            ortho.far = 12345.;
            let frustum = match c["frustum"].as_str().unwrap_or("perspective") {
                "ortho" => UniformFrustum::Orthographic(ortho),
                "offcenter" => {
                    UniformFrustum::PerspectiveOffCenter(PerspectiveOffCenterFrustum::new(-1., 2., 3., -2., 2., 12345.))
                }
                "orthoOffcenter" => UniformFrustum::OrthographicOffCenter(OrthographicOffCenterFrustum::new(
                    -50., 50., 40., -40., 2., 12345.,
                )),
                _ => UniformFrustum::Perspective(perspective),
            };
            let camera = UniformCamera {
                view,
                inverse_view: view.inverse_transformation(),
                position,
                direction,
                right,
                up,
                cartographic: if c["noCartographic"].as_bool().unwrap_or(false) { None } else { Some(cartographic) },
                frustum,
            };
            let date = JulianDate::from_iso8601(c["date"].as_str().unwrap_or("2025-01-02T03:04:05Z")).unwrap();
            let mut frame = UniformFrame::new(camera, date);
            frame.mode = mode;
            frame.map_projection = if c["mercator"].as_bool().unwrap_or(false) {
                Rc::new(WebMercatorProjection::new(ellipsoid))
            } else {
                Rc::new(GeographicProjection::new(ellipsoid))
            };
            frame.frame_number = 37.;
            frame.morph_time = 0.75;
            frame.pixel_ratio = 2.;
            frame.split_position = 0.3;
            frame.maximum_screen_space_error = 8.;
            frame.fog_density = 0.001;
            frame.fog_visual_density_scalar = 2.;
            frame.fog_minimum_brightness = 0.1;
            frame.atmosphere = Some(UniformAtmosphere {
                hsb_shift: Cartesian3::new(0.1, 0.2, 0.3),
                light_intensity: 4.,
                rayleigh_coefficient: Cartesian3::new(0.001, 0.002, 0.003),
                rayleigh_scale_height: 10000.,
                mie_coefficient: Cartesian3::new(0.004, 0.005, 0.006),
                mie_scale_height: 3000.,
                mie_anisotropy: 0.8,
                dynamic_lighting: 2.,
            });
            frame.background_color = Color::new(0.1, 0.2, 0.3, 0.4);
            frame.invert_classification_color = Color::new(0.4, 0.3, 0.2, 0.1);
            frame.gamma = Some(2.2);
            frame.minimum_disable_depth_test_distance =
                if c["infiniteDistance"].as_bool().unwrap_or(false) { f64::INFINITY } else { 100. };
            frame.specular_environment_maps_maximum_lod = Some(5.);
            frame.spherical_harmonic_coefficients =
                (0..9).map(|i| Cartesian3::new(i as f64 * 0.1, i as f64 * 0.2, i as f64 * 0.3)).collect();
            if c["light"].as_bool().unwrap_or(false) {
                frame.light = UniformLight {
                    direction: Some(Cartesian3::new(1., 2., 3.)),
                    color: Color::new(0.3, 0.6, 0.9, 1.),
                    intensity: 4.,
                };
            }
            let mut state = UniformState::default();
            state.viewport = Viewport { x: 7, y: 9, width: 640, height: 480 };
            state.model = Matrix4([2., 0., 0., 0., 0., 3., 0., 0., 0., 0., 4., 0., 12., 34., 56., 1.]);
            state.update(&frame, 800).unwrap();
            state.update_pass(Pass::Opaque);
            if c["subfrustum"].as_bool().unwrap_or(false) {
                state.update_frustum_snapshot(&UniformFrustum::PerspectiveOffCenter(PerspectiveOffCenterFrustum::new(
                    -1., 1., 1., -1., 3., 100.,
                )));
            }

            if c["moveCamera"].as_bool().unwrap_or(false) {
                let mut camera = frame.camera.clone();
                camera.position = camera.position.add(Cartesian3::new(10., 20., 30.));
                camera.cartographic = None;
                camera.view = Matrix4::compute_view(camera.position, camera.direction, camera.up, camera.right);
                camera.inverse_view = camera.view.inverse_transformation();
                state.update_camera_snapshot(&camera, &ellipsoid).unwrap();
            }
            let value = state.get(args[1].as_str().unwrap()).unwrap().unwrap();
            let values = match value {
                UniformValue::Float(v)
                | UniformValue::FloatArray { values: v, .. }
                | UniformValue::Matrix { values: v, .. }
                | UniformValue::MatrixArray { values: v, .. } => v,
                _ => panic!("Expected numeric uniform"),
            };
            Some(Out::Ok(serde_json::json!(values)))
        },
    );
}

#[test]
fn atlas_tracks_async_completion_order_and_propagates_errors() {
    let ctx = Context::new_webgl2(Host::default());
    let mut atlas = TextureAtlas::new(TextureAtlasOptions { initial_size: [16, 16], ..Default::default() }).unwrap();
    let (first_tx, first_rx) = futures_channel::oneshot::channel();
    let (second_tx, second_rx) = futures_channel::oneshot::channel();
    let first = atlas.add_image_future("z-first", async { first_rx.await.unwrap() }).unwrap();
    let second = atlas.add_image_future("a-second", async { second_rx.await.unwrap() }).unwrap();
    let image = || Ok(TextureSource::Pixels { width: 2, height: 2, bytes: vec![255; 16] });
    second_tx.send(image()).ok().unwrap();
    first_tx.send(image()).ok().unwrap();
    atlas.update(&ctx).unwrap();
    assert_eq!(first.now_or_never().unwrap().unwrap(), 0);
    assert_eq!(second.now_or_never().unwrap().unwrap(), 1);
    let first = atlas.compute_texture_coordinates(0).unwrap();
    let second = atlas.compute_texture_coordinates(1).unwrap();
    assert!(second.x < first.x || second.y < first.y, "The earlier completion must be packed first");
    let failed = atlas.add_image_future("failed", std::future::ready(Err(RuntimeError::new("bad image")))).unwrap();
    assert_eq!(failed.now_or_never().unwrap().unwrap_err().message, "bad image");
    let sub = atlas.add_image_sub_region("a-second", cesium_rs::core::BoundingRectangle::new(2., 0., 1., 1.)).unwrap();
    assert!(sub.now_or_never().unwrap().is_err());
}
#[test]
fn freeze_render_state_keeps_nested_snapshots_immutable() {
    let mut original = RenderState::default();
    original.blending.enabled = true;
    let frozen = freeze_render_state(original.clone());
    original.blending.enabled = false;
    assert!(!original.blending.enabled);
    assert!(frozen.blending.enabled);
    let mut copy = frozen.to_owned_state();
    copy.blending.color.red = 1.;
    assert_eq!(copy.blending.color.red, 1.);
    assert_eq!(frozen.blending.color.red, 0.);
}
#[test]
fn cube_loader_starts_all_faces_and_propagates_decode_errors_without_allocating() {
    let host = Host::default();
    let ctx = Context::new_webgl2(host.clone());
    let calls = Rc::new(Cell::new(0));
    let result = load_cube_map(&ctx, std::array::from_fn(|i| i.to_string()), true, |url, skip| {
        calls.set(calls.get() + 1);
        assert!(skip);
        std::future::ready(if url == "2" {
            Err(RuntimeError::new("image failed"))
        } else {
            Ok(TextureSource::Pixels { width: 1, height: 1, bytes: vec![255; 4] })
        })
    })
    .now_or_never()
    .unwrap();
    assert!(result.is_err());
    assert_eq!(calls.get(), 6);
    assert_eq!(host.count("createTexture"), 0);
}
#[test]
fn facade_instancing_preserves_precreated_buffers() {
    let ctx = Context::new_webgl2(Host::default());
    let buffer = Buffer::new(&ctx, BufferKind::Vertex, BufferUsage::StaticDraw, BufferData::Size(24)).unwrap();
    let mut precreated = FacadeAttribute::new(0, 2);
    precreated.vertex_buffer = Some(buffer.clone());
    let mut facade = VertexArrayFacade::new(&ctx, vec![precreated, FacadeAttribute::new(1, 4)], 2, true).unwrap();
    facade.write(1, 0, &[1., 0., 0., 1.]).unwrap();
    facade.write(1, 1, &[0., 1., 0., 1.]).unwrap();
    facade.commit(None).unwrap();
    let va = &facade.vertex_arrays().unwrap()[0].vertex_array;
    assert_eq!(va.attributes()[0].instance_divisor, 1);
    assert_eq!(va.attributes()[1].instance_divisor, 0);
    facade.destroy().unwrap();
    assert!(!buffer.is_destroyed());
}

#[test]
fn facade_counts_full_and_partial_indexed_chunks() {
    let ctx = Context::new_webgl2(Host::default());
    let indices = Buffer::new(
        &ctx,
        BufferKind::Index(cesium_rs::core::IndexDatatype::UnsignedShort),
        BufferUsage::StaticDraw,
        BufferData::Bytes(&[0, 0, 1, 0, 2, 0]),
    )
    .unwrap();
    for (size, expected) in [(65532, vec![98298.]), (131064, vec![98298., 98298.]), (65535, vec![98298., 4.5])] {
        let mut facade = VertexArrayFacade::new(&ctx, vec![FacadeAttribute::new(0, 1)], size, false).unwrap();
        facade.write(0, 0, &[1.]).unwrap();
        facade.commit(Some(indices.clone())).unwrap();
        assert_eq!(facade.vertex_arrays().unwrap().iter().map(|va| va.indices_count).collect::<Vec<_>>(), expected);
    }
}
#[test]
fn packed_cube_rows_flip_as_pixels_on_initial_and_subimage_uploads() {
    for (format, datatype) in [
        (PixelFormat::Rgba, PixelDatatype::UnsignedShort4_4_4_4),
        (PixelFormat::Rgba, PixelDatatype::UnsignedShort5_5_5_1),
        (PixelFormat::Rgb, PixelDatatype::UnsignedShort5_6_5),
    ] {
        let host = Host::default();
        let ctx = Context::new_webgl2(host.clone());
        let mut options = TextureOptions::rgba(2, 2);
        options.pixel_format = format;
        options.pixel_datatype = datatype;
        let bytes = [0x1234u16, 0x5678, 0x9abc, 0xdef0].into_iter().flat_map(u16::to_le_bytes).collect::<Vec<_>>();
        let source = || TextureSource::Pixels { width: 2, height: 2, bytes: bytes.clone() };
        let cube = CubeMap::new(&ctx, options, Some(std::array::from_fn(|_| source()))).unwrap();
        cube.face(CubeFace::PositiveX).copy_from(&source(), 0, 0, false).unwrap();
        let expected = [0x9abcu16, 0xdef0, 0x1234, 0x5678].into_iter().flat_map(u16::to_le_bytes).collect::<Vec<_>>();
        let calls = host.0.calls.borrow();
        let uploads =
            calls.iter().filter(|(name, _)| name == "texImage2D" || name == "texSubImage2D").collect::<Vec<_>>();
        assert_eq!(uploads.len(), 7);
        for (_, args) in uploads {
            assert!(matches!(args.last(),Some(GlValue::Pixels{bytes,..})if bytes==&expected));
        }
    }
}
#[test]
fn volume_memory_counts_allocated_levels_for_supplied_partial_and_generated_mips() {
    let ctx = Context::new_webgl2(Host::default());
    // 8x4x2, 4x2x1, 2x1x1, 1x1x1: immutable storage reserves all levels.
    let full = Texture3D::with_mipmaps(
        &ctx,
        TextureOptions::rgba(8, 4),
        2,
        Some(&[0; 256]),
        &[vec![0; 32], vec![0; 8], vec![0; 4]],
    )
    .unwrap();
    assert_eq!(full.size_in_bytes(), 300);
    let partial =
        Texture3D::with_mipmaps(&ctx, TextureOptions::rgba(8, 4), 2, Some(&[0; 256]), &[vec![0; 32]]).unwrap();
    assert_eq!(partial.size_in_bytes(), 300);
    let blank = Texture3D::new(&ctx, TextureOptions::rgba(3, 5), 2, None).unwrap();
    assert_eq!(blank.size_in_bytes(), 120);
    blank.generate_mipmap(MipmapHint::DontCare).unwrap();
    assert_eq!(blank.size_in_bytes(), 132);
}
#[test]
fn frame_tolerance_uses_first_surface_resize_and_current_draw_viewport() {
    use cesium_rs::core::*;
    let host = Host::default();
    let ctx = Context::new_webgl2(host.clone());
    let camera = UniformCamera {
        view: Matrix4::IDENTITY,
        inverse_view: Matrix4::IDENTITY,
        position: Cartesian3::new(6379137., 0., 0.),
        direction: Cartesian3::new(-1., 0., 0.),
        right: Cartesian3::UNIT_Y,
        up: Cartesian3::UNIT_Z,
        cartographic: Some(Cartographic::new(0., 0., 1000.)),
        frustum: UniformFrustum::Perspective(PerspectiveFrustum::new(1., 1.)),
    };
    let mut frame = UniformFrame::new(camera, JulianDate::from_iso8601("2025-01-01T00:00:00Z").unwrap());
    frame.maximum_screen_space_error = 3.;
    let value = || match ctx.uniform_state().get("czm_geometricToleranceOverMeter").unwrap().unwrap() {
        UniformValue::Float(v) => v[0] as f64,
        _ => panic!("Expected float"),
    };
    let check = |denominator: f64| {
        let expected = 2. * 0.5f64.tan() * 3. / denominator;
        assert!((value() - expected).abs() < 1e-7);
    };
    ctx.update_frame(&frame, &FrameTextures::default()).unwrap();
    check(8.);
    ctx.uniform_state().viewport = Viewport { x: 0, y: 0, width: 4, height: 4 };
    check(4.);
    host.0.dimensions.set(Some([32, 16]));
    ctx.update_frame(&frame, &FrameTextures::default()).unwrap();
    check(32.);
    frame.camera.frustum = UniformFrustum::Orthographic(OrthographicFrustum::new(10., 1.));
    ctx.update_frame(&frame, &FrameTextures::default()).unwrap();
    assert_eq!(value(), 3. / 32.);
    ctx.uniform_state().viewport = Viewport::default();
    assert!(value().is_finite());
}
