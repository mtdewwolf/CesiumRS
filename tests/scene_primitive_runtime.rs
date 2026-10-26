//! Constructor/collection reference cases and explicit unsupported-rendering errors.
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

fn drawable() -> Primitive {
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
fn drawable_primitive_reports_missing_renderer_without_noop_commands() {
    let mut p = drawable();
    let mut frame = frame();
    frame.scene_3d_only = true;
    let error = p.update(&mut frame).unwrap_err();
    assert!(error.to_string().contains("executable GPU command creation is required"));
    assert_eq!(p.state(), PrimitiveState::Failed);
    assert!(!p.is_ready());
    assert!(frame.command_list.is_empty());
    assert!(p.update(&mut frame).is_err());
}

#[test]
fn hidden_primitive_defers_work_and_destroyed_primitive_cannot_update() {
    let mut p = drawable();
    let mut frame = frame();
    p.show = false;
    p.update(&mut frame).unwrap();
    assert_eq!(p.state(), PrimitiveState::Ready);
    p.show = true;
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
                let p = Primitive::new(PrimitiveOptions {
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
            let child = |id| Box::new(Child { id, log: Rc::clone(&log) }) as Box<dyn ScenePrimitive>;
            let mut frame = frame();
            match f {
                "order" => {
                    let mut c = PrimitiveCollection::new(PrimitiveCollectionOptions {
                        destroy_primitives: Some(false),
                        ..Default::default()
                    });
                    for id in ["a", "b", "c"] {
                        c.add(child(id));
                    }
                    c.update(&mut frame).unwrap();
                    let mut result = vec![std::mem::take(&mut *log.borrow_mut())];
                    for op in a[0].as_array().unwrap() {
                        let index = op[1].as_u64().unwrap() as usize;
                        match s(&op[0]) {
                            "raise" => c.raise(index),
                            "raiseToTop" => c.raise_to_top(index),
                            "lower" => c.lower(index),
                            "lowerToBottom" => c.lower_to_bottom(index),
                            "remove" => {
                                c.remove(index);
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
                    nested.add(child("update"));
                    c.add(Box::new(nested));
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
                    c.add(child("a"));
                    c.add(child("b"));
                    c.remove(0);
                    c.destroy().unwrap();
                    Some(Out::Ok(json!({ "log": *log.borrow(), "destroyed": c.is_destroyed() })))
                }
                _ => None,
            }
        },
    );
}
