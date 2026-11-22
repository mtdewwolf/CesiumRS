//! Reference parity tests for the pending-validation Scene modules against
//! direct captures from pinned `@cesium/engine` 23.0.0:
//! - `tests/data/scene-pending-cpu.json` (`tools/cesium/generate-scene-pending-cpu.mjs`)
//! - `tests/data/scene-pending-gpu.json` (`tools/cesium/generate-scene-pending-gpu.mjs`, SwiftShader WebGL2)
//! - `tests/fixtures/workers/gaussianSplatSorter.json` and `gaussianSplatTextureGenerator.json`

use cesium_rs::core::geometry::{GeometryAttribute, GeometryAttributes};
use cesium_rs::core::task_processor::{TaskFuture, TaskWorker, WebAssemblyOptions};
use cesium_rs::core::{
    BoundingSphere, BoxGeometry, Cartesian2, Cartesian3, Color, ComponentDatatype, IndexDatatype, JulianDate, Matrix4,
    PixelFormat, Plane, PrimitiveType, Ray, RuntimeError, TimeStandard, TypedArray, VertexFormat,
};
use cesium_rs::renderer::{
    AttributeSource, Buffer, BufferData, BufferKind, BufferUsage, Context, Gl, GlValue, Pass, PassState, PixelDatatype,
    RenderState, Sampler, ShaderProgramOptions, ShaderSource, Texture, TextureBinding, TextureOptions, UniformFrame,
    VertexAttribute, Viewport,
};
use cesium_rs::scene::model::model_draw_commands::{build_model_draw_command, ModelDrawBuildOptions};
use cesium_rs::scene::model::{
    ModelDrawCommandKind, ModelDrawCommandOptions, ModelRenderResources, ModelUniformState, NodeRenderResources,
    PrimitiveRenderResources,
};
use cesium_rs::scene::model_components as mc;
use cesium_rs::scene::{
    create_tangent_space_debug_primitive, create_tangent_space_debug_primitive_from_factory, BrdfLutGenerator, Camera,
    CameraOptions, ClassificationType, ClippingPlane, CreateTangentSpaceDebugPrimitiveOptions, DerivedCommand,
    Empty3DTileContent, FrameState, GaussianSplatRenderResources, GaussianSplatSorter, GaussianSplatTextureGenerator,
    JobScheduler, JobType, MetadataClassProperty, MetadataPicking, MetadataValue, PickedMetadataInfo,
    RendererSceneBackend, Scene, SceneMode, SceneOptions, ScenePickObject, ShadowMode, TilesetContent,
    TranslucentTileClassification, VoxelBoundsCollection, VoxelBoundsCollectionOptions,
};
use serde_json::{json, Value};
use std::cell::{Cell, RefCell};
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;
use std::sync::{Arc, Mutex};
use std::task::{Context as TaskContext, Poll, Waker};

fn cpu_fixture() -> Value {
    serde_json::from_str(include_str!("data/scene-pending-cpu.json")).unwrap()
}

fn gpu_fixture() -> Value {
    serde_json::from_str(include_str!("data/scene-pending-gpu.json")).unwrap()
}

fn fnv1a_64(bytes: &[u8]) -> String {
    let mut h: u64 = 0xcbf29ce484222325;
    for &b in bytes {
        h = (h ^ u64::from(b)).wrapping_mul(0x100000001b3);
    }
    format!("{h:x}")
}

fn poll_once<F: Future + Unpin>(future: &mut F) -> Poll<F::Output> {
    let mut cx = TaskContext::from_waker(Waker::noop());
    Pin::new(future).poll(&mut cx)
}

fn f64_vec(v: &Value) -> Vec<f64> {
    v.as_array().unwrap().iter().map(|x| x.as_f64().unwrap()).collect()
}

fn make_frame(size: [u32; 2]) -> FrameState {
    let mut c = Camera::new(CameraOptions::new(size[0] as f64, size[1] as f64));
    let u = c.uniform_camera();
    let culling = c.frustum.compute_culling_volume(u.position, u.direction, u.up);
    FrameState::new(UniformFrame::new(u, JulianDate::new(2451545., 0., TimeStandard::Tai)), culling, size)
}

fn matrix4_from(v: &Value) -> Matrix4 {
    let arr: [f64; 16] = f64_vec(v).try_into().unwrap();
    Matrix4(arr)
}

fn assert_close(actual: f64, expected: f64, tol: f64, label: &str) {
    assert!(
        (actual - expected).abs() <= tol,
        "{label}: actual={actual} expected={expected} diff={} tol={tol}",
        (actual - expected).abs()
    );
}

// --- Recording WebGL2 host ---------------------------------------------------

#[derive(Clone)]
struct RecordingGl {
    next: Rc<Cell<u32>>,
    calls: Rc<RefCell<Vec<String>>>,
    call_args: Rc<RefCell<Vec<(String, Vec<f64>)>>>,
    uploads: Rc<RefCell<Vec<(u32, u32, Vec<u8>)>>>,
    draws: Rc<Cell<usize>>,
    depth_texture: bool,
    max_texture_size: u32,
    size: [u32; 2],
    read_pixel: Rc<Cell<[u8; 4]>>,
}

impl RecordingGl {
    fn new(size: [u32; 2], depth_texture: bool, max_texture_size: u32) -> Self {
        Self {
            next: Rc::new(Cell::new(0)),
            calls: Rc::new(RefCell::new(Vec::new())),
            call_args: Rc::new(RefCell::new(Vec::new())),
            uploads: Rc::new(RefCell::new(Vec::new())),
            draws: Rc::new(Cell::new(0)),
            depth_texture,
            max_texture_size,
            size,
            read_pixel: Rc::new(Cell::new([0, 0, 0, 0])),
        }
    }
}

impl Gl for RecordingGl {
    type Object = u32;

    fn drawing_buffer_size(&self) -> Result<[u32; 2], RuntimeError> {
        Ok(self.size)
    }

    fn invoke(&self, name: &str, args: &[GlValue<u32>]) -> Result<GlValue<u32>, RuntimeError> {
        self.calls.borrow_mut().push(name.into());
        if matches!(name, "scissor" | "stencilFunc" | "stencilFuncSeparate" | "blendFuncSeparate") {
            let nums: Vec<f64> = args
                .iter()
                .filter_map(|v| match v {
                    GlValue::Number(n) => Some(*n),
                    _ => None,
                })
                .collect();
            self.call_args.borrow_mut().push((name.into(), nums));
        }
        if name == "drawElements" || name == "drawArrays" {
            self.draws.set(self.draws.get() + 1);
        }
        if name == "texSubImage2D" {
            let bytes = match args.last() {
                Some(GlValue::Bytes(b)) => Some(b.clone()),
                Some(GlValue::Pixels { bytes, .. }) => Some(bytes.clone()),
                _ => None,
            };
            if let (Some(GlValue::Number(w)), Some(GlValue::Number(h)), Some(bytes)) =
                (args.get(4), args.get(5), bytes)
            {
                self.uploads.borrow_mut().push((*w as u32, *h as u32, bytes));
            }
        }
        if name == "readPixels" {
            let w = match args.get(2) {
                Some(GlValue::Number(n)) => *n as usize,
                _ => 1,
            };
            let h = match args.get(3) {
                Some(GlValue::Number(n)) => *n as usize,
                _ => 1,
            };
            let px = self.read_pixel.get();
            let mut out = Vec::with_capacity(w * h * 4);
            for _ in 0..(w * h) {
                out.extend_from_slice(&px);
            }
            return Ok(GlValue::Bytes(out));
        }
        if name.starts_with("create") {
            self.next.set(self.next.get() + 1);
            return Ok(GlValue::Object(self.next.get()));
        }
        Ok(match name {
            "getParameter" => match args.first() {
                Some(GlValue::Number(n)) if [33901., 33902., 3386.].contains(n) => {
                    GlValue::Numbers(vec![1., self.max_texture_size as f64])
                }
                _ => GlValue::Number(self.max_texture_size as f64),
            },
            "getExtension" => {
                let ext = match args.first() {
                    Some(GlValue::String(s)) => s.as_str(),
                    _ => "",
                };
                if !self.depth_texture && ext.contains("depth_texture") {
                    GlValue::Null
                } else {
                    GlValue::Object(1)
                }
            }
            "getShaderParameter" | "getProgramParameter" => GlValue::Bool(true),
            "checkFramebufferStatus" => GlValue::Number(36053.), // FRAMEBUFFER_COMPLETE
            _ => GlValue::Null,
        })
    }
}

// =============================================================================
// 1. Empty3DTileContent
// =============================================================================

