mod common;
use cesium_rs::{
    core::*,
    renderer::*,
    scene::{model::*, model_components as mc, *},
};
use common::*;
use serde_json::{json, Value};
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    rc::Rc,
};

#[derive(Clone, Default)]
struct Host {
    next: Rc<Cell<u32>>,
    calls: Rc<RefCell<Vec<String>>>,
    bound: Rc<Cell<u32>>,
    buffers: Rc<RefCell<HashMap<u32, Vec<u8>>>>,
}
impl Gl for Host {
    type Object = u32;
    fn drawing_buffer_size(&self) -> Result<[u32; 2], RuntimeError> {
        Ok([8, 8])
    }
    fn invoke(&self, name: &str, args: &[GlValue<u32>]) -> Result<GlValue<u32>, RuntimeError> {
        self.calls.borrow_mut().push(name.into());
        if name.starts_with("create") {
            self.next.set(self.next.get() + 1);
            return Ok(GlValue::Object(self.next.get()));
        }
        if name == "bindBuffer" {
            if let Some(GlValue::Object(id)) = args.get(1) {
                self.bound.set(*id);
            }
        }
        if name == "bufferData" {
            if let Some(GlValue::Bytes(data)) = args.get(1) {
                self.buffers.borrow_mut().insert(self.bound.get(), data.clone());
            }
        }
        if name == "getBufferSubData" {
            let GlValue::Number(offset) = args[1] else { panic!() };
            let GlValue::Bytes(data) = &args[2] else { panic!() };
            return Ok(GlValue::Bytes(
                self.buffers.borrow()[&self.bound.get()][offset as usize..offset as usize + data.len()].to_vec(),
            ));
        }
        Ok(match name {
            "getParameter" => match args.first() {
                Some(GlValue::Number(v))
                    if [
                        webgl_constants::ALIASED_LINE_WIDTH_RANGE,
                        webgl_constants::ALIASED_POINT_SIZE_RANGE,
                        webgl_constants::MAX_VIEWPORT_DIMS,
                    ]
                    .contains(&(*v as u32)) =>
                {
                    GlValue::Numbers(vec![1., 16384.])
                }
                _ => GlValue::Number(16384.),
            },
            "getShaderParameter" => GlValue::Bool(true),
            "getProgramParameter" if matches!(args.get(1),Some(GlValue::Number(v)) if *v==webgl_constants::LINK_STATUS as f64) => {
                GlValue::Bool(true)
            }
            "getProgramParameter" => GlValue::Number(0.),
            _ => GlValue::Null,
        })
    }
}
fn frame() -> FrameState {
    let mut camera = Camera::new(CameraOptions::new(8., 8.));
    let uniform = camera.uniform_camera();
    let culling = camera.frustum.compute_culling_volume(uniform.position, uniform.direction, uniform.up);
    let mut frame =
        FrameState::new(UniformFrame::new(uniform, JulianDate::new(2451545., 0., TimeStandard::Tai)), culling, [8, 8]);
    frame.passes.render = true;
    frame.stencil_buffer = true;
    frame
}
fn command() -> ModelCommand<Host> {
    let mut c = ModelCommand::default();
    c.state.cull.enabled = true;
    c.state.depth_test.enabled = true;
    c.metadata.model_matrix = Some(Matrix4::IDENTITY);
    c.metadata.bounding_volume = Some(BoundingSphere::new(Cartesian3::ZERO, 1.));
    c.metadata.cast_shadows = true;
    c.metadata.receive_shadows = true;
    c.count = Some(12);
    c
}
fn snapshot(c: &ModelCommand<Host>) -> Value {
    let r = &c.state;
    json!({"pass":c.pass as u32,"count":c.count,"offset":c.offset,"matrix":c.metadata.model_matrix.map(|m|m.enc()),
        "sphere":c.metadata.bounding_volume.map(|s|s.enc()),"cull":r.cull,"depthMask":r.depth_mask,"depthTest":r.depth_test,
        "colorMask":r.color_mask,"stencilMask":r.stencil_mask,"stencil":r.stencil_test,"polygonOffset":r.polygon_offset,"blending":r.blending.enabled,
        "cast":c.metadata.cast_shadows,"receive":c.metadata.receive_shadows,"debug":c.metadata.debug_show_bounding_volume,
        "pickOnly":c.metadata.pick_only,"cullCommand":c.metadata.cull,"pickId":c.metadata.pick_id})
}
#[test]
fn ordinary_derived_commands_match_cesium() {
    run(
        "scene",
        "ModelDrawCommand",
        |_| TIGHT,
        |_, a| {
            let c = &a[0];
            let mut base = command();
            if c["translucent"] == true {
                base.pass = Pass::Translucent;
            }
            if let Some(idl) = c["idl"].as_f64() {
                base.metadata.model_matrix.as_mut().unwrap().0[13] = idl * std::f64::consts::PI * 6378137.;
            }
            let mut draw = ModelDrawCommand::new(
                base,
                BoundingSphere::new(Cartesian3::ZERO, 1.),
                ModelDrawCommandOptions {
                    double_sided: c["doubleSided"] == true,
                    has_silhouette: c["silhouette"] == true,
                    has_skip_level_of_detail: c["skip"] == true,
                    silhouette_id: 260,
                    silhouette_alpha: c["silhouetteAlpha"].as_f64().unwrap_or(1.),
                    invisible: c["invisible"] == true,
                    ..Default::default()
                },
            );
            if let Some(t) = c.get("translate") {
                draw.set_model_matrix(Matrix4::from_translation(c3(t)));
            }
            if let Some(s) = c["shadows"].as_i64() {
                draw.set_shadow_mode(ShadowMode::from_value(s as i32).unwrap());
            }
            if let Some(v) = c["cull"].as_bool() {
                draw.set_back_face_culling(v);
            }
            if let Some(v) = c["debug"].as_bool() {
                draw.set_debug_show_bounding_volume(v);
            }
            let mut f = frame();
            f.mode = SceneMode::from_value(c["mode"].as_u64().unwrap_or(3) as i32).unwrap();
            f.passes.pick = c["pick"] == true;
            let selection = ModelCommandSelection {
                style: c["style"].as_i64().and_then(|s| StyleCommandsNeeded::from_value(s as i32)),
                project_to_2d: c["project"] == true,
                bounding_sphere_2d: Some(BoundingSphere::new(
                    Cartesian3::new(0., c["idl"].as_f64().unwrap_or(0.) * std::f64::consts::PI * 6378137., 0.),
                    10.,
                )),
                mixed_content: c["mixed"] == true,
                final_resolution: c["final"] == true,
                selection_depth: c["depth"].as_u64().unwrap_or(0) as u32,
            };
            let result = draw.commands(&f, &selection);
            ok(
                json!({"commands":result.commands.iter().map(snapshot).collect::<Vec<_>>(),"backfaces":result.backfaces.iter().map(snapshot).collect::<Vec<_>>(),"silhouettes":result.silhouettes.iter().map(snapshot).collect::<Vec<_>>()}),
            )
        },
    );
}
#[test]
fn classification_batches_match_cesium() {
    run(
        "scene",
        "ClassificationModelDrawCommand",
        |_| EXACT,
        |_, a| {
            let c = &a[0];
            let draw = ClassificationModelDrawCommand::new(
                command(),
                BoundingSphere::new(Cartesian3::ZERO, 1.),
                ClassificationType::from_value(c["type"].as_i64().unwrap() as i32).unwrap(),
                vec![3, 9],
                vec![0, 3],
                c["allow"] == true,
                Some("u_pickColor".into()),
            )
            .unwrap();
            let mut f = frame();
            f.passes.render = c["render"] == true;
            f.passes.pick = c["pick"] == true;
            f.invert_classification = c["invert"] == true;
            ok(json!(draw.commands(&f).iter().map(snapshot).collect::<Vec<_>>()))
        },
    );
}
#[test]
fn model_visibility_matches_cesium() {
    run(
        "scene",
        "ModelProperties",
        |_| EXACT,
        |_, a| {
            let c = &a[0];
            let options = ModelOptions {
                color: c["alpha"].as_f64().map(|a| Color::new(1., 1., 1., a)),
                classification_type: c["classification"]
                    .as_i64()
                    .and_then(|i| ClassificationType::from_value(i as i32)),
                silhouette_size: n(&c["size"]),
                ..Default::default()
            };
            let model =
                Model::from_components(Context::new_webgl2(Host::default()), mc::Components::default(), options)
                    .unwrap();
            let mut f = frame();
            f.stencil_buffer = c["stencil"] == true;
            ok(json!([model.is_translucent(), model.is_invisible(), model.has_silhouette(&f)]))
        },
    );
}
fn triangle_components() -> mc::Components<Host> {
    let primitive = mc::Primitive {
        primitive_type: Some(PrimitiveType::Triangles),
        attributes: vec![mc::Attribute {
            semantic: Some(mc::AttributeSemantic::Vertex(VertexAttributeSemantic::Position)),
            attribute_type: Some(AttributeType::Vec3),
            component_datatype: Some(ComponentDatatype::Float),
            count: Some(3),
            typed_array: Some(TypedArray::Float32(vec![-1., -1., 0., 1., -1., 0., 0., 1., 0.])),
            min: Some(mc::MathValue::Cartesian3(Cartesian3::new(-1., -1., 0.))),
            max: Some(mc::MathValue::Cartesian3(Cartesian3::new(1., 1., 0.))),
            ..Default::default()
        }],
        indices: Some(mc::Indices {
            index_datatype: Some(IndexDatatype::UnsignedShort),
            count: Some(3),
            typed_array: Some(TypedArray::Uint16(vec![0, 1, 2])),
            buffer: None,
        }),
        ..Default::default()
    };
    mc::Components {
        up_axis: Some(Axis::Z),
        forward_axis: Some(Axis::X),
        scene: Some(mc::Scene { nodes: vec![0] }),
        nodes: vec![mc::Node {
            index: Some(0),
            name: Some("root".into()),
            primitives: vec![primitive],
            ..Default::default()
        }],
        ..Default::default()
    }
}
#[test]
fn cpu_picking_matches_cesium() {
    run(
        "scene",
        "pickModel",
        |_| TIGHT,
        |_, a| {
            let c = &a[0];
            let mut components = triangle_components();
            if c["quantized"] == true {
                components.nodes[0].primitives[0].attributes[0].quantization = Some(mc::Quantization {
                    component_datatype: Some(ComponentDatatype::Float),
                    attribute_type: Some(AttributeType::Vec3),
                    quantized_volume_step_size: Some(mc::MathValue::Cartesian3(Cartesian3::new(2., 2., 2.))),
                    quantized_volume_offset: Some(mc::MathValue::Cartesian3(Cartesian3::new(0., 0., 1.))),
                    ..Default::default()
                });
            }
            let translation = c3(&c["translation"]);
            let graph = ModelSceneGraph::<Host>::new(
                components,
                ModelSceneGraphOptions { model_matrix: Matrix4::from_translation(translation), ..Default::default() },
            )
            .unwrap();
            let origin = c3(&c["origin"]);
            let ray = Ray::new(origin.add(translation), Cartesian3::new(0., 0., if origin.z > 0. { -1. } else { 1. }));
            ok(pick_model(
                &graph,
                &ray,
                &frame(),
                ModelPickOptions { back_face_culling: c["backface"] == true, ..Default::default() },
            )
            .unwrap())
        },
    );
}
fn drawable_components(context: &Context<Host>) -> mc::Components<Host> {
    let mut c = triangle_components();
    let p = &mut c.nodes[0].primitives[0];
    let bytes = p.attributes[0].typed_array.as_ref().unwrap().to_le_bytes();
    p.attributes[0].buffer =
        Some(Buffer::new(context, BufferKind::Vertex, BufferUsage::StaticDraw, BufferData::Bytes(&bytes)).unwrap());
    p.indices.as_mut().unwrap().buffer = Some(
        Buffer::new(
            context,
            BufferKind::Index(IndexDatatype::UnsignedShort),
            BufferUsage::StaticDraw,
            BufferData::Bytes(&[0, 0, 1, 0, 2, 0]),
        )
        .unwrap(),
    );
    p.material = Some(mc::Material { unlit: true, ..Default::default() });
    c
}
fn finish_frame(f: &mut FrameState) {
    for callback in std::mem::take(&mut f.after_render) {
        callback().unwrap();
    }
}
#[test]
fn model_loads_defers_ready_updates_draws_and_releases_resources() {
    let host = Host::default();
    let context = Context::new_webgl2(host.clone());
    let mut model = Model::from_components(
        context.clone(),
        drawable_components(&context),
        ModelOptions { id: Some("triangle".into()), ..Default::default() },
    )
    .unwrap();
    let mut f = frame();
    model.update(&mut f).unwrap();
    assert!(!model.ready());
    assert!(f.command_list.is_empty());
    finish_frame(&mut f);
    assert!(model.ready());
    assert_eq!(model.statistics.triangles_length, 1);
    model.update(&mut f).unwrap();
    assert_eq!(f.command_list.len(), 1);
    assert_eq!(f.command_list[0].metadata.owner.as_deref(), Some("triangle"));
    f.command_list.clear();
    finish_frame(&mut f);
    model.options.model_matrix = Matrix4::from_translation(Cartesian3::new(10., 20., 30.));
    model.options.shadows = ShadowMode::CastOnly;
    model.update(&mut f).unwrap();
    assert_eq!(f.command_list[0].metadata.model_matrix.unwrap().get_translation(), Cartesian3::new(10., 20., 30.));
    assert!(f.command_list[0].metadata.cast_shadows);
    assert!(!f.command_list[0].metadata.receive_shadows);
    let ray = Ray::new(Cartesian3::new(10., 20., 35.), Cartesian3::new(0., 0., -1.));
    assert_eq!(model.pick(&ray, &f).unwrap(), Some(Cartesian3::new(10., 20., 30.)));
    f.command_list.clear();
    finish_frame(&mut f);
    model.options.show = false;
    model.update(&mut f).unwrap();
    assert!(f.command_list.is_empty());
    finish_frame(&mut f);
    model.destroy().unwrap();
    assert!(model.is_destroyed());
    assert!(model.update(&mut f).is_err());
    assert!(host.calls.borrow().iter().any(|s| s == "deleteProgram"));
}
#[test]
fn destroyed_model_does_not_fire_queued_ready_event() {
    let context = Context::new_webgl2(Host::default());
    let mut model =
        Model::from_components(context.clone(), drawable_components(&context), ModelOptions::default()).unwrap();
    let ready = std::sync::Arc::new(std::sync::atomic::AtomicBool::new(false));
    let seen = ready.clone();
    model.ready_event.borrow_mut().add_event_listener(move |_| {
        seen.store(true, std::sync::atomic::Ordering::SeqCst);
    });
    let mut f = frame();
    model.update(&mut f).unwrap();
    model.destroy().unwrap();
    finish_frame(&mut f);
    assert!(!ready.load(std::sync::atomic::Ordering::SeqCst));
    assert!(!model.ready());
}
#[test]
fn tile_content_processes_on_the_real_frame_and_submits_tile_pass() {
    let context = Context::new_webgl2(Host::default());
    let model =
        Model::from_components(context.clone(), drawable_components(&context), ModelOptions::default()).unwrap();
    let mut content = Model3DTileContent::new(model, 0, Resource::new("https://test/triangle.glb").unwrap());
    let mut f = frame();
    assert!(!content.process_with_frame(&mut f).unwrap());
    assert!(!content.ready());
    finish_frame(&mut f);
    assert!(content.process_with_frame(&mut f).unwrap());
    assert_eq!(content.metrics().triangles, 1);
    assert!(f.command_list.is_empty());
    finish_frame(&mut f);
    content.update(&mut f).unwrap();
    assert_eq!(f.command_list.len(), 1);
    assert_eq!(f.command_list[0].pass, Pass::Cesium3DTile);
    content.destroy().unwrap();
    assert!(content.is_destroyed());
}

