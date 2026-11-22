//! Constructor and collection reference cases.
mod common;

use cesium_rs::{core::*, renderer::UniformFrame, scene::*};
use common::*;
use serde_json::json;
use std::{cell::RefCell, rc::Rc};

fn frame() -> FrameState {
    let mut camera = Camera::new(CameraOptions::new(16., 16.));
    let uniform = camera.uniform_camera();
    let volume = camera.frustum.compute_culling_volume(uniform.position, uniform.direction, uniform.up);
    FrameState::new(UniformFrame::new(uniform, JulianDate::new(2451545., 0., TimeStandard::Tai)), volume, [16, 16])
}

fn drawable() -> Primitive<NoGl> {
    let mut attributes = GeometryAttributes::default();
    attributes.set(
        "position",
        GeometryAttribute {
            component_datatype: ComponentDatatype::Float,
            components_per_attribute: 3,
            normalize: false,
            values: TypedArray::Float32(vec![0., 0., 0., 1., 0., 0., 0., 1., 0.]),
        },
    );
    let geometry = Geometry::new(attributes, Some(TypedArray::Uint16(vec![0, 1, 2])), PrimitiveType::Triangles);
    Primitive::new(PrimitiveOptions {
        geometry_instances: vec![GeometryInstance::new(geometry, None)],
        appearance: Some(Box::new(PerInstanceColorAppearance::default())),
        compress_vertices: Some(false),
        ..Default::default()
    })
}

#[test]
fn drawable_primitive_requires_a_renderer_when_rendering() {
    let mut p = drawable();
    let mut frame = frame();
    frame.scene_3d_only = true;
    frame.passes.render = true;
    let error = p.update(&mut frame).unwrap_err();
    assert!(error.to_string().contains("frameState.context is required"));
    assert_eq!(p.state(), PrimitiveState::Ready);
    assert!(!p.is_ready());
    assert!(frame.command_list.is_empty());
    assert!(p.update(&mut frame).is_err());
}

#[test]
fn inactive_pass_defers_work_and_destroyed_primitive_cannot_update() {
    let mut p = drawable();
    let mut frame = frame();
    p.update(&mut frame).unwrap();
    assert_eq!(p.state(), PrimitiveState::Ready);
    frame.passes.render = true;
    assert!(p.update(&mut frame).is_err());
    p.destroy().unwrap();
    assert!(p.is_destroyed());
    assert!(p.update(&mut frame).unwrap_err().to_string().contains("destroyed"));
    assert!(frame.command_list.is_empty());
}

#[test]
fn primitive_constructor_reference() {
    run(
        "scene",
        "Primitive",
        |_| EXACT,
        |f, a| match f {
            "options" => {
                let options = &a[0];
                let p = Primitive::<NoGl>::new(PrimitiveOptions {
                    show: options["show"].as_bool(),
                    asynchronous: options["asynchronous"].as_bool(),
                    allow_picking: options["allowPicking"].as_bool(),
                    cull: options["cull"].as_bool(),
                    ..Default::default()
                });
                Some(Out::Ok(json!({
                    "show": p.show,
                    "asynchronous": p.asynchronous,
                    "allowPicking": p.allow_picking,
                    "cull": p.cull,
                    "compressVertices": p.compress_vertices,
                    "releaseGeometryInstances": p.release_geometry_instances,
                    "ready": p.is_ready(),
                    "destroyed": p.is_destroyed(),
                })))
            }
            _ => None,
        },
    );
}

struct Child {
    id: &'static str,
    log: Rc<RefCell<Vec<String>>>,
}

impl ScenePrimitive for Child {
    fn pre_passes_update(&mut self, _: &mut FrameState) -> Result<(), RuntimeError> {
        self.log.borrow_mut().push("pre".into());
        Ok(())
    }
    fn update(&mut self, _: &mut FrameState) -> Result<(), RuntimeError> {
        self.log.borrow_mut().push(self.id.into());
        Ok(())
    }
    fn update_for_pass(&mut self, _: &mut FrameState, _: Cesium3DTilePass) -> Result<(), RuntimeError> {
        self.log.borrow_mut().push("pass".into());
        Ok(())
    }
    fn post_passes_update(&mut self, _: &mut FrameState) -> Result<(), RuntimeError> {
        self.log.borrow_mut().push("post".into());
        Ok(())
    }
    fn destroy(&mut self) -> Result<(), RuntimeError> {
        self.log.borrow_mut().push(self.id.into());
        Ok(())
    }
}