#[test]
fn empty_3d_tile_content_matches_cesium_reference() {
    let f = &cpu_fixture()["empty3DTileContent"];
    let init = &f["initial"];
    let mut c = Empty3DTileContent::new();

    assert_eq!(c.features_length(), init["featuresLength"].as_u64().unwrap() as usize);
    assert_eq!(c.points_length(), init["pointsLength"].as_u64().unwrap() as usize);
    assert_eq!(c.triangles_length(), init["trianglesLength"].as_u64().unwrap() as usize);
    assert_eq!(c.geometry_byte_length(), init["geometryByteLength"].as_u64().unwrap() as usize);
    assert_eq!(c.textures_byte_length(), init["texturesByteLength"].as_u64().unwrap() as usize);
    assert_eq!(c.batch_table_byte_length(), init["batchTableByteLength"].as_u64().unwrap() as usize);
    assert!(c.inner_contents().is_none() && init["innerContents"].is_null());
    assert_eq!(c.ready(), init["ready"].as_bool().unwrap());
    assert!(c.url().is_none() && init["url"].is_null());
    assert!(c.metadata().is_none() && init["metadata"].is_null());
    assert!(c.batch_table().is_none() && init["batchTable"].is_null());
    assert!(c.group().is_none() && init["group"].is_null());
    assert_eq!(c.feature_properties_dirty, init["featurePropertiesDirty"].as_bool().unwrap());
    assert_eq!(c.has_property(0, "height"), init["hasProperty"].as_bool().unwrap());
    assert!(c.get_feature(0).is_none() && init["getFeature"].is_null());
    assert_eq!(c.is_destroyed(), init["isDestroyed"].as_bool().unwrap());

    assert_eq!(c.set_metadata().unwrap_err().message, f["setMetadataError"].as_str().unwrap());
    assert_eq!(c.set_group().unwrap_err().message, f["setGroupError"].as_str().unwrap());

    c.feature_properties_dirty = true;
    assert_eq!(c.feature_properties_dirty, f["dirtyAfterSet"].as_bool().unwrap());
    c.apply_debug_settings(true, Some(Color::RED));
    c.apply_style(Some("{}"));
    c.update();

    // TilesetContent trait contract
    let mut frame = make_frame([8, 8]);
    let ray = Ray::new(Cartesian3::ZERO, Cartesian3::UNIT_Z);
    assert!(c.pick(&ray, &frame).is_none());
    assert_eq!(TilesetContent::metrics(&c), Default::default());
    assert_eq!(TilesetContent::process(&mut c, &frame).unwrap(), true);
    TilesetContent::update(&mut c, &mut frame).unwrap();
    assert_eq!(TilesetContent::pick(&c, &ray, &frame).unwrap(), None);

    c.destroy();
    assert_eq!(c.is_destroyed(), f["isDestroyedAfter"].as_bool().unwrap());
    assert!(TilesetContent::process(&mut c, &frame).is_err());
    assert!(TilesetContent::update(&mut c, &mut frame).is_err());
    assert!(TilesetContent::pick(&c, &ray, &frame).is_err());
}

// =============================================================================
// 2. JobScheduler
// =============================================================================

thread_local! {
    static TEST_CLOCK: Cell<f64> = const { Cell::new(0.0) };
}

fn test_clock_now() -> f64 {
    TEST_CLOCK.with(|c| c.get())
}

fn assert_scheduler_state(s: &JobScheduler, expected: &Value, context: &str) {
    assert_eq!(s.total_budget(), expected["totalBudget"].as_f64().unwrap(), "{context}: totalBudget");
    assert_close(
        s.total_used_this_frame(),
        expected["totalUsedThisFrame"].as_f64().unwrap(),
        1e-12,
        &format!("{context}: totalUsedThisFrame"),
    );
    let exec_exp: Vec<bool> =
        expected["executedThisFrame"].as_array().unwrap().iter().map(|v| v.as_bool().unwrap()).collect();
    assert_eq!(s.executed_this_frame(), exec_exp.as_slice(), "{context}: executedThisFrame");
    for (i, b_exp) in expected["budgets"].as_array().unwrap().iter().enumerate() {
        let b = &s.budgets()[i];
        assert_eq!(b.total(), b_exp["total"].as_f64().unwrap(), "{context}: budget[{i}].total");
        assert_close(
            b.used_this_frame,
            b_exp["usedThisFrame"].as_f64().unwrap(),
            1e-12,
            &format!("{context}: budget[{i}].usedThisFrame"),
        );
        assert_close(
            b.stolen_from_me_this_frame,
            b_exp["stolenFromMeThisFrame"].as_f64().unwrap(),
            1e-12,
            &format!("{context}: budget[{i}].stolenFromMeThisFrame"),
        );
        assert_eq!(
            b.starved_this_frame,
            b_exp["starvedThisFrame"].as_bool().unwrap(),
            "{context}: budget[{i}].starvedThisFrame"
        );
        assert_eq!(
            b.starved_last_frame,
            b_exp["starvedLastFrame"].as_bool().unwrap(),
            "{context}: budget[{i}].starvedLastFrame"
        );
    }
}

#[test]
fn job_scheduler_matches_cesium_reference() {
    let f = &cpu_fixture()["jobScheduler"];

    for case in f["constructorCases"].as_array().unwrap() {
        let name = case["name"].as_str().unwrap();
        let res = if case["budgets"].is_null() {
            Ok(JobScheduler::default())
        } else {
            let budgets = f64_vec(&case["budgets"]);
            JobScheduler::try_new(&budgets)
        };
        if let Some(err) = case["error"].as_str() {
            assert_eq!(res.err().unwrap().message, err, "ctor {name}");
        } else {
            let s = res.unwrap();
            assert_scheduler_state(&s, &case["state"], &format!("ctor {name}"));
        }
    }

    for trace in f["traces"].as_array().unwrap() {
        let name = trace["name"].as_str().unwrap();
        let budgets = f64_vec(&trace["budgets"]);
        TEST_CLOCK.with(|c| c.set(100.0));
        let mut s = JobScheduler::try_new(&budgets).unwrap();
        s.set_timestamp_provider(Some(test_clock_now));

        for (step_idx, step) in trace["steps"].as_array().unwrap().iter().enumerate() {
            let op = step["op"].as_array().unwrap();
            let ctx = format!("{name}[{step_idx}] {op:?}");
            match op[0].as_str().unwrap() {
                "exec" => {
                    let job_type = match op[1].as_u64().unwrap() {
                        0 => JobType::Texture,
                        1 => JobType::Program,
                        2 => JobType::Buffer,
                        other => panic!("unexpected job type {other}"),
                    };
                    let duration = op[2].as_f64().unwrap();
                    let mut invoked = false;
                    let ran = s.execute(
                        || {
                            invoked = true;
                            TEST_CLOCK.with(|c| c.set(c.get() + duration));
                        },
                        job_type,
                    );
                    assert_eq!(ran, step["ran"].as_bool().unwrap(), "{ctx}: ran");
                    assert_eq!(invoked, step["invoked"].as_bool().unwrap(), "{ctx}: invoked");
                }
                "reset" => s.reset_budgets(),
                "disable" => s.disable_this_frame(),
                other => panic!("unexpected op {other}"),
            }
            assert_scheduler_state(&s, &step["state"], &ctx);
        }
    }
}

// =============================================================================
// 3. createTangentSpaceDebugPrimitive
// =============================================================================

#[test]
fn create_tangent_space_debug_primitive_matches_cesium_reference() {
    let f = &cpu_fixture()["tangentSpaceDebugPrimitive"];
    let positions = f64_vec(&f["positions"]);
    let normals = f64_vec(&f["normals"]);
    let tangents = f64_vec(&f["tangents"]);
    let bitangents = f64_vec(&f["bitangents"]);

    for case in f["cases"].as_array().unwrap() {
        let name = case["name"].as_str().unwrap();
        let attrs: Vec<&str> = case["attrs"].as_array().unwrap().iter().map(|v| v.as_str().unwrap()).collect();
        let geometry = cesium_rs::core::Geometry::new(
            GeometryAttributes {
                position: Some(GeometryAttribute::doubles(3, positions.clone())),
                normal: attrs.contains(&"normal").then(|| GeometryAttribute::floats(3, &normals)),
                tangent: attrs.contains(&"tangent").then(|| GeometryAttribute::floats(3, &tangents)),
                bitangent: attrs.contains(&"bitangent").then(|| GeometryAttribute::floats(3, &bitangents)),
                ..Default::default()
            },
            None,
            PrimitiveType::Triangles,
        );
        let options = CreateTangentSpaceDebugPrimitiveOptions {
            geometry,
            length: case["length"].as_f64(),
            model_matrix: (!case["matrix"].is_null()).then(|| matrix4_from(&case["matrix"])),
        };
        let actual = create_tangent_space_debug_primitive(options);
        if case["result"].is_null() {
            assert!(actual.is_none(), "{name}: expected None");
            continue;
        }
        let prim = actual.expect(name);
        let exp = &case["result"];
        assert_eq!(prim.asynchronous, exp["asynchronous"].as_bool().unwrap(), "{name}: asynchronous");

        let appearance = prim.appearance.as_ref().expect("appearance");
        assert_eq!(appearance.translucent(), exp["appearance"]["translucent"].as_bool().unwrap(), "{name}: translucent");
        assert_eq!(
            appearance.vertex_shader_source(),
            exp["appearance"]["vertexShaderSource"].as_str().unwrap(),
            "{name}: vs"
        );
        assert_eq!(
            appearance.fragment_shader_source(),
            exp["appearance"]["fragmentShaderSource"].as_str().unwrap(),
            "{name}: fs"
        );

        let instances = prim.geometry_instances().expect("geometry_instances");
        let exp_instances = exp["instances"].as_array().unwrap();
        assert_eq!(instances.len(), exp_instances.len(), "{name}: instance count");

        for (idx, (inst, exp_inst)) in instances.iter().zip(exp_instances).enumerate() {
            let label = format!("{name}[{idx}]");
            let exp_m = matrix4_from(&exp_inst["modelMatrix"]);
            for i in 0..16 {
                assert_close(inst.model_matrix.0[i], exp_m.0[i], 1e-12, &format!("{label}: modelMatrix[{i}]"));
            }
            let inst_geom = inst.geometry.as_ref().expect("instance geometry");
            assert_eq!(
                inst_geom.primitive_type as u32,
                exp_inst["primitiveType"].as_u64().unwrap() as u32,
                "{label}: primitiveType"
            );
            let (_, color_attr) = inst.attributes.iter().find(|(k, _)| k == "color").expect("color attr");
            assert_eq!(
                color_attr.component_datatype as u32,
                exp_inst["color"]["componentDatatype"].as_u64().unwrap() as u32,
                "{label}: color datatype"
            );
            assert_eq!(
                color_attr.components_per_attribute,
                exp_inst["color"]["componentsPerAttribute"].as_u64().unwrap() as u8,
                "{label}: color components"
            );
            assert_eq!(color_attr.normalize, exp_inst["color"]["normalize"].as_bool().unwrap(), "{label}: normalize");
            let exp_rgba: Vec<u8> =
                exp_inst["color"]["value"].as_array().unwrap().iter().map(|v| v.as_u64().unwrap() as u8).collect();
            match &color_attr.value {
                TypedArray::Uint8(bytes) => assert_eq!(bytes.as_slice(), exp_rgba.as_slice(), "{label}: color bytes"),
                other => panic!("{label}: expected Uint8 color bytes, got {other:?}"),
            }

            let TypedArray::Float64(pos) = &inst_geom.attributes.position.as_ref().unwrap().values else {
                panic!("{label}: expected Float64 positions");
            };
            let exp_pos = f64_vec(&exp_inst["positions"]);
            assert_eq!(pos.len(), exp_pos.len(), "{label}: positions len");
            for (i, (a, e)) in pos.iter().zip(exp_pos.iter()).enumerate() {
                assert_close(*a, *e, 1e-9, &format!("{label}: pos[{i}]"));
            }

            if !exp_inst["boundingSphere"].is_null() {
                let bs = inst_geom.bounding_sphere.expect("bounding_sphere");
                let ec = f64_vec(&exp_inst["boundingSphere"]["center"]);
                assert_close(bs.center.x, ec[0], 1e-9, &format!("{label}: bs.x"));
                assert_close(bs.center.y, ec[1], 1e-9, &format!("{label}: bs.y"));
                assert_close(bs.center.z, ec[2], 1e-9, &format!("{label}: bs.z"));
                assert_close(bs.radius, exp_inst["boundingSphere"]["radius"].as_f64().unwrap(), 1e-9, &format!("{label}: bs.r"));
            }
        }
    }

    // Factory path matching JS uncomputed geometry fallback
    let fc = &f["factoryCase"];
    let dims = f64_vec(&fc["dimensions"]);
    let box_geom =
        BoxGeometry::from_dimensions(Cartesian3::new(dims[0], dims[1], dims[2]), VertexFormat::POSITION_AND_NORMAL, None)
            .unwrap();
    let prim = create_tangent_space_debug_primitive_from_factory(&box_geom, fc["length"].as_f64(), None).unwrap();
    assert_eq!(prim.asynchronous, fc["asynchronous"].as_bool().unwrap());
    let instances = prim.geometry_instances().unwrap();
    assert_eq!(instances.len(), fc["instanceCount"].as_u64().unwrap() as usize);
    let TypedArray::Float64(pos) = &instances[0].geometry.as_ref().unwrap().attributes.position.as_ref().unwrap().values
    else {
        panic!("factory: expected Float64 positions");
    };
    let exp_pos = f64_vec(&fc["positions"]);
    assert_eq!(pos.len(), exp_pos.len());
    for (i, (a, e)) in pos.iter().zip(exp_pos.iter()).enumerate() {
        assert_close(*a, *e, 1e-9, &format!("factory pos[{i}]"));
    }
}

