//! Reference parity tests for all 6 Point Cloud & Voxel Scene modules against
//! SwiftShader WebGL2 captures from pinned `@cesium/engine` 23.0.0:
//! `tests/data/scene-pointcloud-voxel-gpu.json` (`tools/cesium/generate-scene-pointcloud-voxel-gpu.mjs`).
use base64::Engine;
use cesium_rs::core::{
    clock::{get_timestamp, ClockOptions},
    request::RequestFuture,
    resource::{HttpRequest, HttpResponse, ResourceTransport},
    webgl_constants as gl, BoundingSphere, Cartesian3, Clock, ClockRange, JulianDate, Matrix4, PrimitiveType, Resource,
    ResourceClient, RuntimeError, TimeInterval, TimeIntervalCollection, TimeStandard, TypedArray,
};
use cesium_rs::renderer::{
    Buffer, BufferData, BufferKind, BufferUsage, Context, DrawMetadata, Gl, GlValue, Pass, ShaderProgram,
    ShaderProgramOptions, ShaderSource, UniformFrame, VertexArray, VertexAttribute,
};
use cesium_rs::scene::{
    megatexture::Megatexture, Camera, CameraOptions, Cesium3DTilesVoxelProvider, ClippingPlane,
    ClippingPlaneCollection, FrameResource, FrameState, MetadataComponentType, MetadataType, PointCloud,
    PointCloudEyeDomeLighting, PointCloudOptions, PointCloudShading, PointCloudShadingOptions, SceneCommand,
    SceneDrawOptions, SceneMode, ScenePrimitive, ShadowMode, TimeDynamicPointCloud, TimeDynamicPointCloudInterval,
    TimeDynamicPointCloudOptions, VoxelContent, VoxelMetadataOrder, VoxelPrimitive, VoxelPrimitiveOptions,
    VoxelProvider, VoxelRequest, VoxelShapeType,
};
use futures_util::FutureExt;
use serde_json::Value;
use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, HashMap};
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

fn fixture() -> Value {
    serde_json::from_str(include_str!("data/scene-pointcloud-voxel-gpu.json")).unwrap()
}

fn decode_b64(s: &str) -> Vec<u8> {
    base64::engine::general_purpose::STANDARD.decode(s).unwrap()
}

fn f64s(v: &Value) -> Vec<f64> {
    v.as_array().unwrap().iter().map(|x| x.as_f64().unwrap()).collect()
}

fn assert_close(a: f64, b: f64, tol: f64, label: &str) {
    assert!((a - b).abs() <= tol, "{label}: actual={a} expected={b} diff={} tol={tol}", (a - b).abs());
}

fn assert_slice_close(actual: &[f64], expected: &[f64], tol: f64, label: &str) {
    assert_eq!(actual.len(), expected.len(), "{label}: len mismatch");
    for (i, (&a, &b)) in actual.iter().zip(expected).enumerate() {
        assert_close(a, b, tol, &format!("{label}[{i}]"));
    }
}

#[derive(Clone)]
struct RecordingGl {
    next: Rc<Cell<u32>>,
    max_3d_texture_size: u32,
    size: [u32; 2],
}

impl RecordingGl {
    fn new(size: [u32; 2], max_3d_texture_size: u32) -> Self {
        Self { next: Rc::new(Cell::new(0)), max_3d_texture_size, size }
    }
}

impl Gl for RecordingGl {
    type Object = u32;
    fn drawing_buffer_size(&self) -> Result<[u32; 2], RuntimeError> {
        Ok(self.size)
    }
    fn invoke(&self, name: &str, args: &[GlValue<u32>]) -> Result<GlValue<u32>, RuntimeError> {
        if name.starts_with("create") {
            self.next.set(self.next.get() + 1);
            return Ok(GlValue::Object(self.next.get()));
        }
        let p = match args.first() {
            Some(GlValue::Number(v)) => Some(*v as u32),
            _ => None,
        };
        Ok(match name {
            "getParameter" if matches!(p, Some(gl::MAX_3D_TEXTURE_SIZE)) => {
                GlValue::Number(f64::from(self.max_3d_texture_size))
            }
            "getParameter"
                if matches!(
                    p,
                    Some(gl::MAX_VIEWPORT_DIMS | gl::ALIASED_LINE_WIDTH_RANGE | gl::ALIASED_POINT_SIZE_RANGE)
                ) =>
            {
                GlValue::Numbers(vec![1.0, 4096.0])
            }
            "getParameter" if matches!(p, Some(gl::MAX_COLOR_ATTACHMENTS | gl::MAX_DRAW_BUFFERS)) => {
                GlValue::Number(8.0)
            }
            "getParameter" => GlValue::Number(4096.0),
            "getExtension" => GlValue::Object(1),
            "getShaderParameter" | "getProgramParameter" => GlValue::Bool(true),
            "checkFramebufferStatus" => GlValue::Number(gl::FRAMEBUFFER_COMPLETE as f64),
            "getUniformLocation" => GlValue::Object(1),
            _ => GlValue::Null,
        })
    }
}

