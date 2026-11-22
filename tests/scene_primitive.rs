//! Replay Primitive's SwiftShader WebGL2 capture from @cesium/engine 23.0.0.
mod common;

use cesium_rs::{
    core::{webgl_constants as gl, *},
    renderer::{Context, Gl, GlValue, ShaderProgram, UniformFrame, UniformValue},
    scene::*,
};
use common::{c3, nums};
use serde_json::{json, Value};
use std::{
    cell::{Cell, RefCell},
    collections::{BTreeMap, HashMap},
    rc::Rc,
};

#[derive(Default)]
struct RecordingGl {
    next: Cell<u32>,
    calls: RefCell<Vec<Value>>,
    draws: RefCell<Vec<Value>>,
    deleted: RefCell<Vec<String>>,
    active_attributes: Vec<Vec<String>>,
    programs: RefCell<HashMap<u32, usize>>,
}

fn fnv(bytes: &[u8]) -> String {
    let mut hash = 0xcbf29ce484222325u64;
    for byte in bytes {
        hash = (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3);
    }
    format!("{hash:x}")
}

impl Gl for RecordingGl {
    type Object = u32;
    fn drawing_buffer_size(&self) -> Result<[u32; 2], RuntimeError> {
        Ok([8, 8])
    }

    fn invoke(&self, method: &str, args: &[GlValue<u32>]) -> Result<GlValue<u32>, RuntimeError> {
        let number = |index| match args.get(index) {
            Some(GlValue::Number(n)) => *n as u32,
            _ => 0,
        };
        let arg = |index| match args.get(index) {
            Some(GlValue::Bool(value)) => json!(value),
            _ => json!(number(index)),
        };
        let object = |index| match args.get(index) {
            Some(GlValue::Object(id)) => *id,
            _ => 0,
        };
        let data = |index| match args.get(index) {
            Some(GlValue::Bytes(bytes) | GlValue::Pixels { bytes, .. }) => Some(bytes.as_slice()),
            _ => None,
        };
        let call = match method {
            "bufferData" => data(1).map(|bytes| json!([method, arg(0), arg(2), bytes.len(), fnv(bytes)])),
            "texImage2D" | "texSubImage2D" if args.len() == 9 => {
                let first = if method == "texImage2D" { 3 } else { 4 };
                let bytes = data(8);
                Some(json!([method, arg(first), arg(first + 1), arg(7), bytes.map_or(0, <[u8]>::len), bytes.map(fnv)]))
            }
            "vertexAttribPointer" => Some(json!(["pointer", arg(0), arg(1), arg(2), arg(3), arg(4), arg(5)])),
            _ => None,
        };
        if let Some(call) = call {
            self.calls.borrow_mut().push(call);
        }
        if matches!(method, "drawArrays" | "drawElements") {
            self.draws.borrow_mut().push(json!([method, (0..args.len()).map(arg).collect::<Vec<_>>()]));
        }
        if method.starts_with("delete") {
            self.deleted.borrow_mut().push(method.into());
        }
        if method.starts_with("create") {
            let next = self.next.get() + 1;
            self.next.set(next);
            return Ok(GlValue::Object(next));
        }

        let active = || {
            self.programs
                .borrow()
                .get(&object(0))
                .and_then(|index| self.active_attributes.get(*index))
                .map(Vec::as_slice)
                .unwrap_or_default()
        };
        Ok(match method {
            "getParameter" => match number(0) {
                gl::MAX_TEXTURE_SIZE => GlValue::Number(8192.),
                gl::ALIASED_POINT_SIZE_RANGE | gl::ALIASED_LINE_WIDTH_RANGE | gl::MAX_VIEWPORT_DIMS => {
                    GlValue::Numbers(vec![1., 4096.])
                }
                _ => GlValue::Number(4096.),
            },
            "getExtension" => GlValue::Object(0),
            "linkProgram" => {
                let mut programs = self.programs.borrow_mut();
                let index = programs.len();
                programs.insert(object(0), index);
                GlValue::Null
            }
            "getActiveAttrib" => {
                let name = &active()[number(1) as usize];
                GlValue::Record(json!({"name": name, "size": 1, "type": gl::FLOAT}))
            }
            "getProgramParameter" if number(1) == gl::ACTIVE_ATTRIBUTES => GlValue::Number(active().len() as f64),
            "getProgramParameter" if number(1) == gl::ACTIVE_UNIFORMS => GlValue::Number(0.),
            "getProgramParameter" | "getShaderParameter" => GlValue::Bool(true),
            "checkFramebufferStatus" => GlValue::Number(f64::from(gl::FRAMEBUFFER_COMPLETE)),
            _ => GlValue::Null,
        })
    }
}