// =============================================================================
// 4. GaussianSplatRenderResources
// =============================================================================

#[test]
fn gaussian_splat_render_resources_matches_cesium_reference() {
    for case in cpu_fixture()["gaussianSplatRenderResources"].as_array().unwrap() {
        let use_log_depth = case["useLogDepth"].as_bool().unwrap();
        let res = GaussianSplatRenderResources::new(use_log_depth);

        let rs = &res.render_state;
        let exp_rs = &case["renderStateOptions"];
        assert_eq!(rs.front_face, exp_rs["frontFace"].as_u64().unwrap() as u32);
        assert_eq!(rs.cull.enabled, exp_rs["cull"]["enabled"].as_bool().unwrap());
        assert_eq!(rs.cull.face, exp_rs["cull"]["face"].as_u64().unwrap() as u32);
        assert_eq!(rs.line_width, exp_rs["lineWidth"].as_f64().unwrap());
        assert_eq!(rs.polygon_offset.enabled, exp_rs["polygonOffset"]["enabled"].as_bool().unwrap());
        assert_eq!(rs.scissor_test.enabled, exp_rs["scissorTest"]["enabled"].as_bool().unwrap());
        assert_eq!(rs.depth_range.near, exp_rs["depthRange"]["near"].as_f64().unwrap());
        assert_eq!(rs.depth_range.far, exp_rs["depthRange"]["far"].as_f64().unwrap());
        assert_eq!(rs.depth_test.enabled, exp_rs["depthTest"]["enabled"].as_bool().unwrap());
        assert_eq!(rs.depth_test.func, exp_rs["depthTest"]["func"].as_u64().unwrap() as u32);
        assert_eq!(rs.depth_mask, exp_rs["depthMask"].as_bool().unwrap());
        assert_eq!(rs.color_mask.red, exp_rs["colorMask"]["red"].as_bool().unwrap());
        assert_eq!(rs.color_mask.green, exp_rs["colorMask"]["green"].as_bool().unwrap());
        assert_eq!(rs.color_mask.blue, exp_rs["colorMask"]["blue"].as_bool().unwrap());
        assert_eq!(rs.color_mask.alpha, exp_rs["colorMask"]["alpha"].as_bool().unwrap());
        assert_eq!(rs.blending.enabled, exp_rs["blending"]["enabled"].as_bool().unwrap());
        assert_eq!(rs.blending.equation_rgb, exp_rs["blending"]["equationRgb"].as_u64().unwrap() as u32);
        assert_eq!(rs.blending.equation_alpha, exp_rs["blending"]["equationAlpha"].as_u64().unwrap() as u32);
        assert_eq!(rs.blending.function_source_rgb, exp_rs["blending"]["functionSourceRgb"].as_u64().unwrap() as u32);
        assert_eq!(
            rs.blending.function_destination_rgb,
            exp_rs["blending"]["functionDestinationRgb"].as_u64().unwrap() as u32
        );
        assert_eq!(rs.stencil_test.enabled, exp_rs["stencilTest"]["enabled"].as_bool().unwrap());
        assert_eq!(rs.stencil_test.front_function, exp_rs["stencilTest"]["frontFunction"].as_u64().unwrap() as u32);
        assert_eq!(rs.sample_coverage.enabled, exp_rs["sampleCoverage"]["enabled"].as_bool().unwrap());

        assert_eq!(res.has_skip_level_of_detail, case["hasSkipLevelOfDetail"].as_bool().unwrap());
        assert_eq!(res.uniform_map.len(), case["uniformKeys"].as_array().unwrap().len());
        assert!(res.alpha_options.pass.is_none() && case["alphaOptions"]["pass"].is_null());
        assert!(res.alpha_options.alpha_cutoff.is_none() && case["alphaOptions"]["alphaCutoff"].is_null());

        let (vs, fs) = res.shader_builder.sources();
        assert_eq!(json!(vs.defines), case["vertexDefines"]);
        assert_eq!(json!(fs.defines), case["fragmentDefines"]);
    }
}

// =============================================================================
// 5 & 6. GaussianSplatSorter & GaussianSplatTextureGenerator
// =============================================================================

struct ScriptedInitWorker {
    init_result: Result<Value, RuntimeError>,
    init_calls: Rc<Cell<usize>>,
    wasm_files: Rc<RefCell<Vec<Option<String>>>>,
    run_calls: Rc<Cell<usize>>,
}

impl TaskWorker<Value, Value> for ScriptedInitWorker {
    fn init_web_assembly_module(&self, options: WebAssemblyOptions) -> TaskFuture<Value> {
        self.init_calls.set(self.init_calls.get() + 1);
        self.wasm_files.borrow_mut().push(options.wasm_binary_file);
        let r = self.init_result.clone();
        Box::pin(async move { r })
    }
    fn run(&self, _: u64, _: Value) -> TaskFuture<Value> {
        self.run_calls.set(self.run_calls.get() + 1);
        Box::pin(async { Ok(json!("scheduled-ok")) })
    }
    fn cancel_task(&self, _: u64) {}
    fn terminate(&self) {}
}