struct MockTransport {
    routes: HashMap<String, Result<Vec<u8>, String>>,
}

impl ResourceTransport for MockTransport {
    fn execute(&self, request: HttpRequest) -> RequestFuture<HttpResponse> {
        let entry = self.routes.get(&request.url).cloned().unwrap_or_else(|| Err(format!("404 {}", request.url)));
        Box::pin(async move {
            match entry {
                Ok(body) => Ok(HttpResponse { status: 200, headers: Vec::new(), body }),
                Err(msg) => Err(cesium_rs::core::RequestErrorEvent::new(
                    Some(404),
                    Some(msg.into_bytes()),
                    Some(Vec::new()),
                )),
            }
        })
    }
}

fn make_frame(context: &Context<RecordingGl>, size: [u32; 2], time: JulianDate) -> FrameState {
    let mut camera = Camera::new(CameraOptions::new(f64::from(size[0]), f64::from(size[1])));
    camera.position = Cartesian3::new(0.0, 0.0, 5.0);
    camera.direction = Cartesian3::new(0.0, 0.0, -1.0);
    camera.up = Cartesian3::UNIT_Y;
    camera.right = Cartesian3::UNIT_X;
    let uniform = camera.uniform_camera();
    let culling = camera.frustum.compute_culling_volume(uniform.position, uniform.direction, uniform.up);
    let uniform_frame = UniformFrame::new(uniform, time);
    let _ = context.uniform_state().update(&uniform_frame, size[0]);
    let mut frame = FrameState::new(uniform_frame, culling, size);
    frame.context = Some(FrameResource::new(context.clone()));
    frame.passes.render = true;
    frame
}

#[test]
fn megatexture_slots_and_uploads_match_cesium_gpu_capture() {
    let f = fixture();
    let m = &f["megatexture"];
    let max_3d = m["max3D"].as_u64().unwrap() as u32;
    let context = Context::new_webgl2(RecordingGl::new([64, 64], max_3d));

    // m1: 1 channel FLOAT32, tile (4,4,4), available 1024*1024, max 4 -> 1D-to-3D upload + slot reuse
    let mut m1 = Megatexture::new(
        &context,
        Cartesian3::new(4.0, 4.0, 4.0),
        1,
        MetadataComponentType::Float32,
        1024 * 1024,
        Some(4),
    )
    .unwrap();
    let m1_exp = &m["m1Info"];
    assert_eq!(m1.maximum_tile_count as u64, m1_exp["maximumTileCount"].as_u64().unwrap());
    assert_eq!(m1.texture_memory_byte_length as u64, m1_exp["textureMemoryByteLength"].as_u64().unwrap());
    assert_eq!(m1.texture.pixel_format() as u32, m1_exp["pixelFormat"].as_u64().unwrap() as u32);
    assert_eq!(m1.texture.pixel_datatype() as u32, m1_exp["pixelDatatype"].as_u64().unwrap() as u32);
    assert_eq!(
        serde_json::json!(m1.texture.dimensions()),
        m1_exp["textureDims"]
    );
    assert_eq!(m1.texture.sampler().minification_filter as u32, m1_exp["samplerLinear"]["min"].as_u64().unwrap() as u32);
    m1.set_nearest_sampling(true).unwrap();
    assert_eq!(m1.texture.sampler().minification_filter as u32, m1_exp["samplerNearest"]["min"].as_u64().unwrap() as u32);
    m1.destroy().unwrap();
    assert!(m1.is_destroyed());

    // m2: 4 channels UINT8, tile (2,3,2), max 6 -> full add/remove/reuse sequence + fullError
    let mut m2 = Megatexture::new(
        &context,
        Cartesian3::new(2.0, 3.0, 2.0),
        4,
        MetadataComponentType::Uint8,
        1024,
        Some(6),
    )
    .unwrap();
    let m2_exp = &m["m2Info"];
    assert_eq!(m2.maximum_tile_count as u64, m2_exp["maximumTileCount"].as_u64().unwrap());
    assert_eq!(m2.texture_memory_byte_length as u64, m2_exp["textureMemoryByteLength"].as_u64().unwrap());
    assert_eq!(m2.texture.pixel_format() as u32, m2_exp["pixelFormat"].as_u64().unwrap() as u32);
    assert_eq!(m2.texture.pixel_datatype() as u32, m2_exp["pixelDatatype"].as_u64().unwrap() as u32);
    assert_eq!(serde_json::json!(m2.texture.dimensions()), m2_exp["textureDims"]);
    let tile = TypedArray::Uint8(vec![7u8; 48]);
    for step in m2_exp["seq"].as_array().unwrap() {
        let op = step["op"].as_str().unwrap();
        let expected_slot = step["slot"].as_u64().unwrap() as usize;
        if op == "add" {
            let slot = m2.add(&tile).unwrap();
            assert_eq!(slot, expected_slot);
        } else {
            m2.remove(expected_slot).unwrap();
        }
        assert_eq!(m2.occupied_count as u64, step["occupied"].as_u64().unwrap());
        assert_eq!(m2.is_full(), step["full"].as_bool().unwrap());
    }
    assert_eq!(m2.add(&tile).unwrap_err().message, m2_exp["fullError"].as_str().unwrap());
    m2.destroy().unwrap();
    assert_eq!(m2.is_destroyed(), m["m2Destroyed"].as_bool().unwrap());
}