fn frame(context: &Context<RecordingGl>) -> FrameState {
    let mut camera = Camera::new(CameraOptions::new(8., 8.));
    camera.position = Cartesian3::new(7000000., 0., 0.);
    camera.direction = Cartesian3::new(-1., 0., 0.);
    camera.up = Cartesian3::UNIT_Z;
    camera.right = Cartesian3::UNIT_Y;
    let mut frustum = PerspectiveFrustum::new(std::f64::consts::FRAC_PI_2, 1.);
    frustum.near = 1.;
    frustum.far = 32.;
    camera.frustum = CameraFrustum::Perspective(frustum);
    let uniform = camera.uniform_camera();
    let volume = camera.frustum.compute_culling_volume(uniform.position, uniform.direction, uniform.up);
    let mut frame =
        FrameState::new(UniformFrame::new(uniform, JulianDate::new(2451545., 0., TimeStandard::Tai)), volume, [8, 8]);
    frame.context = Some(FrameResource::new(context.clone()));
    frame.context_capabilities = context.capabilities().unwrap();
    frame.passes.render = true;
    frame
}

#[test]
fn data_source_display_queues_geometry_and_polyline_draws() {
    use cesium_rs::data_sources::*;

    let context = Context::new_webgl2(RecordingGl::default());
    let mut frame = frame(&context);
    frame.scene_3d_only = true;
    let backend = RendererSceneBackend::new(context);
    let mut render_scene =
        Scene::new(backend, CameraOptions::new(8.0, 8.0), SceneOptions::default(), frame.time).unwrap();
    render_scene.backend.configure_frame(&mut frame);
    let host = BasicGeometryScene::attach_to(&mut render_scene);
    let scene = host.clone();
    let factory: VisualizerFactory = Rc::new(move |source| {
        vec![
            Box::new(GeometryVisualizer::new(scene.clone(), source.entities(), None, None)),
            Box::new(PolylineVisualizer::new(scene.clone(), source.entities(), None, None)),
        ]
    });
    let display = DataSourceDisplay::new(DataSourceCollection::new(), factory);
    let source = display.default_data_source();
    let position = Cartesian3::new(6_999_984.0, 0.0, 0.0);
    let entity = Entity::with_id("box");
    entity.set_position(Some(Rc::new(ConstantPositionProperty::new(Some(position), ReferenceFrame::Fixed)))).unwrap();
    let graphics = Rc::new(BoxGraphics::new());
    graphics.set_dimensions(Some(PropertyInput::value(Cartesian3::new(4.0, 4.0, 4.0)))).unwrap();
    entity.set_property("box", Some(PropertyInput::Component(graphics))).unwrap();
    source.entities().add(entity.clone()).unwrap();

    let line = Entity::with_id("dynamic-line");
    let graphics = Rc::new(PolylineGraphics::new());
    let positions = DynamicValue::Positions(Rc::new(vec![position, position + Cartesian3::UNIT_Y]));
    let callback = CallbackProperty::new(Rc::new(move |_, _| Ok(Some(positions.clone()))), false);
    graphics.set_positions(Some(PropertyInput::Component(PropertyComponent::wrap(Rc::new(callback))))).unwrap();
    graphics.set_arc_type(Some(PropertyInput::value(ArcType::None as u8 as f64))).unwrap();
    line.set_property("polyline", Some(PropertyInput::Component(graphics))).unwrap();
    let samples = Rc::new(SampledPositionProperty::new(ReferenceFrame::Fixed, 0));
    samples.add_sample(frame.time.add_seconds(-1.0), &position, None).unwrap();
    samples.add_sample(frame.time.add_seconds(1.0), &(position + Cartesian3::UNIT_Z), None).unwrap();
    line.set_position(Some(samples)).unwrap();
    let path = Rc::new(PathGraphics::new());
    path.set_lead_time(Some(PropertyInput::value(1.0))).unwrap();
    path.set_trail_time(Some(PropertyInput::value(1.0))).unwrap();
    line.set_property("path", Some(PropertyInput::Component(path))).unwrap();
    source.entities().add(line).unwrap();

    assert!(!display.update(&frame.time).unwrap());
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(2);
    loop {
        frame.command_list.clear();
        render_scene.primitives[0].update(&mut frame).unwrap();
        for callback in std::mem::take(&mut frame.after_render) {
            callback().unwrap();
        }
        if display.update(&frame.time).unwrap() {
            break;
        }
        assert!(std::time::Instant::now() < deadline, "geometry batch failed to become ready");
        std::thread::yield_now();
    }
    frame.command_list.clear();
    render_scene.primitives[0].update(&mut frame).unwrap();
    let box_draw = frame.command_list.iter().find(|command| command.id.starts_with("Primitive#")).unwrap();
    assert_eq!(box_draw.metadata.bounding_volume.unwrap().center, position);
    assert!(frame.command_list.iter().any(|command| command.id.starts_with("polyline-collection-")));

    let mut paths = PathVisualizer::<RecordingGl>::new(SceneMode::Scene3D, source.entities().clone());
    paths.update(&frame.time).unwrap();
    frame.command_list.clear();
    paths.update_frame(&mut frame).unwrap();
    assert!(frame.command_list.iter().any(|command| command.id.starts_with("polyline-collection-")));
    paths.destroy();

    assert_eq!(display.get_bounding_sphere(&entity, false, &mut BoundingSphere::default()), BoundingSphereState::Done,);
    display.destroy();
    assert!(host.primitives.borrow().is_empty());
}