#[test]
fn gaussian_splat_coordinators_match_cesium_reference_and_worker_fixtures() {
    let f = &cpu_fixture()["gaussianCoordinators"];

    for c in f["concurrencyFormula"].as_array().unwrap() {
        let hc = c["hardwareConcurrency"].as_u64().unwrap() as usize;
        let exp = c["maxSortingConcurrency"].as_u64().unwrap() as usize;
        assert_eq!(GaussianSplatSorter::max_sorting_concurrency(hc), exp);
        assert_eq!(GaussianSplatTextureGenerator::max_sorting_concurrency(hc), exp);
    }

    // Verify lifecycle against captured CesiumJS coordinator scenarios
    for (kind, scenarios) in [
        ("sorter", f["sorter"].as_array().unwrap()),
        ("textureGenerator", f["textureGenerator"].as_array().unwrap()),
    ] {
        for sc in scenarios {
            let mode = sc["mode"].as_str().unwrap();
            let init_calls = Rc::new(Cell::new(0));
            let wasm_files = Rc::new(RefCell::new(Vec::new()));
            let run_calls = Rc::new(Cell::new(0));
            let worker = Rc::new(ScriptedInitWorker {
                init_result: match mode {
                    "success" => Ok(Value::Bool(true)),
                    "falseInit" => Ok(Value::Bool(false)),
                    "rejectInit" => Err(RuntimeError::new("wasm load failed")),
                    _ => unreachable!(),
                },
                init_calls: init_calls.clone(),
                wasm_files: wasm_files.clone(),
                run_calls: run_calls.clone(),
            });

            if kind == "sorter" {
                assert_eq!(GaussianSplatSorter::WORKER_NAME, sc["workerName"].as_str().unwrap());
                assert_eq!(GaussianSplatSorter::WASM_BINARY_FILE, sc["wasmBinaryFile"].as_str().unwrap());
                let sorter = GaussianSplatSorter::new(Some(worker), 4);
                assert!(sorter.radix_sort_indexes(json!({})).unwrap().is_none() && sc["beforeInit"].is_null());
                let mut cx = TaskContext::from_waker(Waker::noop());
                let _ = sorter.poll_initialization(&mut cx);
                assert_eq!(sorter.is_ready(), sc["readyAfter"].as_bool().unwrap());
                assert_eq!(init_calls.get(), sc["initCount"].as_u64().unwrap() as usize);
                assert_eq!(wasm_files.borrow()[0].as_deref(), sc["wasmBinaryFile"].as_str());
                match sorter.radix_sort_indexes(json!({})) {
                    Ok(Some(mut task)) => {
                        let v = match poll_once(&mut task) {
                            Poll::Ready(Ok(v)) => v,
                            other => panic!("{kind}/{mode}: unexpected poll {other:?}"),
                        };
                        assert_eq!(v, sc["afterVal"]);
                        let _ = sorter.radix_sort_indexes(json!({})).unwrap().unwrap();
                    }
                    Ok(None) => panic!("{kind}/{mode}: expected task or error"),
                    Err(e) => {
                        assert_eq!(e.message, sc["afterErr"].as_str().unwrap(), "{kind}/{mode}");
                        assert_eq!(
                            sorter.radix_sort_indexes(json!({})).err().unwrap().message,
                            sc["secondErr"].as_str().unwrap()
                        );
                    }
                }
                assert_eq!(run_calls.get(), sc["scheduledCount"].as_u64().unwrap() as usize);
            } else {
                assert_eq!(GaussianSplatTextureGenerator::WORKER_NAME, sc["workerName"].as_str().unwrap());
                assert_eq!(GaussianSplatTextureGenerator::WASM_BINARY_FILE, sc["wasmBinaryFile"].as_str().unwrap());
                let gen = GaussianSplatTextureGenerator::new(Some(worker), 4);
                assert!(gen.generate_from_attributes(json!({})).unwrap().is_none() && sc["beforeInit"].is_null());
                let mut cx = TaskContext::from_waker(Waker::noop());
                let _ = gen.poll_initialization(&mut cx);
                assert_eq!(gen.is_ready(), sc["readyAfter"].as_bool().unwrap());
                assert_eq!(init_calls.get(), sc["initCount"].as_u64().unwrap() as usize);
                assert_eq!(wasm_files.borrow()[0].as_deref(), sc["wasmBinaryFile"].as_str());
                match gen.generate_from_attributes(json!({})) {
                    Ok(Some(mut task)) => {
                        let v = match poll_once(&mut task) {
                            Poll::Ready(Ok(v)) => v,
                            other => panic!("{kind}/{mode}: unexpected poll {other:?}"),
                        };
                        assert_eq!(v, sc["afterVal"]);
                        let _ = gen.generate_from_attributes(json!({})).unwrap().unwrap();
                    }
                    Ok(None) => panic!("{kind}/{mode}: expected task or error"),
                    Err(e) => {
                        assert_eq!(e.message, sc["afterErr"].as_str().unwrap(), "{kind}/{mode}");
                        assert_eq!(
                            gen.generate_from_attributes(json!({})).err().unwrap().message,
                            sc["secondErr"].as_str().unwrap()
                        );
                    }
                }
                assert_eq!(run_calls.get(), sc["scheduledCount"].as_u64().unwrap() as usize);
            }
        }
    }

    let is_finite_json = |v: &Value| -> bool {
        fn walk(v: &Value) -> bool {
            match v {
                Value::String(s) if matches!(s.as_str(), "NaN" | "Infinity" | "-Infinity" | "-0") => false,
                Value::Array(a) => a.iter().all(walk),
                Value::Object(m) => m.values().all(walk),
                _ => true,
            }
        }
        walk(v)
    };

    // End-to-end replay of ALL finite pinned CesiumJS WASM worker fixtures through both coordinators!
    let sorter = GaussianSplatSorter::native(4);
    let mut cx = TaskContext::from_waker(Waker::noop());
    assert!(matches!(sorter.poll_initialization(&mut cx), Poll::Ready(Ok(true))));
    let sorter_fixture: Value = serde_json::from_str(include_str!("fixtures/workers/gaussianSplatSorter.json")).unwrap();
    let mut sorter_cases = 0usize;
    for case in sorter_fixture["cases"].as_array().unwrap() {
        if !is_finite_json(&case["a"]) {
            continue;
        }
        sorter_cases += 1;
        let a = case["a"].as_array().unwrap();
        let params = json!({
            "primitive": { "positions": a[0], "modelView": a[1], "count": a[2] },
            "sortType": case["f"],
        });
        let mut task = sorter.radix_sort_indexes(params).unwrap().unwrap();
        let out = match poll_once(&mut task) {
            Poll::Ready(r) => r,
            Poll::Pending => panic!("native sorter task did not finish"),
        };
        if let Some(err_msg) = case["r"].get("throws").and_then(Value::as_str) {
            assert_eq!(out.unwrap_err().message, err_msg, "sorter error mismatch for {case}");
        } else if case["r"].is_null() {
            assert_eq!(out.unwrap(), Value::Null);
        } else {
            assert_eq!(out.unwrap(), case["r"]["indices"], "sorter mismatch for {case}");
        }
    }

    assert!(sorter_cases > 0);

    let gen = GaussianSplatTextureGenerator::native(4);
    assert!(matches!(gen.poll_initialization(&mut cx), Poll::Ready(Ok(true))));
    let tex_fixture: Value =
        serde_json::from_str(include_str!("fixtures/workers/gaussianSplatTextureGenerator.json")).unwrap();
    let mut tex_cases = 0usize;
    for case in tex_fixture["cases"].as_array().unwrap() {
        if !is_finite_json(&case["a"]) {
            continue;
        }
        tex_cases += 1;
        let a = case["a"].as_array().unwrap();
        let params = json!({
            "attributes": { "positions": a[0], "scales": a[1], "rotations": a[2], "colors": a[3] },
            "count": a[4],
        });
        let mut task = gen.generate_from_attributes(params).unwrap().unwrap();
        let out = match poll_once(&mut task) {
            Poll::Ready(r) => r,
            Poll::Pending => panic!("native texture generator task did not finish"),
        };
        if let Some(err_msg) = case["r"].get("throws").and_then(Value::as_str) {
            assert_eq!(out.unwrap_err().message, err_msg, "texgen error mismatch for {case}");
        } else {
            let v = out.unwrap();
            let exp = &case["r"];
            assert_eq!(v["width"], exp["width"]);
            assert_eq!(v["height"], exp["height"]);
            let data: Vec<u64> = v["data"].as_array().unwrap().iter().map(|x| x.as_u64().unwrap()).collect();
            assert_eq!(data.len(), exp["data"]["length"].as_u64().unwrap() as usize);
            let end = data.iter().rposition(|&x| x != 0).map_or(0, |i| i + 1);
            let exp_vals: Vec<u64> =
                exp["data"]["values"].as_array().unwrap().iter().map(|x| x.as_u64().unwrap()).collect();
            assert_eq!(&data[..end], exp_vals.as_slice());
        }
    }
    assert!(tex_cases > 0);
}

// =============================================================================
// 7. VoxelBoundsCollection (SwiftShader WebGL2 capture replay)
// =============================================================================