#[test]
fn cesium_3d_tiles_voxel_provider_async_gltf_matches_cesium_capture() {
    let f = fixture();
    let vp = &f["cesium3DTilesVoxelProvider"];
    let assets = &f["assets"];

    let mut routes = HashMap::new();
    let base = "https://example.test/voxels";
    routes.insert(format!("{base}/box/tileset.json"), Ok(serde_json::to_vec(&assets["voxelBoxTileset"]).unwrap()));
    routes.insert(
        format!("{base}/region/tileset.json"),
        Ok(serde_json::to_vec(&assets["voxelRegionTileset"]).unwrap()),
    );
    routes.insert(
        format!("{base}/cylinder/tileset.json"),
        Ok(serde_json::to_vec(&assets["voxelCylinderTileset"]).unwrap()),
    );
    routes.insert(format!("{base}/box/subtrees/0.0.0.0.subtree"), Ok(decode_b64(assets["subtree0Base64"].as_str().unwrap())));
    routes.insert(format!("{base}/box/content/0_0_0_0.glb"), Ok(decode_b64(assets["glb0Base64"].as_str().unwrap())));
    routes.insert(format!("{base}/box/content/1_0_0_0.glb"), Ok(decode_b64(assets["glb1Base64"].as_str().unwrap())));

    let client = ResourceClient::new(Rc::new(MockTransport { routes }));

    let check_provider = |p: &Cesium3DTilesVoxelProvider, exp: &Value, name: &str| {
        let shape_str = match p.shape() {
            VoxelShapeType::Box => "BOX",
            VoxelShapeType::Ellipsoid => "ELLIPSOID",
            VoxelShapeType::Cylinder => "CYLINDER",
        };
        assert_eq!(shape_str, exp["shape"].as_str().unwrap(), "{name} shape");
        assert_slice_close(&[p.dimensions().x, p.dimensions().y, p.dimensions().z], &f64s(&exp["dimensions"]), 1e-9, &format!("{name} dimensions"));
        assert_slice_close(&[p.padding_before().x, p.padding_before().y, p.padding_before().z], &f64s(&exp["paddingBefore"]), 1e-9, &format!("{name} paddingBefore"));
        assert_slice_close(&[p.padding_after().x, p.padding_after().y, p.padding_after().z], &f64s(&exp["paddingAfter"]), 1e-9, &format!("{name} paddingAfter"));
        assert_slice_close(&p.global_transform().0, &f64s(&exp["globalTransform"]), 1e-6, &format!("{name} globalTransform"));
        assert_slice_close(&p.shape_transform().0, &f64s(&exp["shapeTransform"]), 1e-6, &format!("{name} shapeTransform"));
        assert_eq!(p.maximum_tile_count().map(|v| v as u64), exp["maximumTileCount"].as_u64(), "{name} maxTileCount");
        assert_eq!(p.available_levels().map(u64::from), exp["availableLevels"].as_u64(), "{name} availableLevels");
        assert_eq!(p.metadata_order() as u32, exp["metadataOrder"].as_u64().unwrap() as u32, "{name} metadataOrder");
        let exp_names: Vec<String> = exp["names"].as_array().unwrap().iter().map(|s| s.as_str().unwrap().into()).collect();
        assert_eq!(p.names(), exp_names.as_slice(), "{name} names");
        if let Some(min_b) = p.min_bounds() {
            assert_slice_close(&[min_b.x, min_b.y, min_b.z], &f64s(&exp["minBounds"]), 1e-6, &format!("{name} minBounds"));
        }
        if let Some(max_b) = p.max_bounds() {
            assert_slice_close(&[max_b.x, max_b.y, max_b.z], &f64s(&exp["maxBounds"]), 1e-6, &format!("{name} maxBounds"));
        }
    };

    let mut box_provider = Cesium3DTilesVoxelProvider::from_url(
        Resource::new(format!("{base}/box/tileset.json")).unwrap(),
        client.clone(),
    )
    .now_or_never()
    .unwrap()
    .unwrap();
    check_provider(&box_provider, &vp["box"], "box");

    let region_provider = Cesium3DTilesVoxelProvider::from_url(
        Resource::new(format!("{base}/region/tileset.json")).unwrap(),
        client.clone(),
    )
    .now_or_never()
    .unwrap()
    .unwrap();
    check_provider(&region_provider, &vp["region"], "region");

    let cylinder_provider = Cesium3DTilesVoxelProvider::from_url(
        Resource::new(format!("{base}/cylinder/tileset.json")).unwrap(),
        client,
    )
    .now_or_never()
    .unwrap()
    .unwrap();
    check_provider(&cylinder_provider, &vp["cylinder"], "cylinder");

    // End-to-end async GLB voxel attribute decoding for tile0 and tile1
    for (req, key) in [
        (VoxelRequest { tile_level: 0, tile_x: 0, tile_y: 0, tile_z: 0, keyframe: 0 }, "tile0"),
        (VoxelRequest { tile_level: 1, tile_x: 0, tile_y: 0, tile_z: 0, keyframe: 0 }, "tile1"),
    ] {
        let content = box_provider.request_data_async(req).now_or_never().unwrap().unwrap();
        let arrays = content.metadata();
        let exp_arrs = vp[key].as_array().unwrap();
        assert_eq!(arrays.len(), exp_arrs.len(), "{key} attribute count");
        assert_slice_close(&arrays[0].to_f64_vec(), &f64s(&exp_arrs[0]["values"]), 1e-5, &format!("{key} temperature"));
        assert_slice_close(&arrays[1].to_f64_vec(), &f64s(&exp_arrs[1]["values"]), 1e-5, &format!("{key} rgb"));
    }

    // Unavailable tile & unsupported keyframe errors
    let unavail = box_provider
        .request_data_async(VoxelRequest { tile_level: 1, tile_x: 1, tile_y: 1, tile_z: 1, keyframe: 0 })
        .now_or_never()
        .unwrap();
    assert!(unavail.is_err(), "unavailable tile must fail");
    let kf_err = box_provider
        .request_data_async(VoxelRequest { tile_level: 0, tile_x: 0, tile_y: 0, tile_z: 0, keyframe: 1 })
        .now_or_never()
        .unwrap();
    assert!(kf_err.is_err(), "non-zero keyframe must fail");
}