fn sphere(value: Option<BoundingSphere>) -> Value {
    value.map_or(Value::Null, |s| json!([s.center.x, s.center.y, s.center.z, s.radius]))
}

fn geometry(spec: &Value) -> Geometry {
    let base = c3(&spec["base"]);
    let points = [base, base + Cartesian3::new(32., 64., 0.), base + Cartesian3::new(0., 0., 64.)];
    let values: Vec<_> = points.iter().flat_map(|p| [p.x, p.y, p.z]).collect();
    let position = if spec["rtc"].as_bool().unwrap_or_default() {
        GeometryAttribute::floats(3, &values)
    } else {
        GeometryAttribute::doubles(3, values)
    };
    let mut attributes = GeometryAttributes { position: Some(position), ..Default::default() };
    if spec["vectors"].as_bool().unwrap_or_default() {
        attributes.normal = Some(GeometryAttribute::floats(3, &[1., 0., 0.].repeat(3)));
        attributes.st = Some(GeometryAttribute::floats(2, &[0., 0., 1., 0., 0., 1.]));
        attributes.tangent = Some(GeometryAttribute::floats(3, &[0., 1., 0.].repeat(3)));
        attributes.bitangent = Some(GeometryAttribute::floats(3, &[0., 0., 1.].repeat(3)));
    }
    let offset = spec["offsetAttribute"].as_f64().and_then(GeometryOffsetAttribute::unpack);
    if let Some(offset) = offset {
        let values = if offset == GeometryOffsetAttribute::Top { vec![0, 1, 1] } else { vec![1, 1, 1] };
        attributes
            .set("applyOffset", GeometryAttribute::new(ComponentDatatype::UnsignedByte, 1, TypedArray::Uint8(values)));
    }
    let mut geometry = Geometry::new(attributes, Some(TypedArray::Uint16(vec![0, 1, 2])), PrimitiveType::Triangles);
    geometry.bounding_sphere = Some(BoundingSphere::from_points(&points));
    geometry.offset_attribute = offset;
    geometry.bounding_sphere_cv =
        (!spec["boundingSphereCV"].is_null()).then(|| common::bsphere(&spec["boundingSphereCV"]));
    geometry
}

fn instance_attribute(name: &str, value: &Value) -> GeometryInstanceAttribute {
    let (component, normalize) = match name {
        "color" | "depthFailColor" => (ComponentDatatype::UnsignedByte, true),
        "show" => (ComponentDatatype::UnsignedByte, false),
        _ => (ComponentDatatype::Float, false),
    };
    let values = nums(value);
    GeometryInstanceAttribute::new(
        component,
        values.len() as u8,
        Some(normalize),
        TypedArray::from_values(component, &values),
    )
}

fn instance(spec: &Value) -> GeometryInstance {
    let mut result = GeometryInstance::new(geometry(spec), None);
    result.id = spec["id"].as_str().map(str::to_owned);
    if let Some(attributes) = spec["attributes"].as_object() {
        result.attributes = attributes.iter().map(|(n, v)| (n.clone(), instance_attribute(n, v))).collect();
    }
    result
}