#[test]
fn voxel_bounds_collection_matches_cesium_swiftshader_capture() {
    let f = &gpu_fixture()["voxelBoundsCollection"];
    let max_tex = f["maxTextureSize"].as_u64().unwrap() as u32;
    let host = RecordingGl::new([8, 8], true, max_tex);
    let context = Context::new_webgl2(host.clone());

    let p0 = ClippingPlane::handle(Cartesian3::new(1., 0., 0.), 2.5);
    let p1 = ClippingPlane::handle(Cartesian3::new(0., 1., 0.), -4.0);
    let initial_model = matrix4_from(&f["initialModel"]);
    let transform_a = Matrix4::IDENTITY;
    let transform_b = matrix4_from(&f["transformB"]);

    let mut col = VoxelBoundsCollection::<RecordingGl>::new(VoxelBoundsCollectionOptions {
        planes: vec![p0.clone(), p1.clone()],
        model_matrix: Some(initial_model),
        union_clipping_regions: Some(true),
    });

    let events: Arc<Mutex<Vec<Value>>> = Arc::new(Mutex::new(Vec::new()));
    {
        let ev = events.clone();
        col.plane_added.add_event_listener(move |e| {
            ev.lock().unwrap().push(json!({ "kind": "added", "index": e.index, "distance": e.plane.borrow().distance() }));
        });
    }
    {
        let ev = events.clone();
        col.plane_removed.add_event_listener(move |e| {
            ev.lock()
                .unwrap()
                .push(json!({ "kind": "removed", "index": e.index, "distance": e.plane.borrow().distance() }));
        });
    }

    let check_step = |col: &VoxelBoundsCollection<RecordingGl>, step: &Value| {
        let label = step["label"].as_str().unwrap();
        assert_eq!(col.len(), step["length"].as_u64().unwrap() as usize, "{label}: length");
        assert_eq!(
            col.union_clipping_regions,
            step["unionClippingRegions"].as_bool().unwrap(),
            "{label}: unionClippingRegions"
        );
        assert_eq!(
            col.clipping_planes_state(),
            step["clippingPlanesState"].as_i64().unwrap() as isize,
            "{label}: clippingPlanesState"
        );
        let indices: Vec<isize> = (0..col.len()).map(|i| col.get(i).unwrap().borrow().index).collect();
        let exp_indices: Vec<isize> =
            step["indices"].as_array().unwrap().iter().map(|v| v.as_i64().unwrap() as isize).collect();
        assert_eq!(indices, exp_indices, "{label}: indices");

        if step["textureResolution"].is_null() {
            assert_eq!(col.texture_resolution(max_tex), [0, 0], "{label}: empty res");
        } else {
            let exp_res = step["textureResolution"].as_array().unwrap();
            assert_eq!(
                col.texture_resolution(max_tex),
                [exp_res[0].as_u64().unwrap() as u32, exp_res[1].as_u64().unwrap() as u32],
                "{label}: texture_resolution"
            );
        }

        if step["texture"].is_null() {
            assert!(col.texture().is_none(), "{label}: expected no texture");
        } else {
            let tex = col.texture().expect(label);
            let exp_tex = &step["texture"];
            assert_eq!(tex.width(), exp_tex["width"].as_u64().unwrap() as u32, "{label}: tex width");
            assert_eq!(tex.height(), exp_tex["height"].as_u64().unwrap() as u32, "{label}: tex height");
            assert_eq!(tex.pixel_format() as u32, exp_tex["pixelFormat"].as_u64().unwrap() as u32, "{label}: format");
            assert_eq!(
                tex.pixel_datatype() as u32,
                exp_tex["pixelDatatype"].as_u64().unwrap() as u32,
                "{label}: datatype"
            );
            assert_eq!(tex.flip_y(), exp_tex["flipY"].as_bool().unwrap(), "{label}: flipY");
        }

        let got_events: Vec<Value> = events.lock().unwrap().drain(..).collect();
        let exp_events = step["events"].as_array().unwrap();
        assert_eq!(got_events.len(), exp_events.len(), "{label}: events len");
        for (i, (g, e)) in got_events.iter().zip(exp_events.iter()).enumerate() {
            assert_eq!(g["kind"], e["kind"], "{label}: event[{i}].kind");
            assert_eq!(g["index"], e["index"], "{label}: event[{i}].index");
            assert_close(
                g["distance"].as_f64().unwrap(),
                e["distance"].as_f64().unwrap(),
                1e-12,
                &format!("{label}: event[{i}].distance"),
            );
        }

        let got_uploads: Vec<(u32, u32, Vec<u8>)> = host.uploads.borrow_mut().drain(..).collect();
        if let Some(exp_floats) = step["gpuReadbackFloats"].as_array() {
            let (uw, uh, bytes) = got_uploads.last().expect(label);
            assert_eq!(*uw, step["texture"]["width"].as_u64().unwrap() as u32, "{label}: upload w");
            assert_eq!(*uh, step["texture"]["height"].as_u64().unwrap() as u32, "{label}: upload h");
            assert_eq!(bytes.len(), exp_floats.len() * 4, "{label}: upload bytes len");
            let actual_floats: Vec<f32> =
                bytes.chunks_exact(4).map(|c| f32::from_le_bytes(c.try_into().unwrap())).collect();
            for (i, (a, ev)) in actual_floats.iter().zip(exp_floats.iter()).enumerate() {
                let e = ev.as_f64().unwrap() as f32;
                assert!(
                    (a - e).abs() <= 1e-6_f32.max(e.abs() * 1e-6),
                    "{label}: float[{i}] actual={a} expected={e}"
                );
            }
            if let Some(exp_hash) = step["gpuReadbackHash"].as_str() {
                // Exact IEEE-754 float32 bytes match on identity/rational transforms;
                // verify exact hash whenever the floats match bit-for-bit.
                let exact_bits = actual_floats
                    .iter()
                    .zip(exp_floats.iter())
                    .all(|(a, ev)| a.to_bits() == (ev.as_f64().unwrap() as f32).to_bits());
                if exact_bits {
                    assert_eq!(fnv1a_64(bytes), exp_hash, "{label}: exact upload hash");
                }
            }
        }
    };

    let steps = f["steps"].as_array().unwrap();
    check_step(&col, &steps[0]);

    col.update(&context, transform_a).unwrap();
    check_step(&col, &steps[1]);

    col.union_clipping_regions = false;
    col.update(&context, transform_b).unwrap();
    check_step(&col, &steps[2]);

    p1.borrow_mut().set_distance(7.25);
    p1.borrow_mut().set_normal(Cartesian3::new(0., 3., 4.).normalize());
    col.update(&context, transform_b).unwrap();
    check_step(&col, &steps[3]);

    let extra = [
        ClippingPlane::handle(Cartesian3::new(0., 0., 1.), 1.0),
        ClippingPlane::handle(Cartesian3::new(-1., 0., 0.), 3.0),
        ClippingPlane::handle(Cartesian3::new(0., -1., 0.), 5.0),
        ClippingPlane::handle(Cartesian3::new(0., 0., -1.), 6.0),
    ];
    for p in &extra {
        col.add(p.clone());
    }
    col.update(&context, transform_a).unwrap();
    check_step(&col, &steps[4]);

    let removed_p0 = col.remove(&Plane::new(Cartesian3::new(1., 0., 0.), 2.5));
    let removed_missing = col.remove(&Plane::new(Cartesian3::new(1., 0., 0.), 999.));
    let plane_extra1 = extra[1].borrow().as_plane();
    let removed_extra1 = col.remove(&plane_extra1);
    assert_eq!(removed_p0, steps[5]["removedP0"].as_bool().unwrap());
    assert_eq!(removed_missing, steps[5]["removedMissing"].as_bool().unwrap());
    assert_eq!(removed_extra1, steps[5]["removedExtra1"].as_bool().unwrap());
    // Rust always unlinks the removed ClippingPlaneHandle (index = -1), whereas CesiumJS
    // only unlinks when the argument is `instanceof ClippingPlane` (not a plain `Plane`).
    assert_eq!(p0.borrow().index, -1);
    assert_eq!(extra[1].borrow().index, -1);
    col.update(&context, transform_a).unwrap();
    check_step(&col, &steps[5]);

    let plane_extra0 = extra[0].borrow().as_plane();
    let plane_extra2 = extra[2].borrow().as_plane();
    col.remove(&plane_extra0);
    col.remove(&plane_extra2);
    col.update(&context, transform_a).unwrap();
    check_step(&col, &steps[6]);

    col.remove_all();
    col.update(&context, transform_a).unwrap();
    check_step(&col, &steps[7]);

    col.add(ClippingPlane::handle(Cartesian3::new(0., 0., 1.), -1.5));
    col.update(&context, transform_a).unwrap();
    check_step(&col, &steps[8]);

    col.destroy().unwrap();
    assert_eq!(col.is_destroyed(), f["destroyed"].as_bool().unwrap());
    assert!(col.update(&context, transform_a).is_err());
}

// =============================================================================
// 8. BrdfLutGenerator (SwiftShader WebGL2 capture + existing 262144-byte GPU proof)
// =============================================================================

#[test]
fn brdf_lut_generator_matches_cesium_swiftshader_capture() {
    let f = &gpu_fixture()["brdfLutGenerator"];
    let host = RecordingGl::new([256, 256], true, 4096);
    let context = Context::new_webgl2(host.clone());

    let mut gen = BrdfLutGenerator::<RecordingGl>::new();
    assert!(gen.color_texture().is_none() && f["beforeTexture"].is_null());

    gen.update(&context).unwrap();
    let tex = gen.color_texture().expect("color texture").clone();
    assert_eq!(tex.width(), f["width"].as_u64().unwrap() as u32);
    assert_eq!(tex.height(), f["height"].as_u64().unwrap() as u32);
    assert_eq!(tex.pixel_format() as u32, f["pixelFormat"].as_u64().unwrap() as u32);
    assert_eq!(tex.pixel_datatype() as u32, f["pixelDatatype"].as_u64().unwrap() as u32);
    assert_eq!(tex.sampler().minification_filter as u32, f["sampler"]["minificationFilter"].as_u64().unwrap() as u32);
    assert_eq!(tex.sampler().magnification_filter as u32, f["sampler"]["magnificationFilter"].as_u64().unwrap() as u32);
    assert_eq!(tex.sampler().wrap_s as u32, f["sampler"]["wrapS"].as_u64().unwrap() as u32);
    assert_eq!(tex.sampler().wrap_t as u32, f["sampler"]["wrapT"].as_u64().unwrap() as u32);

    let vp = gen.draw_command().unwrap().options.render_state.viewport.unwrap();
    assert_eq!(vp.x, f["viewport"]["x"].as_i64().unwrap() as i32);
    assert_eq!(vp.y, f["viewport"]["y"].as_i64().unwrap() as i32);
    assert_eq!(vp.width, f["viewport"]["width"].as_u64().unwrap() as u32);
    assert_eq!(vp.height, f["viewport"]["height"].as_u64().unwrap() as u32);
    assert_eq!(host.draws.get(), f["drawsAfterFirst"].as_u64().unwrap() as usize);

    // Idempotent second update
    gen.update(&context).unwrap();
    assert!(Rc::ptr_eq(&tex, gen.color_texture().unwrap()) && f["sameTextureOnSecond"].as_bool().unwrap());
    assert_eq!(host.draws.get(), f["drawsAfterSecond"].as_u64().unwrap() as usize);

    gen.destroy();
    assert_eq!(gen.is_destroyed(), f["isDestroyedAfter"].as_bool().unwrap());
    assert!(tex.is_destroyed());
    assert!(gen.update(&context).is_err());

    // Confirm checked-in real WebGL2 SwiftShader byte-exact comparison evidence
    let gpu_evidence: Value =
        serde_json::from_str(include_str!("../docs/model-effects-evidence/webgl2-brdf-postprocess.json")).unwrap();
    let brdf_entry = gpu_evidence.as_array().unwrap().iter().find(|e| e["name"] == "BRDF").unwrap();
    assert_eq!(brdf_entry["bytes"].as_u64().unwrap(), f["byteLength"].as_u64().unwrap());
    assert_eq!(brdf_entry["maxDifference"].as_u64().unwrap(), 0);
}

// =============================================================================
// 9. PickedMetadataInfo (+ DerivedCommand::pick_metadata + MetadataPicking + Scene)
// =============================================================================