struct BoxCaptureProvider {
    names: Vec<String>,
    types: Vec<MetadataType>,
    component_types: Vec<MetadataComponentType>,
    mins: Vec<Vec<f64>>,
    maxs: Vec<Vec<f64>>,
    identity: Matrix4,
}

impl BoxCaptureProvider {
    fn new() -> Self {
        Self {
            names: vec!["density".into(), "color".into()],
            types: vec![MetadataType::Scalar, MetadataType::Vec3],
            component_types: vec![MetadataComponentType::Float32, MetadataComponentType::Uint8],
            mins: vec![vec![0.0], vec![0.0, 0.0, 0.0]],
            maxs: vec![vec![1.0], vec![255.0, 255.0, 255.0]],
            identity: Matrix4::IDENTITY,
        }
    }
}

impl VoxelProvider for BoxCaptureProvider {
    fn global_transform(&self) -> &Matrix4 {
        &self.identity
    }
    fn shape_transform(&self) -> &Matrix4 {
        &self.identity
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
        Cartesian3::new(2.0, 2.0, 2.0)
    }
    fn padding_before(&self) -> Cartesian3 {
        Cartesian3::new(1.0, 0.0, 0.0)
    }
    fn padding_after(&self) -> Cartesian3 {
        Cartesian3::new(0.0, 1.0, 0.0)
    }
    fn names(&self) -> &[String] {
        &self.names
    }
    fn types(&self) -> &[MetadataType] {
        &self.types
    }
    fn component_types(&self) -> &[MetadataComponentType] {
        &self.component_types
    }
    fn minimum_values(&self) -> Option<&[Vec<f64>]> {
        Some(&self.mins)
    }
    fn maximum_values(&self) -> Option<&[Vec<f64>]> {
        Some(&self.maxs)
    }
    fn available_levels(&self) -> Option<u32> {
        Some(2)
    }
    fn maximum_tile_count(&self) -> Option<usize> {
        Some(9)
    }
    fn keyframe_count(&self) -> Option<u32> {
        None
    }
    fn time_interval_collection(&self) -> Option<&TimeIntervalCollection<u32>> {
        None
    }
    fn metadata_order(&self) -> VoxelMetadataOrder {
        VoxelMetadataOrder::YUp
    }
    fn request_data(
        &mut self,
        options: VoxelRequest,
    ) -> Option<Pin<Box<dyn Future<Output = Result<VoxelContent, RuntimeError>>>>> {
        if options.tile_level >= 2 {
            return None;
        }
        let n = 18;
        let res = VoxelContent::from_metadata_array(vec![
            TypedArray::Float32(vec![0.5; n]),
            TypedArray::Uint8(vec![200; n * 3]),
        ]);
        Some(Box::pin(async move { Ok(res) }))
    }
}