fn options(spec: &Value) -> PrimitiveOptions {
    let closed = spec["closed"].as_bool().unwrap_or_default();
    let flat = spec["flat"].as_bool().unwrap_or(!closed);
    let translucent = spec["translucent"].as_bool().unwrap_or_default();
    let color = || {
        PerInstanceColorAppearance::new(PerInstanceColorAppearanceOptions {
            closed: Some(closed),
            flat: Some(flat),
            translucent: Some(translucent),
            ..Default::default()
        })
    };
    let appearance: Box<dyn Appearance> = match spec["material"].as_str() {
        Some(kind) if spec["appearance"] == "ellipsoid" => {
            Box::new(EllipsoidSurfaceAppearance::new(EllipsoidSurfaceAppearanceOptions {
                flat: Some(flat),
                translucent: Some(translucent),
                material: Some(Material::from_type(kind)),
                ..Default::default()
            }))
        }
        Some(kind) => Box::new(MaterialAppearance::new(MaterialAppearanceOptions {
            closed: Some(closed),
            flat: Some(flat),
            translucent: Some(translucent),
            material: Some(Material::from_type(kind)),
            ..Default::default()
        })),
        None => Box::new(color()),
    };
    PrimitiveOptions {
        geometry_instances: common::list(&spec["instances"], instance),
        appearance: Some(appearance),
        depth_fail_appearance: spec["depthFail"]
            .as_bool()
            .unwrap_or_default()
            .then(|| Box::new(color()) as Box<dyn Appearance>),
        asynchronous: Some(false),
        rtc_center: (!spec["rtcCenter"].is_null()).then(|| c3(&spec["rtcCenter"])),
        allow_picking: spec["allowPicking"].as_bool(),
        interleave: spec["interleave"].as_bool(),
        compress_vertices: spec["compressVertices"].as_bool(),
        cull: spec["cull"].as_bool(),
        shadows: spec["shadows"].as_i64().and_then(|n| ShadowMode::from_value(n as i32)),
        debug_show_bounding_volume: spec["debugShowBoundingVolume"].as_bool(),
        create_pick_offsets: spec["createPickOffsets"].as_bool().unwrap_or_default(),
        ..Default::default()
    }
}

enum Target {
    Primitive(Box<Primitive<RecordingGl>>),
    Camera(DebugCameraPrimitive<RecordingGl>),
    ModelMatrix(Box<DebugModelMatrixPrimitive<RecordingGl>>),
}

impl Target {
    fn new(spec: &Value) -> Self {
        match spec["kind"].as_str() {
            Some("camera") => Self::Camera(DebugCameraPrimitive::new(DebugCameraPrimitiveOptions {
                frustum_splits: Some(vec![1., 10., 32.]),
                id: Some("camera".into()),
                update_on_change: spec["updateOnChange"].as_bool(),
                ..Default::default()
            })),
            Some("modelMatrix") => {
                Self::ModelMatrix(Box::new(DebugModelMatrixPrimitive::new(DebugModelMatrixPrimitiveOptions {
                    length: Some(64.),
                    width: Some(3.),
                    id: Some("axes".into()),
                    ..Default::default()
                })))
            }
            Some("tangentSpace") => Self::Primitive(Box::new(
                create_tangent_space_debug_primitive(CreateTangentSpaceDebugPrimitiveOptions {
                    geometry: geometry(&spec["instances"][0]),
                    length: Some(32.),
                    model_matrix: None,
                })
                .unwrap(),
            )),
            _ => Self::Primitive(Box::new(Primitive::new(options(spec)))),
        }
    }

    fn update(&mut self, step: &Value, spec: &Value, frame: &mut FrameState) {
        match self {
            Self::Primitive(p) => {
                if !step["translation"].is_null() {
                    p.model_matrix = Matrix4::from_translation(c3(&step["translation"]));
                }
                if !step["materialColor"].is_null() {
                    let material = p.appearance.as_mut().unwrap().material_mut().unwrap();
                    material
                        .set_uniform("color", MaterialValue::from_color(common::color(&step["materialColor"])))
                        .unwrap();
                }
                if let Some(mutation) = step["mutate"].as_object() {
                    let id = spec["instances"][0]["id"].as_str().unwrap();
                    let mut attributes = p.get_geometry_instance_attributes(id).unwrap().unwrap();
                    for (name, value) in mutation {
                        attributes.set(name, &instance_attribute(name, value).value).unwrap();
                    }
                }
                p.update(frame).unwrap();
            }
            Self::Camera(p) => ScenePrimitive::update(p, frame).unwrap(),
            Self::ModelMatrix(p) => {
                if let Some(length) = step["length"].as_f64() {
                    p.length = length;
                }
                p.update(frame).unwrap();
            }
        }
    }

