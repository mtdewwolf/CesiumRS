//! Primitive constructor and PrimitiveCollection reference cases. Rendering is
//! checked against a SwiftShader capture in `tests/scene_primitive.rs`.
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

/// No update runs in the constructor cases, so the host is never called.
struct NoGl;
impl cesium_rs::renderer::Gl for NoGl {
    type Object = ();
    fn drawing_buffer_size(&self) -> Result<[u32; 2], RuntimeError> {
        unreachable!()
    }
    fn invoke(
        &self,
        _: &str,
        _: &[cesium_rs::renderer::GlValue<()>],
    ) -> Result<cesium_rs::renderer::GlValue<()>, RuntimeError> {
        unreachable!()
    }
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
                    "asynchronous": p.asynchronous(),
                    "allowPicking": p.allow_picking(),
                    "cull": p.cull,
                    "compressVertices": p.compress_vertices(),
                    "releaseGeometryInstances": p.release_geometry_instances(),
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