#[test]
fn primitive_collection_reference() {
    run(
        "scene",
        "PrimitiveCollection",
        |_| EXACT,
        |f, a| {
            let log = Rc::new(RefCell::new(Vec::new()));
            let child = |id| PrimitiveHandle::new(Child { id, log: Rc::clone(&log) });
            let mut frame = frame();
            match f {
                "order" => {
                    let mut c = PrimitiveCollection::new(PrimitiveCollectionOptions {
                        destroy_primitives: Some(false),
                        ..Default::default()
                    });
                    for id in ["a", "b", "c"] {
                        c.add(child(id), None).unwrap();
                    }
                    c.update(&mut frame).unwrap();
                    let mut result = vec![std::mem::take(&mut *log.borrow_mut())];
                    for op in a[0].as_array().unwrap() {
                        // Cesium's fixture passes `c.get(index)`.
                        let primitive = c.get(op[1].as_u64().unwrap() as usize).unwrap();
                        match s(&op[0]) {
                            "raise" => c.raise(&primitive).unwrap(),
                            "raiseToTop" => c.raise_to_top(&primitive).unwrap(),
                            "lower" => c.lower(&primitive).unwrap(),
                            "lowerToBottom" => c.lower_to_bottom(&primitive).unwrap(),
                            "remove" => {
                                c.remove(&primitive).unwrap();
                            }
                            other => panic!("unknown operation {other}"),
                        }
                        c.update(&mut frame).unwrap();
                        result.push(std::mem::take(&mut *log.borrow_mut()));
                    }
                    Some(Out::Ok(json!(result)))
                }
                "dispatch" => {
                    let mut c = PrimitiveCollection::new(PrimitiveCollectionOptions {
                        show: a[0].as_bool(),
                        ..Default::default()
                    });
                    let mut nested = PrimitiveCollection::default();
                    nested.add(child("update"), None).unwrap();
                    c.add(PrimitiveHandle::new(nested), None).unwrap();
                    c.pre_passes_update(&mut frame).unwrap();
                    c.update(&mut frame).unwrap();
                    c.update_for_pass(&mut frame, Cesium3DTilePass::Render).unwrap();
                    c.post_passes_update(&mut frame).unwrap();
                    Some(Out::Ok(json!(*log.borrow())))
                }
                "destroy" => {
                    let mut c = PrimitiveCollection::new(PrimitiveCollectionOptions {
                        destroy_primitives: a[0].as_bool(),
                        ..Default::default()
                    });
                    c.add(child("a"), None).unwrap();
                    c.add(child("b"), None).unwrap();
                    let first = c.get(0).unwrap();
                    c.remove(&first).unwrap();
                    c.destroy().unwrap();
                    Some(Out::Ok(json!({ "log": *log.borrow(), "destroyed": c.is_destroyed() })))
                }
                _ => None,
            }
        },
    );
}