#[test]
fn voxel_primitive_commands_and_metrics_match_cesium_gpu_capture() {
    let f = fixture();
    let case_a = &f["voxelPrimitive"]["caseA"];
    let context = Context::new_webgl2(RecordingGl::new([64, 64], 2048));
    let mut prim = VoxelPrimitive::<RecordingGl>::new(VoxelPrimitiveOptions {
        provider: Some(Box::new(BoxCaptureProvider::new())),
        calculate_statistics: true,
        ..Default::default()
    })
    .unwrap();
    let epoch = JulianDate::new(2_451_545.0, 0.0, TimeStandard::Tai);
    for frame_num in 1..=4 {
        let mut frame = make_frame(&context, [64, 64], epoch);
        frame.frame_number = frame_num;
        prim.update(&mut frame).unwrap();
    }
    assert_eq!(prim.ready(), case_a["ready"].as_bool().unwrap());
    assert_slice_close(
        &[prim.dimensions().x, prim.dimensions().y, prim.dimensions().z],
        &f64s(&case_a["dimensions"]),
        1e-9,
        "voxel dimensions",
    );
    assert_slice_close(
        &[prim.input_dimensions().x, prim.input_dimensions().y, prim.input_dimensions().z],
        &f64s(&case_a["inputDimensions"]),
        1e-9,
        "voxel inputDimensions",
    );
    let bs = prim.bounding_sphere().unwrap();
    assert_close(bs.radius, case_a["boundingSphere"]["radius"].as_f64().unwrap(), 1e-6, "voxel boundingSphere radius");
    assert!(prim.statistics().textures_byte_length > 0);

    // Render pass command
    let mut frame_render = make_frame(&context, [64, 64], epoch);
    frame_render.frame_number = 5;
    prim.update(&mut frame_render).unwrap();
    assert_eq!(frame_render.command_list.len(), 1);
    assert_eq!(frame_render.command_list[0].pass as u32, case_a["renderSnap"]["pass"].as_u64().unwrap() as u32);
    assert_eq!(frame_render.command_list[0].metadata.pick_only, case_a["renderSnap"]["pickOnly"].as_bool().unwrap());

    // Pick pass command
    let mut frame_pick = make_frame(&context, [64, 64], epoch);
    frame_pick.frame_number = 6;
    frame_pick.passes.render = false;
    frame_pick.passes.pick = true;
    prim.update(&mut frame_pick).unwrap();
    assert_eq!(frame_pick.command_list.len(), 1);
    assert_eq!(frame_pick.command_list[0].pass as u32, case_a["pickSnap"]["pass"].as_u64().unwrap() as u32);
    assert_eq!(frame_pick.command_list[0].metadata.pick_only, case_a["pickSnap"]["pickOnly"].as_bool().unwrap());

    // PickVoxel pass command
    let mut frame_pick_voxel = make_frame(&context, [64, 64], epoch);
    frame_pick_voxel.frame_number = 7;
    frame_pick_voxel.passes.render = false;
    frame_pick_voxel.passes.pick_voxel = true;
    prim.update(&mut frame_pick_voxel).unwrap();
    assert_eq!(frame_pick_voxel.command_list.len(), 1);
    assert_eq!(
        frame_pick_voxel.command_list[0].pass as u32,
        case_a["pickVoxelSnap"]["pass"].as_u64().unwrap() as u32
    );

    prim.destroy().unwrap();
    assert_eq!(prim.statistics().textures_byte_length, 0);
}