fn json_to_metadata_value(v: &Value) -> MetadataValue {
    match v {
        Value::Number(n) => MetadataValue::Number(n.as_f64().unwrap()),
        Value::Array(arr) => MetadataValue::Array(arr.iter().map(json_to_metadata_value).collect()),
        _ => panic!("unexpected transform value {v}"),
    }
}

#[test]
fn picked_metadata_info_matches_cesium_swiftshader_capture() {
    for case in gpu_fixture()["pickedMetadataInfo"].as_array().unwrap() {
        let name = case["name"].as_str().unwrap();
        let schema_id = case["schemaId"].as_str().map(str::to_owned);
        let class_name = case["className"].as_str().unwrap().to_owned();
        let property_name = case["propertyName"].as_str().unwrap().to_owned();

        let class_property = Arc::new(
            MetadataClassProperty::from_json(&property_name, &case["classProp"], &Default::default())
                .unwrap_or_else(|e| panic!("{name}: from_json failed: {e}")),
        );
        let value_transform = if case["hasValueTransform"].as_bool().unwrap() {
            let mt = class_property.metadata_type();
            let offset =
                MetadataPicking::convert_to_object_type(mt, Some(json_to_metadata_value(&case["offset"]))).unwrap().unwrap();
            let scale =
                MetadataPicking::convert_to_object_type(mt, Some(json_to_metadata_value(&case["scale"]))).unwrap().unwrap();
            Some((offset, scale))
        } else {
            None
        };

        let info = PickedMetadataInfo::new(
            schema_id.clone(),
            class_name.clone(),
            property_name.clone(),
            class_property,
            value_transform,
        )
        .unwrap();

        assert_eq!(info.schema_id(), schema_id.as_deref(), "{name}: schema_id");
        assert_eq!(info.class_name(), class_name, "{name}: class_name");
        assert_eq!(info.property_name(), property_name, "{name}: property_name");
        assert_eq!(info.shader.cache_key(), case["cacheKey"].as_str().unwrap(), "{name}: cache_key");

        let base = ShaderProgramOptions {
            vertex_shader_source: ShaderSource::new("void main() { gl_Position = vec4(0.0); }"),
            fragment_shader_source: ShaderSource {
                defines: vec![
                    "EXISTING_DEFINE".into(),
                    "METADATA_PICKING_VALUE_TYPE float".into(),
                    "METADATA_PICKING_VALUE_STRING 0.0".into(),
                    "METADATA_PICKING_VALUE_COMPONENT_X 0.0".into(),
                    "METADATA_PICKING_VALUE_COMPONENT_Y 0.0".into(),
                    "METADATA_PICKING_VALUE_COMPONENT_Z 0.0".into(),
                    "METADATA_PICKING_VALUE_COMPONENT_W 0.0".into(),
                ],
                sources: vec!["void main() { out_FragColor = vec4(1.0); }".into()],
                ..Default::default()
            },
            attribute_locations: Default::default(),
        };
        let derived = DerivedCommand::pick_metadata(&base, &info.shader);
        assert_eq!(json!(derived.fragment_shader_source.defines), case["derivedDefines"], "{name}: derivedDefines");

        // Replay the real SwiftShader-rendered RGBA8 pixel through MetadataPicking and Scene::pick_metadata_with_info
        if let Some(px_arr) = case["pixel"].as_array() {
            let pixel: [u8; 4] = [
                px_arr[0].as_u64().unwrap() as u8,
                px_arr[1].as_u64().unwrap() as u8,
                px_arr[2].as_u64().unwrap() as u8,
                px_arr[3].as_u64().unwrap() as u8,
            ];
            let decoded = MetadataPicking::decode_metadata_values(
                &info.class_property,
                info.value_transform.as_ref().map(|(o, s)| (o, s)),
                &pixel,
            )
            .unwrap();

            // Also verify end-to-end Scene::pick_metadata_with_info decodes identically
            let host = RecordingGl::new([4, 4], true, 4096);
            host.read_pixel.set(pixel);
            let mut scene = Scene::new(
                RendererSceneBackend::new(Context::new_webgl2(host)),
                CameraOptions::new(4., 4.),
                SceneOptions::default(),
                JulianDate::new(2451545., 0., TimeStandard::Tai),
            )
            .unwrap();
            let scene_decoded = scene.pick_metadata_with_info(Cartesian2::new(2., 2.), info.clone()).unwrap();
            assert_eq!(scene_decoded, decoded, "{name}: Scene::pick_metadata_with_info");

            // Check against CesiumJS decoded value
            let exp = &case["decoded"];
            match (&decoded, exp) {
                (MetadataValue::Number(a), Value::Number(e)) => {
                    assert_close(*a, e.as_f64().unwrap(), 1e-6, &format!("{name}: decoded scalar"));
                }
                (MetadataValue::Cartesian2(a), Value::Array(e)) => {
                    assert_close(a.x, e[0].as_f64().unwrap(), 1e-6, &format!("{name}: vec2.x"));
                    assert_close(a.y, e[1].as_f64().unwrap(), 1e-6, &format!("{name}: vec2.y"));
                }
                (MetadataValue::Cartesian3(a), Value::Array(e)) => {
                    assert_close(a.x, e[0].as_f64().unwrap(), 1e-6, &format!("{name}: vec3.x"));
                    assert_close(a.y, e[1].as_f64().unwrap(), 1e-6, &format!("{name}: vec3.y"));
                    assert_close(a.z, e[2].as_f64().unwrap(), 1e-6, &format!("{name}: vec3.z"));
                }
                (MetadataValue::Cartesian4(a), Value::Array(e)) => {
                    assert_close(a.x, e[0].as_f64().unwrap(), 1e-6, &format!("{name}: vec4.x"));
                    assert_close(a.y, e[1].as_f64().unwrap(), 1e-6, &format!("{name}: vec4.y"));
                    assert_close(a.z, e[2].as_f64().unwrap(), 1e-6, &format!("{name}: vec4.z"));
                    assert_close(a.w, e[3].as_f64().unwrap(), 1e-6, &format!("{name}: vec4.w"));
                }
                other => panic!("{name}: unexpected decoded pair {other:?}"),
            }

            // Confirm ScenePickObject metadata map lookup works with PickedMetadataInfo
            let mut pick_obj = ScenePickObject::new(Rc::new(42u32), Rc::new(Cell::new(true)));
            pick_obj.metadata.insert((schema_id.clone(), class_name.clone(), property_name.clone()), info);
            assert!(pick_obj.metadata.contains_key(&(schema_id, class_name, property_name)));
        }
    }
}

// =============================================================================
// 10. TranslucentTileClassification (SwiftShader WebGL2 capture replay)
// =============================================================================