#[test]
fn model_style_and_pass_changes_rebuild_commands() {
    let context = Context::new_webgl2(Host::default());
    let mut model =
        Model::from_components(context.clone(), drawable_components(&context), ModelOptions::default()).unwrap();
    let mut f = frame();
    model.update(&mut f).unwrap();
    finish_frame(&mut f);
    model.set_style(Some(Rc::new(RefCell::new(
        Cesium3DTileStyle::new(Some(&json!({"color":"color('red', 0.5)"}))).unwrap(),
    ))));
    model.update(&mut f).unwrap();
    assert_eq!(model.options.color, Some(Color::new(1., 0., 0., 0.5)));
    assert_eq!(f.command_list[0].pass, Pass::Translucent);
    f.command_list.clear();
    finish_frame(&mut f);
    model.set_style(None);
    model.options.opaque_pass = Pass::Cesium3DTile;
    model.update(&mut f).unwrap();
    assert_eq!(f.command_list[0].pass, Pass::Cesium3DTile);
    f.command_list.clear();
    finish_frame(&mut f);
    model.set_style(Some(Rc::new(RefCell::new(Cesium3DTileStyle::new(Some(&json!({"show":"false"}))).unwrap()))));
    model.update(&mut f).unwrap();
    assert!(!model.options.show);
    assert!(f.command_list.is_empty());
}