#[test]
fn point_cloud_eye_dome_lighting_hijacks_commands_and_matches_cesium() {
    let f = fixture();
    let edl_ref = &f["pointCloudEyeDomeLighting"];
    let assets = &f["assets"];
    let context = Context::new_webgl2(RecordingGl::new([32, 24], 2048));
    assert_eq!(PointCloudEyeDomeLighting::<RecordingGl>::is_supported(&context), edl_ref["supported"].as_bool().unwrap());

    let epoch = JulianDate::new(2_451_545.0, 0.0, TimeStandard::Tai);
    let mut frame = make_frame(&context, [32, 24], epoch);
    frame.pixel_ratio = 2.0;

    // [0]: pre-existing opaque point cloud before command_start=1 (must stay untouched)
    let pnts0 = Arc::new(decode_b64(assets["tdFrame0Base64"].as_str().unwrap()));
    let mut pre_cloud = PointCloud::<RecordingGl>::new(PointCloudOptions {
        array_buffer: pnts0.clone(),
        ..Default::default()
    })
    .unwrap();
    pre_cloud.update(&mut frame).unwrap();
    assert_eq!(frame.command_list.len(), 1);
    let command_start = frame.command_list.len();

    // [1]: opaque point cloud after command_start (must be hijacked into EDL MRT FBO)
    let mut opaque_cloud = PointCloud::<RecordingGl>::new(PointCloudOptions {
        array_buffer: pnts0,
        ..Default::default()
    })
    .unwrap();
    opaque_cloud.update(&mut frame).unwrap();

    // [2]: translucent point cloud (must NOT be hijacked)
    let mut translucent_cmd = frame.command_list[1].clone();
    translucent_cmd.pass = Pass::Translucent;
    frame.command_list.push(translucent_cmd);

    // [3]: triangle draw command (must NOT be hijacked)
    let vb = Buffer::new(&context, BufferKind::Vertex, BufferUsage::StaticDraw, BufferData::Bytes(&[0u8; 36])).unwrap();
    let va = Rc::new(VertexArray::new(&context, vec![VertexAttribute::buffer(0, vb, 3)], None).unwrap());
    let prog = Rc::new(
        ShaderProgram::from_sources(
            &context,
            &ShaderProgramOptions {
                vertex_shader_source: ShaderSource::new("in vec3 position; void main() { gl_Position = vec4(position, 1.0); }"),
                fragment_shader_source: ShaderSource::new("void main() { out_FragColor = vec4(1.0); }"),
                attribute_locations: BTreeMap::from([("position".into(), 0)]),
            },
        )
        .unwrap(),
    );
    let mut tri_opts = SceneDrawOptions::new(va, prog);
    tri_opts.primitive_type = PrimitiveType::Triangles;
    frame.command_list.push(SceneCommand::from_draw(
        "tri",
        context.clone(),
        Pass::Cesium3DTile,
        DrawMetadata::default(),
        tri_opts,
    ));

    let pre_program_id = Rc::as_ptr(&frame.command_list[0].draw_options::<RecordingGl>().unwrap().program);
    let orig_opaque_program_id = Rc::as_ptr(&frame.command_list[1].draw_options::<RecordingGl>().unwrap().program);
    let orig_translucent_program_id = Rc::as_ptr(&frame.command_list[2].draw_options::<RecordingGl>().unwrap().program);
    let orig_tri_program_id = Rc::as_ptr(&frame.command_list[3].draw_options::<RecordingGl>().unwrap().program);

    let shading = PointCloudShading::new(PointCloudShadingOptions {
        eye_dome_lighting: Some(true),
        eye_dome_lighting_strength: Some(1.75),
        eye_dome_lighting_radius: Some(2.5),
        ..Default::default()
    });
    let bv = BoundingSphere::new(Cartesian3::new(1.0, 2.0, 3.0), 15.0);
    let mut edl = PointCloudEyeDomeLighting::<RecordingGl>::new();
    edl.update(&mut frame, command_start, &shading, Some(bv)).unwrap();

    assert_eq!(frame.command_list.len() as u64, edl_ref["commandListLength"].as_u64().unwrap());
    let fb_ptr = Rc::as_ptr(edl.framebuffer().unwrap());
    assert_eq!(
        Rc::as_ptr(&frame.command_list[0].draw_options::<RecordingGl>().unwrap().program) == pre_program_id,
        edl_ref["preUntouched"].as_bool().unwrap()
    );
    let hijacked = frame.command_list[1].draw_options::<RecordingGl>().unwrap();
    assert_eq!(Rc::as_ptr(&hijacked.program) != orig_opaque_program_id, edl_ref["opaqueReplaced"].as_bool().unwrap());
    assert_eq!(
        hijacked.framebuffer.as_ref().map(Rc::as_ptr) == Some(fb_ptr),
        edl_ref["opaqueTargetsEdlFb"].as_bool().unwrap()
    );
    assert_eq!(frame.command_list[1].metadata.cast_shadows, edl_ref["opaqueCastShadows"].as_bool().unwrap());
    assert_eq!(frame.command_list[1].metadata.receive_shadows, edl_ref["opaqueReceiveShadows"].as_bool().unwrap());
    let derived_sources = &hijacked.program.source_options().unwrap().fragment_shader_source.sources;
    let exp_sources: Vec<String> = edl_ref["derivedFragmentSources"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| s.as_str().unwrap().into())
        .collect();
    assert_eq!(derived_sources, &exp_sources);

    assert_eq!(
        Rc::as_ptr(&frame.command_list[2].draw_options::<RecordingGl>().unwrap().program) == orig_translucent_program_id,
        edl_ref["translucentUntouched"].as_bool().unwrap()
    );
    assert_eq!(
        Rc::as_ptr(&frame.command_list[3].draw_options::<RecordingGl>().unwrap().program) == orig_tri_program_id,
        edl_ref["triangleUntouched"].as_bool().unwrap()
    );

    // [4]: EDL fullscreen blend command
    let blend = &frame.command_list[4];
    assert_eq!(blend.pass as u32, edl_ref["blendPass"].as_u64().unwrap() as u32);
    assert_eq!(blend.metadata.bounding_volume, Some(bv));
    let blend_opts = blend.draw_options::<RecordingGl>().unwrap();
    assert_eq!(blend_opts.program.source_options().unwrap().fragment_shader_source.defines, vec!["LOG_DEPTH_WRITE"]);
    let exp_dist = f64s(&edl_ref["blendDistanceAndStrength"]);
    let u_val = &blend_opts.uniforms[0].1;
    match u_val {
        cesium_rs::renderer::UniformValue::Float(v) => {
            assert_close(f64::from(v[0]), exp_dist[0], 1e-5, "edl radius");
            assert_close(f64::from(v[1]), exp_dist[1], 1e-5, "edl strength");
        }
        other => panic!("unexpected EDL uniform: {other:?}"),
    }

    // [5]: EDL clear command
    let clear = &frame.command_list[5];
    assert_eq!(clear.pass as u32, edl_ref["clearPass"].as_u64().unwrap() as u32);
    assert_eq!(edl.color_g_buffer().unwrap().width(), edl_ref["framebufferDims"][0].as_u64().unwrap() as u32);
    assert_eq!(edl.color_g_buffer().unwrap().height(), edl_ref["framebufferDims"][1].as_u64().unwrap() as u32);
    assert!(edl.depth_g_buffer().is_some());

    edl.destroy().unwrap();
    assert_eq!(edl.is_destroyed(), edl_ref["destroyed"].as_bool().unwrap());
    let _ = pre_cloud.destroy();
    let _ = opaque_cloud.destroy();
}