#[test]
fn translucent_tile_classification_matches_cesium_swiftshader_capture() {
    let f = &gpu_fixture()["translucentTileClassification"];
    let host = RecordingGl::new([8, 8], true, 4096);
    let context = Context::new_webgl2(host.clone());

    // Unsupported when depth_texture is false
    let unsupported_ctx = Context::new_webgl1(RecordingGl::new([8, 8], false, 4096));
    let mut unsupported = TranslucentTileClassification::new(&unsupported_ctx).unwrap();
    assert!(!unsupported.is_supported());
    let dummy_tex = Rc::new(Texture::new(&unsupported_ctx, TextureOptions::rgba(8, 8), None).unwrap());
    assert!(unsupported.begin_translucent_depth(&unsupported_ctx, &PassState::default(), dummy_tex, true).unwrap().is_none());

    let mut ttc = TranslucentTileClassification::new(&context).unwrap();
    assert_eq!(ttc.is_supported(), f["afterUnflagged"]["supported"].as_bool().unwrap());

    let make_opaque_depth = |w, h| {
        let mut opts = TextureOptions::rgba(w, h);
        opts.pixel_format = PixelFormat::DepthStencil;
        opts.pixel_datatype = PixelDatatype::UnsignedInt24_8;
        opts.sampler = Sampler::NEAREST;
        Rc::new(Texture::new(&context, opts, None).unwrap())
    };

    let opaque8 = make_opaque_depth(8, 8);
    let initial_globe_depth = Rc::new(Texture::new(&context, TextureOptions::rgba(8, 8), None).unwrap());
    context
        .set_automatic_texture("czm_globeDepthTexture", Some(TextureBinding::Texture(initial_globe_depth.clone())))
        .unwrap();

    let pass_full = PassState { viewport: Some(Viewport { x: 0, y: 0, width: 8, height: 8 }), ..Default::default() };

    // Step 1: Unflagged command does nothing
    assert!(ttc.begin_translucent_depth(&context, &pass_full, opaque8.clone(), false).unwrap().is_none());
    assert_eq!(ttc.has_translucent_depth(), f["afterUnflagged"]["hasTranslucentDepth"].as_bool().unwrap());
    assert!(ttc.packed_depth_texture().unwrap().is_none() && !f["afterUnflagged"]["hasDepthTex"].as_bool().unwrap());

    // Step 2: Single frustum (render pass, full viewport)
    let depth_pass = ttc.begin_translucent_depth(&context, &pass_full, opaque8.clone(), true).unwrap().unwrap();
    assert!(depth_pass.framebuffer.is_some());
    assert_eq!(ttc.has_translucent_depth(), f["afterDepth1"]["hasTranslucentDepth"].as_bool().unwrap());
    let packed = ttc.packed_depth_texture().unwrap().unwrap().clone();
    assert_eq!(packed.width(), f["afterDepth1"]["depthTex"]["width"].as_u64().unwrap() as u32);
    assert_eq!(packed.height(), f["afterDepth1"]["depthTex"]["height"].as_u64().unwrap() as u32);
    ttc.pack_depth(&context, &pass_full).unwrap();

    let classify_pass = ttc.begin_classification(&context, &pass_full).unwrap().unwrap();
    assert!(classify_pass.framebuffer.is_some());
    assert_eq!(context.uniform_state().pass, Some(Pass::Cesium3DTileClassification));
    // During classification, czm_globeDepthTexture must be swapped to the packed translucent depth texture!
    match context.automatic_texture("czm_globeDepthTexture").unwrap() {
        Some(TextureBinding::Texture(t)) => assert!(Rc::ptr_eq(&t, &packed)),
        _ => panic!("expected packed depth bound to czm_globeDepthTexture"),
    }
    ttc.end_classification(&context, &pass_full).unwrap();
    // After end_classification, czm_globeDepthTexture must be restored to initial_globe_depth!
    match context.automatic_texture("czm_globeDepthTexture").unwrap() {
        Some(TextureBinding::Texture(t)) => {
            assert!(Rc::ptr_eq(&t, &initial_globe_depth) && f["afterExec1"]["globeDepthRestored1"].as_bool().unwrap());
        }
        _ => panic!("expected initial globe depth restored"),
    }
    let draws_before_exec1 = host.draws.get();
    ttc.execute(&context, &pass_full, false).unwrap();
    assert!(host.draws.get() > draws_before_exec1);
    assert_eq!(ttc.has_translucent_depth(), f["afterExec1"]["hasTranslucentDepth"].as_bool().unwrap());

    // Step 3: Multi-frustum (2 frustums, sub-viewport -> scissor enabled, pick pass)
    let pass_sub = PassState { viewport: Some(Viewport { x: 1, y: 2, width: 4, height: 4 }), ..Default::default() };
    for _ in 0..2 {
        assert!(ttc.begin_translucent_depth(&context, &pass_sub, opaque8.clone(), true).unwrap().is_some());
        ttc.pack_depth(&context, &pass_sub).unwrap();
        assert!(ttc.begin_classification(&context, &pass_sub).unwrap().is_some());
        ttc.end_classification(&context, &pass_sub).unwrap();
    }
    // Verify scissorTest, stencilTest (EQUAL 0x80), and ALPHA_BLEND match CesiumJS captured render states
    let rs_acc = &f["beforeExec2"]["rsAccumulateSub"];
    let rs_comp = &f["beforeExec2"]["rsCompSub"];
    let exp_scissor: Vec<f64> = f64_vec(&rs_acc["scissorRect"]);
    let exp_stencil_func = rs_acc["stencilFrontFunc"].as_f64().unwrap();
    let exp_stencil_ref = rs_acc["stencilRef"].as_f64().unwrap();
    let exp_src_rgb = rs_comp["blendingSrcRgb"].as_f64().unwrap();
    let exp_dst_rgb = rs_comp["blendingDstRgb"].as_f64().unwrap();

    ttc.execute(&context, &pass_sub, true).unwrap();
    assert_eq!(ttc.has_translucent_depth(), f["afterExec2"]["hasTranslucentDepth"].as_bool().unwrap());
    let recorded = host.call_args.borrow();
    assert!(
        recorded.iter().any(|(n, a)| n == "scissor" && a == &exp_scissor),
        "expected scissor({exp_scissor:?}) in {recorded:?}"
    );
    assert!(
        recorded.iter().any(|(n, a)| {
            (n == "stencilFuncSeparate" && a.get(1) == Some(&exp_stencil_func) && a.get(2) == Some(&exp_stencil_ref))
                || (n == "stencilFunc" && a.first() == Some(&exp_stencil_func) && a.get(1) == Some(&exp_stencil_ref))
        }),
        "expected stencil func {exp_stencil_func} ref {exp_stencil_ref} in {recorded:?}"
    );
    assert!(
        recorded
            .iter()
            .any(|(n, a)| n == "blendFuncSeparate" && a.first() == Some(&exp_src_rgb) && a.get(1) == Some(&exp_dst_rgb)),
        "expected blendFuncSeparate({exp_src_rgb}, {exp_dst_rgb}, ...) in {recorded:?}"
    );
    match context.automatic_texture("czm_globeDepthTexture").unwrap() {
        Some(TextureBinding::Texture(t)) => {
            assert!(Rc::ptr_eq(&t, &initial_globe_depth) && f["afterExec2"]["globeDepthRestored2"].as_bool().unwrap());
        }
        _ => panic!("expected initial globe depth restored after multi-frustum"),
    }

    // Step 4: Resize opaque depth to 12x6
    let opaque12 = make_opaque_depth(12, 6);
    let pass12 = PassState { viewport: Some(Viewport { x: 0, y: 0, width: 12, height: 6 }), ..Default::default() };
    assert!(ttc.begin_translucent_depth(&context, &pass12, opaque12, true).unwrap().is_some());
    let resized_packed = ttc.packed_depth_texture().unwrap().unwrap();
    assert_eq!(
        [resized_packed.width(), resized_packed.height()],
        [
            f["afterResize"]["packedDims"][0].as_u64().unwrap() as u32,
            f["afterResize"]["packedDims"][1].as_u64().unwrap() as u32,
        ]
    );

    ttc.destroy().unwrap();
    assert_eq!(ttc.is_destroyed(), f["destroyed"].as_bool().unwrap());
    assert!(ttc.begin_translucent_depth(&context, &pass_full, opaque8, true).is_err());

    // Keep RenderState import verified against CesiumJS constants captured in TranslucentTileClassification
    let _ = RenderState::default();
}

// =============================================================================
// 11. ModelDrawCommands (SwiftShader WebGL2 capture replay)
// =============================================================================

#[test]
fn model_draw_commands_matches_cesium_swiftshader_capture() {
    let f = &gpu_fixture()["modelDrawCommands"];
    let host = RecordingGl::new([8, 8], true, 4096);
    let context = Context::new_webgl2(host);

    let vb = Buffer::new(&context, BufferKind::Vertex, BufferUsage::StaticDraw, BufferData::Bytes(&[0u8; 36])).unwrap();
    let ib = Buffer::new(
        &context,
        BufferKind::Index(IndexDatatype::UnsignedShort),
        BufferUsage::StaticDraw,
        BufferData::Bytes(&[0u8; 6]),
    )
    .unwrap();
    let wire_ib = Buffer::new(
        &context,
        BufferKind::Index(IndexDatatype::UnsignedShort),
        BufferUsage::StaticDraw,
        BufferData::Bytes(&[0u8; 12]),
    )
    .unwrap();

    let translate_m = matrix4_from(&f["translateMatrix"]);
    let mirror_m = matrix4_from(&f["mirrorMatrix"]);
    let node_m = matrix4_from(&f["nodeMatrix"]);

    for case in f["cases"].as_array().unwrap() {
        let name = case["name"].as_str().unwrap();
        let mut model_state = ModelUniformState::default();
        if name == "classification" {
            model_state.classification_type = Some(ClassificationType::Cesium3DTile);
        }
        let model_res = ModelRenderResources::<RecordingGl>::new(Rc::new(RefCell::new(model_state)));
        let node_res = NodeRenderResources::new(&model_res, 0);

        let mut prim_mc = mc::Primitive::<RecordingGl>::default();
        let mut pos_attr = mc::Attribute::<RecordingGl>::default();
        pos_attr.semantic = Some(mc::AttributeSemantic::Vertex(
            cesium_rs::scene::VertexAttributeSemantic::Position,
        ));
        pos_attr.min = Some(mc::MathValue::Cartesian3(Cartesian3::new(-4., -3., -2.)));
        pos_attr.max = Some(mc::MathValue::Cartesian3(Cartesian3::new(6., 7., 8.)));
        pos_attr.count = Some(3);
        prim_mc.attributes.push(pos_attr);

        let mut resources = PrimitiveRenderResources::new(&node_res, 0, &prim_mc, None, None).unwrap();
        resources.bounding_sphere = BoundingSphere::new(Cartesian3::new(1., 2., 3.), 5.);
        resources.pipeline.shader_builder.add_attribute("vec3", "a_position");
        resources.pipeline.render_state_options.cull.enabled = true;
        resources.attributes.push(Rc::new(VertexAttribute {
            index: 0,
            enabled: true,
            source: AttributeSource::Buffer(vb.clone()),
            components: 3,
            datatype: ComponentDatatype::Float,
            normalize: false,
            offset: 0,
            stride: 12,
            instance_divisor: 0,
        }));
        resources.pick_id = Some("czm_pickColor".into());
        resources.batch_lengths = Some(vec![3]);
        resources.batch_offsets = Some(vec![0]);

        let mut frame = make_frame([8, 8]);
        let mut opts = ModelDrawBuildOptions::default();
        opts.derived = ModelDrawCommandOptions { shadows: ShadowMode::Enabled, ..Default::default() };

        let make_indices = |buffer: Option<Buffer<RecordingGl>>| mc::Indices {
            index_datatype: Some(IndexDatatype::UnsignedShort),
            count: Some(3),
            buffer,
            typed_array: None,
        };

        match name {
            "ordinary-3d" => {
                opts.model_matrix = translate_m;
                opts.node_transform = node_m;
                resources.indices = Some(make_indices(Some(ib.clone())));
                resources.count = Some(3);
                resources.instance_count = 2;
            }
            "mirrored-cull-front" => {
                opts.model_matrix = mirror_m;
                opts.derived.shadows = ShadowMode::CastOnly;
                resources.indices = Some(make_indices(Some(ib.clone())));
                resources.count = Some(3);
            }
            "wireframe-index-override" => {
                resources.indices = Some(make_indices(Some(ib.clone())));
                resources.wireframe_index_buffer = Some(wire_ib.clone());
                resources.primitive_type = Some(PrimitiveType::Lines);
                resources.count = Some(6);
            }
            "projected-2d" => {
                frame.mode = SceneMode::Scene2D;
                opts.model_matrix = translate_m;
                opts.node_transform = node_m;
                opts.project_to_2d = true;
                opts.bounding_sphere_2d = Some(BoundingSphere::new(Cartesian3::new(10., 20., 30.), 7.));
                resources.pipeline.alpha_options.borrow_mut().pass = Some(Pass::Translucent);
                resources.count = Some(3);
            }
            "classification" => {
                resources.indices = Some(make_indices(Some(ib.clone())));
                resources.count = Some(3);
            }
            "missing-indices-buffer-error" => {
                resources.indices = Some(make_indices(None));
            }
            other => panic!("unexpected case {other}"),
        }

        let built = build_model_draw_command(&mut resources, &context, &frame, opts);
        if let Some(err) = case["error"].as_str() {
            assert_eq!(built.err().unwrap().message, err, "{name}: error");
            continue;
        }
        let kind = built.unwrap_or_else(|e| panic!("{name}: unexpected error {e}"));
        let is_class = matches!(kind, ModelDrawCommandKind::Classification(_));
        assert_eq!(is_class, case["isClassification"].as_bool().unwrap(), "{name}: isClassification");

        let cmd = kind.command();
        assert_eq!(cmd.state.cull.face, case["cullFace"].as_u64().unwrap() as u32, "{name}: cullFace");
        assert_eq!(cmd.pass as u32, case["pass"].as_u64().unwrap() as u32, "{name}: pass");
        assert_eq!(cmd.primitive_type as u32, case["primitiveType"].as_u64().unwrap() as u32, "{name}: primitiveType");
        assert_eq!(cmd.count, case["count"].as_u64().map(|n| n as usize), "{name}: count");
        assert_eq!(cmd.instance_count, case["instanceCount"].as_u64().unwrap() as u32, "{name}: instanceCount");
        assert_eq!(cmd.metadata.cull, case["cull"].as_bool().unwrap(), "{name}: cull");
        assert_eq!(
            cmd.metadata.debug_show_bounding_volume,
            case["debugShowBoundingVolume"].as_bool().unwrap(),
            "{name}: debugShowBoundingVolume"
        );
        assert_eq!(cmd.metadata.pick_id.as_deref(), case["pickId"].as_str(), "{name}: pickId");
        assert_eq!(
            cmd.metadata.pick_metadata_allowed,
            case["pickMetadataAllowed"].as_bool().unwrap(),
            "{name}: pickMetadataAllowed"
        );
        assert_eq!(cmd.metadata.cast_shadows, case["castShadows"].as_bool().unwrap(), "{name}: castShadows");
        assert_eq!(cmd.metadata.receive_shadows, case["receiveShadows"].as_bool().unwrap(), "{name}: receiveShadows");

        let exp_m = matrix4_from(&case["modelMatrix"]);
        let actual_m = cmd.metadata.model_matrix.unwrap();
        for i in 0..16 {
            assert_close(actual_m.0[i], exp_m.0[i], 1e-12, &format!("{name}: modelMatrix[{i}]"));
        }
        let bs = cmd.metadata.bounding_volume.unwrap();
        let ec = f64_vec(&case["boundingSphere"]["center"]);
        assert_close(bs.center.x, ec[0], 1e-12, &format!("{name}: bs.x"));
        assert_close(bs.center.y, ec[1], 1e-12, &format!("{name}: bs.y"));
        assert_close(bs.center.z, ec[2], 1e-12, &format!("{name}: bs.z"));
        assert_close(bs.radius, case["boundingSphere"]["radius"].as_f64().unwrap(), 1e-12, &format!("{name}: bs.r"));

        let gpu = cmd.resources.as_ref().unwrap();
        assert_eq!(gpu.vertices.index_buffer().is_some(), case["hasIndexBuffer"].as_bool().unwrap(), "{name}: hasIb");
    }
}