    fn primitives(&self) -> Vec<&Primitive<RecordingGl>> {
        match self {
            Self::Primitive(p) => vec![p],
            Self::Camera(p) => p
                .outline_primitives()
                .iter()
                .zip(p.planes_primitives())
                .flat_map(|(outline, planes)| [outline, planes])
                .collect(),
            Self::ModelMatrix(p) => p.primitive().into_iter().collect(),
        }
    }

    fn ready(&self) -> Option<bool> {
        match self {
            Self::Primitive(p) => Some(p.is_ready()),
            _ => None,
        }
    }

    fn readbacks(&mut self, spec: &Value) -> Vec<Value> {
        let Self::Primitive(p) = self else { return vec![] };
        if spec["kind"] == "tangentSpace" {
            return vec![];
        }
        spec["instances"].as_array().unwrap().iter().map(|instance| {
            let id = instance["id"].as_str().unwrap();
            let attrs = p.get_geometry_instance_attributes(id).unwrap().unwrap();
            let values: BTreeMap<_, _> = instance["attributes"].as_object().into_iter().flatten().filter_map(|(name, _)| {
                attrs.get(name).unwrap().map(|value| (name, value.to_f64_vec()))
            }).collect();
            json!({
                "id": id, "attributes": values,
                "boundingSphere": sphere(attrs.bounding_sphere()), "boundingSphereCV": sphere(attrs.bounding_sphere_cv()),
                "pickKey": attrs.pick_id().key,
                "pickColor": attrs.pick_id().color.to_bytes().map(|byte| f64::from(byte) / 255.0),
            })
        }).collect()
    }
}

fn read_shader(shader: &ShaderProgram<RecordingGl>) -> Value {
    let options = shader.source_options().unwrap();
    let mut active: Vec<_> = shader.vertex_attributes().iter().map(|a| &a.name).collect();
    active.sort();
    json!({
        "vertex": options.vertex_shader_source.sources[0],
        "fragment": options.fragment_shader_source.sources[0],
        "locations": options.attribute_locations,
        "active": active,
    })
}

fn uniform_value(value: &UniformValue) -> Value {
    match value {
        UniformValue::Matrix { values, .. } | UniformValue::Float(values) => json!(values),
        UniformValue::Int(values) => json!(values),
        _ => panic!("Unexpected captured uniform {value:?}"),
    }
}

fn read_command(command: &PrimitiveCommand, program: usize) -> Value {
    let meta = &command.metadata;
    json!({
        "pass": command.pass as u32, "primitiveType": command.primitive_type as u32, "program": program,
        "renderState": command.render_state,
        "vertexArray": command.vertex_array, "boundingVolume": sphere(meta.bounding_volume),
        "modelMatrix": meta.model_matrix.unwrap().0,
        "cull": meta.cull, "debugShowBoundingVolume": meta.debug_show_bounding_volume,
        "castShadows": meta.cast_shadows, "receiveShadows": meta.receive_shadows,
        "pickId": meta.pick_id, "uniforms": command.uniforms,
        "uniformValues": command.uniform_values.iter().map(|(name, v)| (name.clone(), uniform_value(v))).collect::<BTreeMap<_, _>>(),
    })
}

fn compare(actual: &Value, expected: &Value, path: &str) {
    match (actual, expected) {
        (Value::Number(a), Value::Number(e)) => {
            let (a, e) = (a.as_f64().unwrap(), e.as_f64().unwrap());
            let tolerance = if path.contains("uniformValues") { 5e-8 * e.abs().max(1.) } else { 3e-8 };
            assert!((a - e).abs() <= tolerance, "{path}: {a} != {e}");
        }
        (Value::Object(a), Value::Object(e)) => {
            assert_eq!(a.len(), e.len(), "{path}");
            for (key, v) in e {
                compare(&a[key], v, &format!("{path}.{key}"));
            }
        }
        (Value::Array(a), Value::Array(e)) => {
            assert_eq!(a.len(), e.len(), "{path}: {a:?} != {e:?}");
            for (i, (a, e)) in a.iter().zip(e).enumerate() {
                compare(a, e, &format!("{path}[{i}]"));
            }
        }
        (Value::String(a), Value::String(e)) if path.ends_with(".vertex") || path.ends_with(".fragment") => {
            if a != e {
                let line = a
                    .split('\n')
                    .zip(e.split('\n'))
                    .position(|(a, e)| a != e)
                    .unwrap_or_else(|| a.lines().count().min(e.lines().count()));
                panic!(
                    "{path} line {}\nactual:   {:?}\nexpected: {:?}",
                    line + 1,
                    a.split('\n').nth(line),
                    e.split('\n').nth(line)
                );
            }
        }
        _ => assert_eq!(actual, expected, "{path}"),
    }
}