#[test]
fn billboard_clamping_batch_table_and_labels_sdf_gpu_pipeline() {
    use cesium_rs::renderer::{Context, Gl, GlValue};
    use std::cell::Cell;

    #[derive(Clone, Default)]
    struct MockGl {
        next: Rc<Cell<u32>>,
        draws: Rc<Cell<usize>>,
        buffer_uploads: Rc<Cell<usize>>,
    }
    impl Gl for MockGl {
        type Object = u32;
        fn drawing_buffer_size(&self) -> Result<[u32; 2], RuntimeError> {
            Ok([256, 256])
        }
        fn invoke(&self, name: &str, args: &[GlValue<u32>]) -> Result<GlValue<u32>, RuntimeError> {
            if name.starts_with("create") || name == "fenceSync" {
                let id = self.next.get() + 1;
                self.next.set(id);
                return Ok(GlValue::Object(id));
            }
            if name == "bufferData" || name == "bufferSubData" {
                self.buffer_uploads.set(self.buffer_uploads.get() + 1);
            }
            if name == "drawElements"
                || name == "drawElementsInstanced"
                || name == "drawArrays"
                || name == "drawArraysInstanced"
            {
                self.draws.set(self.draws.get() + 1);
            }
            Ok(match name {
                "getParameter" => match args.first() {
                    Some(GlValue::Number(n))
                        if *n == webgl_constants::ALIASED_LINE_WIDTH_RANGE as f64
                            || *n == webgl_constants::ALIASED_POINT_SIZE_RANGE as f64
                            || *n == webgl_constants::MAX_VIEWPORT_DIMS as f64 =>
                    {
                        GlValue::Numbers(vec![1.0, 16384.0])
                    }
                    _ => GlValue::Number(16384.0),
                },
                "getExtension" => GlValue::Object(0),
                "checkFramebufferStatus" => GlValue::Number(webgl_constants::FRAMEBUFFER_COMPLETE as f64),
                "getShaderParameter" | "getProgramParameter" => GlValue::Bool(true),
                _ => GlValue::Null,
            })
        }
    }

    let gl = MockGl::default();
    let ctx = Context::new_webgl2(gl.clone());
    let time = JulianDate::new(2451545.0, 0.0, TimeStandard::Tai);
    let mut scene = Scene::new(
        RendererSceneBackend::new(ctx.clone()),
        CameraOptions::new(256.0, 256.0),
        SceneOptions::default(),
        time,
    )
    .unwrap();
    scene.rethrow_render_errors = true;

    let mut bb_coll = BillboardCollection::<MockGl>::new(BillboardCollectionOptions::default());
    bb_coll.set_height_sampler(Some(Rc::new(|_carto, _hr| Some(500.0))));
    let bt = Rc::new(RefCell::new(
        BatchTable::new(
            &ctx,
            vec![BatchTableAttribute {
                function_name: "czm_batchTable_color".into(),
                component_datatype: ComponentDatatype::UnsignedByte,
                components_per_attribute: 4,
                normalize: true,
            }],
            1,
        )
        .unwrap(),
    ));
    bb_coll.set_batch_table(Some(bt));
    let pos = Ellipsoid::WGS84.cartographic_to_cartesian(&Cartographic::from_degrees(0.0, 0.0, 100.0));
    let bb = bb_coll
        .add(
            BillboardOptions {
                position: pos,
                height_reference: HeightReference::ClampToGround,
                ..Default::default()
            },
            Some(BillboardImage::pixels(2, 2, vec![255u8; 16])),
        )
        .unwrap();
    scene.primitives.push(Box::new(bb_coll));

    let mut labels = LabelCollection::new_with_context(ctx.clone());
    labels.set_height_sampler(Some(Rc::new(|_, _| Some(250.0))));
    let label = labels
        .add(LabelOptions {
            position: pos,
            text: "Hi!".into(),
            show_background: true,
            height_reference: HeightReference::ClampToGround,
            ..Default::default()
        })
        .unwrap();
    assert!(labels.expanded_bounding_sphere().radius >= 0.0);
    scene.primitives.push(Box::new(labels));

    assert!(scene.render(time).unwrap());
    assert!(scene.force_render(time).unwrap());
    assert!(bb.clamped_position().is_some());
    let clamped_carto = Ellipsoid::WGS84.cartesian_to_cartographic(bb.clamped_position().unwrap()).unwrap();
    assert!((clamped_carto.height - 500.0).abs() < 1e-3);
    assert!(gl.draws.get() >= 2, "expected GPU draws for billboards and SDF labels");

    // Stabilize initial atlas/buffer uploads, then verify an unchanged frame performs fewer
    // buffer uploads than a frame where ordinary label properties (not text/font) are mutated.
    assert!(scene.force_render(time).unwrap());
    let uploads_before_clean = gl.buffer_uploads.get();
    assert!(scene.force_render(time).unwrap());
    let uploads_on_clean_frame = gl.buffer_uploads.get() - uploads_before_clean;

    {
        let mut l = label.borrow_mut();
        l.set_position(Ellipsoid::WGS84.cartographic_to_cartesian(&Cartographic::from_degrees(1.0, 1.0, 100.0)));
        l.set_pixel_offset(Cartesian2::new(10.0, 20.0));
        l.set_background_color(Color::RED);
        l.set_show(false);
        assert!(l.is_dirty());
        assert!(!l.rebind_all_glyphs());
        assert!(!l.reposition_all_glyphs());
    }
    let uploads_before_dirty = gl.buffer_uploads.get();
    assert!(scene.force_render(time).unwrap());
    let uploads_on_dirty_frame = gl.buffer_uploads.get() - uploads_before_dirty;
    assert!(!label.borrow().is_dirty());
    assert!(
        uploads_on_dirty_frame > uploads_on_clean_frame,
        "mutating ordinary label properties (show/position/pixel_offset/background_color) must re-sync GPU billboards (clean={uploads_on_clean_frame}, dirty={uploads_on_dirty_frame})"
    );

    scene.destroy().unwrap();
}