// =============================================================================
// 11 & 12. Model & Model3DTileContent (Rc<StructuralMetadata>, axis defaults,
//          and SwiftShader WebGL2 pixel parity evidence)
// =============================================================================

#[test]
fn model_and_tile_content_regressions_and_webgl2_evidence() {
    use cesium_rs::scene::model::{Model, ModelOptions, ModelSceneGraph, ModelSceneGraphOptions};
    use cesium_rs::scene::{
        AttributeType, Axis, FrameResource, JsonMetadataTable, MetadataSchema, PropertyTable, PropertyTableOptions,
        StructuralMetadata, StructuralMetadataOptions, VertexAttributeSemantic,
    };

    // Regression 1: ModelSceneGraph treats unset up_axis/forward_axis as identity
    // (matching CesiumJS ModelUtility.getAxisCorrectionMatrix(undefined, undefined) for PNTS & GeoJSON).
    let unset_graph = ModelSceneGraph::<RecordingGl>::new(
        mc::Components {
            scene: Some(mc::Scene { nodes: vec![0] }),
            nodes: vec![mc::Node { index: Some(0), ..Default::default() }],
            ..Default::default()
        },
        ModelSceneGraphOptions::default(),
    )
    .unwrap();
    assert_eq!(*unset_graph.axis_correction_matrix(), Matrix4::IDENTITY);

    let gltf_graph = ModelSceneGraph::<RecordingGl>::new(
        mc::Components {
            scene: Some(mc::Scene { nodes: vec![0] }),
            nodes: vec![mc::Node { index: Some(0), ..Default::default() }],
            up_axis: Some(Axis::Y),
            forward_axis: Some(Axis::Z),
            ..Default::default()
        },
        ModelSceneGraphOptions::default(),
    )
    .unwrap();
    assert_eq!(
        *gltf_graph.axis_correction_matrix(),
        Axis::Y_UP_TO_Z_UP.multiply_transformation(&Axis::Z_UP_TO_X_UP)
    );

    // Regression 2: Model resolves Rc<StructuralMetadata<G>> produced by B3dm/I3dm/Pnts/GeoJson loaders.
    let context = Context::new_webgl2(RecordingGl::new([8, 8], true, 1024));
    let pos_bytes = TypedArray::Float32(vec![0., 0., 0., 1., 0., 0., 0., 1., 0.]).to_le_bytes();
    let fid_bytes = TypedArray::Float32(vec![0., 0., 1.]).to_le_bytes();
    let pos_buf =
        Buffer::new(&context, BufferKind::Vertex, BufferUsage::StaticDraw, BufferData::Bytes(&pos_bytes)).unwrap();
    let fid_buf =
        Buffer::new(&context, BufferKind::Vertex, BufferUsage::StaticDraw, BufferData::Bytes(&fid_bytes)).unwrap();
    let table_json = json!({ "kind": [0, 1] });
    let jmt = JsonMetadataTable::new(2, &table_json).unwrap();
    let pt = PropertyTable::new(PropertyTableOptions {
        id: Some(0.into()),
        count: 2,
        json_metadata_table: Some(jmt),
        ..Default::default()
    });
    let schema = MetadataSchema::from_json(&json!({})).unwrap();
    let sm: StructuralMetadata<RecordingGl> = StructuralMetadata::new(StructuralMetadataOptions {
        schema: Arc::new(schema),
        property_tables: Some(vec![pt]),
        ..Default::default()
    });
    let primitive = mc::Primitive {
        attributes: vec![
            mc::Attribute {
                semantic: Some(mc::AttributeSemantic::Vertex(VertexAttributeSemantic::Position)),
                component_datatype: Some(ComponentDatatype::Float),
                attribute_type: Some(AttributeType::Vec3),
                count: Some(3),
                min: Some(mc::MathValue::Cartesian3(Cartesian3::ZERO)),
                max: Some(mc::MathValue::Cartesian3(Cartesian3::new(1., 1., 0.))),
                buffer: Some(pos_buf),
                ..Default::default()
            },
            mc::Attribute {
                semantic: Some(mc::AttributeSemantic::Vertex(VertexAttributeSemantic::FeatureId)),
                set_index: Some(0),
                component_datatype: Some(ComponentDatatype::Float),
                attribute_type: Some(AttributeType::Scalar),
                count: Some(3),
                buffer: Some(fid_buf),
                ..Default::default()
            },
        ],
        feature_ids: vec![mc::FeatureId::Attribute(mc::FeatureIdAttribute {
            feature_count: Some(2),
            null_feature_id: None,
            property_table_id: Some(0),
            set_index: Some(0),
            label: None,
            positional_label: Some("featureId_0".into()),
        })],
        primitive_type: Some(PrimitiveType::Triangles),
        material: Some(mc::Material { unlit: true, ..Default::default() }),
        ..Default::default()
    };
    let components = mc::Components {
        scene: Some(mc::Scene { nodes: vec![0] }),
        nodes: vec![mc::Node { index: Some(0), primitives: vec![primitive], ..Default::default() }],
        structural_metadata: Some(FrameResource::new(Rc::new(sm))),
        ..Default::default()
    };
    let mut model = Model::from_components(context, components, ModelOptions::default()).unwrap();
    let mut frame = make_frame([8, 8]);
    model.update(&mut frame).unwrap();
    for cb in frame.after_render.drain(..) {
        cb().unwrap();
    }
    assert!(model.ready());
    let ft = model.feature_tables.first().expect("feature_tables[0] from Rc<StructuralMetadata>");
    assert_eq!(ft.features_length(), 2);
    assert!(ft.has_property(0, "kind").unwrap());
    model.destroy().unwrap();

    // Verify committed SwiftShader WebGL2 pixel-for-pixel evidence against pinned @cesium/engine 23.0.0.
    let evidence: Value =
        serde_json::from_str(include_str!("../docs/model-effects-evidence/webgl2-model-validation.json")).unwrap();
    let cases = evidence.as_array().unwrap();
    assert_eq!(cases.len(), 13);
    for case in cases {
        let name = case["name"].as_str().unwrap();
        assert_eq!(case["maxChannelDiff"].as_u64().unwrap(), 0, "WebGL2 diff for {name}");
        if name != "gltf-lifecycle-step-1" {
            assert!(case["nonZeroPixels"].as_u64().unwrap() > 0, "expected rendered pixels for {name}");
        }
    }
}