fn submit(frame: &FrameState, draw_mode: SceneDrawMode) {
    for command in &frame.command_list {
        (command.execute)(SceneSubmission {
            pass: command.pass,
            frustum: 0,
            near: 1.,
            far: 32.,
            draw_mode,
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
}

fn replay(name: &str) {
    let fixture: Value = serde_json::from_str(include_str!("data/scene-primitive.json")).unwrap();
    assert_eq!(fixture["engine"], "23.0.0");
    assert!(fixture["renderer"].as_str().unwrap().contains("SwiftShader"));
    let spec = &fixture["scenarios"][name];
    let expected = &fixture["capture"][name];
    let context = Context::new_webgl2(RecordingGl {
        active_attributes: common::list(&expected["shaders"], |s| {
            common::list(&s["active"], |name| name.as_str().unwrap().into())
        }),
        ..Default::default()
    });
    let mut frame = frame(&context);
    frame.scene_3d_only = spec["scene3DOnly"].as_bool().unwrap_or_default();
    let mut target = Target::new(spec);
    let mut shaders = Vec::new();
    for (i, step) in spec["steps"].as_array().unwrap().iter().enumerate() {
        let path = format!("{name}[{i}]");
        let expected_step = &expected["steps"][i];
        frame.mode = SceneMode::from_value(step["mode"].as_i64().unwrap() as i32).unwrap();
        frame.passes.pick = step["pick"].as_bool().unwrap_or_default();
        frame.passes.render = !frame.passes.pick;
        frame.command_list.clear();
        context.host().calls.borrow_mut().clear();
        target.update(step, spec, &mut frame);
        let ready_before = target.ready();
        let ready_callbacks: Vec<_> = frame.after_render.drain(..).map(|callback| callback().unwrap()).collect();
        let mut commands = Vec::new();
        for primitive in target.primitives() {
            for command in primitive.commands() {
                let shader = read_shader(primitive.shader_program(command.depth_fail).unwrap());
                let program = shaders.iter().position(|s| s == &shader).unwrap_or_else(|| {
                    let program = shaders.len();
                    compare(&shader, &expected["shaders"][program], &format!("{name}.shaders[{program}]"));
                    shaders.push(shader);
                    program
                });
                assert_eq!(frame.command_list[commands.len()].metadata, command.metadata);
                commands.push(read_command(command, program));
            }
        }
        let pick_offsets: Vec<_> = target
            .primitives()
            .iter()
            .flat_map(|p| p.pick_offsets())
            .map(|p| json!({"index": p.index, "offset": p.offset, "count": p.count}))
            .collect();
        assert_eq!(frame.command_list.len(), commands.len());
        submit(&frame, SceneDrawMode::default());
        assert_eq!(context.host().draws.borrow().len(), commands.len(), "{path}");
        for (draw, command) in context.host().draws.borrow().iter().zip(&mut commands) {
            assert_eq!(draw[1][0], command["primitiveType"], "{path}");
            let index = if draw[0] == "drawElements" { 1 } else { 2 };
            command["count"] = draw[1][index].clone();
        }
        let actual = json!({
            "uploads": *context.host().calls.borrow(), "commands": commands,
            "readyBefore": ready_before, "readyAfter": target.ready(), "requestedRender": ready_callbacks,
            "readbacks": target.readbacks(spec), "pickOffsets": pick_offsets,
        });
        compare(&actual, expected_step, &path);
        context.host().draws.borrow_mut().clear();
    }
    assert_eq!(shaders.len(), expected["shaders"].as_array().unwrap().len(), "{name}");
}

macro_rules! capture_cases {
    ($($test:ident => $case:literal),+ $(,)?) => { $(
        #[test]
        fn $test() { replay($case); }
    )+ };
}

capture_cases! {
    projected_attribute_updates_and_modes => "projected",
    closed_translucent_depth_fail => "closedDepthFail",
    uncompressed_color_material => "colorMaterial",
    interleaved_checkerboard => "checkerboard",
    compressed_stripe_material => "stripe",
    antimeridian_split => "antimeridian",
    relative_to_center => "rtc",
    debug_camera => "camera",
    static_debug_camera => "cameraStatic",
    debug_model_matrix => "modelMatrix",
    tangent_space => "tangentSpace",
}

fn triangle_options() -> PrimitiveOptions {
    options(&json!({
        "flat": true,
        "instances": [{"id": "triangle", "base": [6378147, 0, 0], "attributes": {"color": [255, 0, 0, 255]}}],
    }))
}

fn advance_worker(primitive: &mut Primitive<RecordingGl>, frame: &mut FrameState, state: PrimitiveState) {
    let deadline = std::time::Instant::now() + std::time::Duration::from_secs(5);
    while primitive.state() != state {
        primitive.update(frame).unwrap();
        assert!(std::time::Instant::now() < deadline, "waiting for {state:?}, at {:?}", primitive.state());
        if primitive.state() != state {
            std::thread::sleep(std::time::Duration::from_millis(1));
        }
    }
}

#[test]
fn asynchronous_worker_creates_combines_and_releases_instances() {
    let context = Context::new_webgl2(RecordingGl::default());
    let mut frame = frame(&context);
    frame.scene_3d_only = true;
    let mut options = triangle_options();
    options.asynchronous = Some(true);
    options.geometry_instances[0].geometry = None;
    options.geometry_descriptions.insert(
        0,
        cesium_rs::workers::GeometryDescription::BoxGeometry(
            BoxGeometry::from_dimensions(Cartesian3::new(4., 6., 8.), VertexFormat::POSITION_ONLY, None).unwrap(),
        ),
    );
    let mut primitive = Primitive::<RecordingGl>::new(options);
    assert_eq!(primitive.state(), PrimitiveState::Ready);
    assert!(primitive.get_geometry_instance_attributes("triangle").is_err());
    for state in
        [PrimitiveState::Creating, PrimitiveState::Created, PrimitiveState::Combining, PrimitiveState::Combined]
    {
        advance_worker(&mut primitive, &mut frame, state);
        assert!(!primitive.is_ready());
        assert!(frame.command_list.is_empty());
        assert!(frame.after_render.is_empty());
    }
    primitive.update(&mut frame).unwrap();
    assert_eq!(primitive.state(), PrimitiveState::Complete);
    assert!(primitive.geometry_instances().is_none());
    assert_eq!(frame.command_list.len(), 1);
    assert!(!primitive.is_ready());
    assert!(frame.after_render.pop().unwrap()().unwrap());
    assert!(primitive.is_ready());
}

#[test]
fn worker_failure_is_terminal_and_marks_ready_after_render() {
    let context = Context::new_webgl2(RecordingGl::default());
    let mut frame = frame(&context);
    frame.scene_3d_only = true;
    let mut options = triangle_options();
    options.asynchronous = Some(true);
    let mut lines = options.geometry_instances[0].clone();
    lines.geometry.as_mut().unwrap().primitive_type = PrimitiveType::Lines;
    options.geometry_instances.push(lines);
    let mut primitive = Primitive::<RecordingGl>::new(options);
    advance_worker(&mut primitive, &mut frame, PrimitiveState::Failed);
    assert!(!primitive.is_ready());
    assert!(frame.command_list.is_empty());
    assert!(frame.after_render.pop().unwrap()().unwrap());
    assert!(primitive.is_ready());
    assert!(primitive.update(&mut frame).unwrap_err().to_string().contains("same primitiveType"));
}

#[test]
fn destroy_releases_pick_texture_arrays_and_cached_programs() {
    let context = Context::new_webgl2(RecordingGl::default());
    let mut frame = frame(&context);
    frame.scene_3d_only = true;
    let mut primitive = Primitive::<RecordingGl>::new(triangle_options());
    primitive.update(&mut frame).unwrap();
    let key = primitive.get_geometry_instance_attributes("triangle").unwrap().unwrap().pick_id().key;
    assert_eq!(context.pick_object::<PrimitivePickObject>(key).unwrap().id.as_deref(), Some("triangle"));
    frame.command_list.clear();
    primitive.destroy().unwrap();
    assert!(!frame.after_render.pop().unwrap()().unwrap());
    assert!(!primitive.is_ready());
    assert!(primitive.is_destroyed());
    assert!(context.get_object_by_pick_color(key).is_none());
    assert!(primitive.update(&mut frame).is_err());
    context.with_shader_cache(|cache| cache.destroy_released_shader_programs()).unwrap();
    for call in ["deleteVertexArray", "deleteTexture", "deleteBuffer", "deleteProgram"] {
        assert!(context.host().deleted.borrow().iter().any(|name| name == call), "{call}");
    }
}

#[test]
fn repeated_updates_queue_independent_draws_without_reuploading() {
    let context = Context::new_webgl2(RecordingGl::default());
    let mut frame = frame(&context);
    frame.scene_3d_only = true;
    let mut primitive = Primitive::<RecordingGl>::new(triangle_options());
    primitive.show = false;
    primitive.update(&mut frame).unwrap();
    assert_eq!(primitive.state(), PrimitiveState::Complete);
    assert!(frame.command_list.is_empty());
    context.host().calls.borrow_mut().clear();
    primitive.show = true;
    primitive.update(&mut frame).unwrap();
    primitive.model_matrix = Matrix4::from_translation(Cartesian3::new(5., 0., 0.));
    primitive.update(&mut frame).unwrap();
    assert!(context.host().calls.borrow().is_empty());
    assert_eq!(context.host().programs.borrow().len(), 1);
    assert_eq!(frame.command_list.len(), 2);
    assert_eq!(frame.after_render.len(), 1);
    assert_eq!(frame.command_list[0].metadata.model_matrix, Some(Matrix4::IDENTITY));
    assert_eq!(frame.command_list[1].metadata.model_matrix, Some(primitive.model_matrix));
    submit(&frame, SceneDrawMode { pick: true, ..Default::default() });
    assert_eq!(context.host().draws.borrow().len(), 2);
    assert_eq!(frame.command_list[0].metadata.pick_id.as_deref(), Some("v_pickColor"));
}

#[test]
fn destroying_during_geometry_creation_drops_in_flight_results() {
    let context = Context::new_webgl2(RecordingGl::default());
    let mut frame = frame(&context);
    frame.scene_3d_only = true;
    let mut options = triangle_options();
    options.asynchronous = Some(true);
    let mut primitive = Primitive::<RecordingGl>::new(options);
    primitive.update(&mut frame).unwrap();
    assert_eq!(primitive.state(), PrimitiveState::Creating);
    primitive.destroy().unwrap();
    assert!(frame.after_render.is_empty());
    assert!(frame.command_list.is_empty());
    assert!(context.get_object_by_pick_color(1).is_none());
    assert!(primitive.update(&mut frame).is_err());
}

#[test]
fn debug_camera_draws_off_center_frustum_geometry() {
    use cesium_rs::renderer::UniformFrustum;

    let context = Context::new_webgl2(RecordingGl::default());
    let mut frame = frame(&context);
    frame.scene_3d_only = true;
    frame.uniform.camera.frustum =
        UniformFrustum::PerspectiveOffCenter(PerspectiveOffCenterFrustum::new(-1., 2., 1., -1., 1., 32.));
    let mut camera = DebugCameraPrimitive::<RecordingGl>::default();
    ScenePrimitive::update(&mut camera, &mut frame).unwrap();
    assert_eq!(frame.command_list.len(), 2);
    let outline = frame.command_list[0].metadata.bounding_volume.unwrap();
    assert!(outline.center.y > 0.);
    submit(&frame, SceneDrawMode::default());
    assert_eq!(context.host().draws.borrow().len(), 2);
}

#[test]
fn debug_camera_tracks_a_camera_independent_of_the_scene_camera() {
    let context = Context::new_webgl2(RecordingGl::default());
    let mut frame = frame(&context);
    frame.scene_3d_only = true;
    let mut camera = Camera::new(CameraOptions::new(8., 8.));
    let camera_state = &frame.uniform.camera;
    camera.position = camera_state.position + Cartesian3::new(1000., 0., 0.);
    camera.direction = camera_state.direction;
    camera.up = camera_state.up;
    camera.right = camera_state.right;
    let cesium_rs::renderer::UniformFrustum::Perspective(frustum) = camera_state.frustum else { unreachable!() };
    camera.frustum = CameraFrustum::Perspective(frustum);
    let camera = std::rc::Rc::new(RefCell::new(camera));
    let mut debug = DebugCameraPrimitive::<RecordingGl>::with_camera(Default::default(), camera.clone());
    ScenePrimitive::update(&mut debug, &mut frame).unwrap();
    let initial = frame.command_list[0].metadata.bounding_volume.unwrap();
    assert!(initial.center.x > frame.uniform.camera.position.x);

    frame.command_list.clear();
    camera.borrow_mut().position.x += 500.;
    ScenePrimitive::update(&mut debug, &mut frame).unwrap();
    let moved = frame.command_list[0].metadata.bounding_volume.unwrap();
    assert!((moved.center.x - initial.center.x - 500.).abs() < 1e-8);
}