#[test]
fn time_dynamic_point_cloud_lifecycle_memory_and_edl_match_cesium() {
    let f = fixture();
    let td_ref = &f["timeDynamicPointCloud"];
    let assets = &f["assets"];
    let context = Context::new_webgl2(RecordingGl::new([32, 32], 2048));

    let base = "https://example.test/td";
    let mut routes = HashMap::new();
    routes.insert(format!("{base}/frame0.pnts"), Ok(decode_b64(assets["tdFrame0Base64"].as_str().unwrap())));
    routes.insert(format!("{base}/frame1.pnts"), Ok(decode_b64(assets["tdFrame1Base64"].as_str().unwrap())));
    routes.insert(format!("{base}/frame2.pnts"), Ok(decode_b64(assets["tdFrame2Base64"].as_str().unwrap())));
    routes.insert(format!("{base}/missing.pnts"), Err("404 missing".into()));
    let client = ResourceClient::new(Rc::new(MockTransport { routes }));

    let t0 = JulianDate::from_iso8601("2025-01-01T00:00:00Z").unwrap();
    let t1 = t0.add_seconds(5.0);
    let t2 = t0.add_seconds(10.0);
    let t3 = t0.add_seconds(15.0);
    let t4 = t0.add_seconds(20.0);
    let mut intervals = TimeIntervalCollection::new();
    for (start, stop, uri, transform) in [
        (t0, t1, format!("{base}/frame0.pnts"), Some(Matrix4::from_translation(Cartesian3::new(10.0, 0.0, 0.0)))),
        (t1, t2, format!("{base}/frame1.pnts"), Some(Matrix4::from_translation(Cartesian3::new(50.0, 0.0, 0.0)))),
        (t2, t3, format!("{base}/frame2.pnts"), None),
        (t3, t4, format!("{base}/missing.pnts"), None),
    ] {
        intervals.add_interval(TimeInterval {
            start,
            stop,
            is_start_included: true,
            is_stop_included: false,
            data: Some(TimeDynamicPointCloudInterval { uri, transform }),
        });
    }

    let clock = Clock::new_at(
        ClockOptions {
            start_time: Some(t0),
            stop_time: Some(t4),
            current_time: Some(t0),
            clock_range: Some(ClockRange::Clamped),
            multiplier: Some(1.0),
            should_animate: Some(true),
            can_animate: Some(true),
            ..Default::default()
        },
        get_timestamp(),
    )
    .unwrap();

    let mut td = TimeDynamicPointCloud::<RecordingGl>::new(TimeDynamicPointCloudOptions {
        clock,
        intervals,
        show: true,
        model_matrix: Matrix4::IDENTITY,
        shadows: ShadowMode::Enabled,
        maximum_memory_usage: 0.0002,
        shading: PointCloudShadingOptions {
            attenuation: Some(true),
            eye_dome_lighting: Some(true),
            eye_dome_lighting_strength: Some(1.0),
            eye_dome_lighting_radius: Some(1.0),
            ..Default::default()
        },
        clipping_planes: Some(Rc::new(RefCell::new(ClippingPlaneCollection::new(vec![Rc::new(RefCell::new(
            ClippingPlane::new(Cartesian3::UNIT_X, 100.0),
        ))])))),
    });
    td.set_resource_client(client);

    let changed = Arc::new(Mutex::new(0usize));
    let failed = Arc::new(Mutex::new(0usize));
    {
        let c = changed.clone();
        td.frame_changed.add_event_listener(move |_| *c.lock().unwrap() += 1);
        let fa = failed.clone();
        td.frame_failed.add_event_listener(move |_| *fa.lock().unwrap() += 1);
    }

    let mut frame_num = 1u32;
    let mut step = |td: &mut TimeDynamicPointCloud<RecordingGl>, time: JulianDate, mode: SceneMode| -> FrameState {
        let mut fr = make_frame(&context, [32, 32], time);
        fr.frame_number = frame_num;
        frame_num += 1;
        fr.mode = mode;
        td.update(&mut fr).unwrap();
        fr
    };

    // Frame 0: initial update starts fetch + resolves on mock transport; second update renders + invokes EDL
    let _ = step(&mut td, t0, SceneMode::Scene3D);
    let f0 = step(&mut td, t0, SceneMode::Scene3D);
    assert_eq!(f0.command_list.len() as u64, td_ref["afterFrame0"]["commandListLength"].as_u64().unwrap());
    assert!(td.total_memory_usage_in_bytes() > 0);
    assert!(*changed.lock().unwrap() >= 1);

    // Frame 1
    let cur1 = t1.add_seconds(1.0);
    td.clock_mut().set_current_time(cur1);
    let _ = step(&mut td, cur1, SceneMode::Scene3D);
    let f1 = step(&mut td, cur1, SceneMode::Scene3D);
    assert_eq!(f1.command_list.len() as u64, td_ref["afterFrame1"]["commandListLength"].as_u64().unwrap());

    // Frame 2: triggers memory budget eviction of older frames
    let cur2 = t2.add_seconds(1.0);
    td.clock_mut().set_current_time(cur2);
    let _ = step(&mut td, cur2, SceneMode::Scene3D);
    let f2 = step(&mut td, cur2, SceneMode::Scene3D);
    assert_eq!(f2.command_list.len() as u64, td_ref["afterEviction"]["commandListLength"].as_u64().unwrap());
    assert!(td.total_memory_usage_in_bytes() <= 250, "memory budget eviction should keep usage small");

    // Hidden & morphing suppress draw commands
    td.show = false;
    let f_hidden = step(&mut td, cur2, SceneMode::Scene3D);
    assert_eq!(f_hidden.command_list.len() as u64, td_ref["hiddenCmds"].as_u64().unwrap());
    td.show = true;
    let f_morph = step(&mut td, cur2, SceneMode::Morphing);
    assert_eq!(f_morph.command_list.len() as u64, td_ref["morphingCmds"].as_u64().unwrap());

    // Missing frame triggers frame_failed event
    let cur3 = t3.add_seconds(1.0);
    td.clock_mut().set_current_time(cur3);
    let _ = step(&mut td, cur3, SceneMode::Scene3D);
    let _ = step(&mut td, cur3, SceneMode::Scene3D);
    assert_eq!(*failed.lock().unwrap() as u64, td_ref["failuresCount"].as_u64().unwrap());

    td.destroy().unwrap();
    assert_eq!(td.is_destroyed(), td_ref["destroyed"].as_bool().unwrap());
}
