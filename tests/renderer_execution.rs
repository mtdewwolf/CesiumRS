//! Target management, compute lifetime and scheduled fence contracts.
mod common;
use cesium_rs::{
    core::{webgl_constants as gl, PixelFormat, RuntimeError},
    renderer::*,
};
use common::*;
use serde_json::{json, Value};
use std::{
    cell::{Cell, RefCell},
    collections::VecDeque,
    future::Future,
    rc::Rc,
    task::{Context as TaskContext, Poll, Waker},
};

#[derive(Default)]
struct State {
    next: Cell<u32>,
    calls: RefCell<Vec<String>>,
    fail: RefCell<Option<String>>,
    signals: RefCell<VecDeque<u32>>,
}
#[derive(Clone, Default)]
struct Host(Rc<State>);
impl Gl for Host {
    type Object = u32;
    fn drawing_buffer_size(&self) -> Result<[u32; 2], RuntimeError> {
        Ok([8, 8])
    }
    fn invoke(&self, name: &str, _: &[GlValue<u32>]) -> Result<GlValue<u32>, RuntimeError> {
        self.0.calls.borrow_mut().push(name.into());
        if self.0.fail.borrow().as_deref() == Some(name) {
            return Err(RuntimeError::new("injected failure"));
        }
        if name.starts_with("create") || name == "fenceSync" {
            let id = self.0.next.get() + 1;
            self.0.next.set(id);
            return Ok(GlValue::Object(id));
        }
        Ok(match name {
            "getParameter" => GlValue::Number(16384.),
            "getExtension" => GlValue::Object(0),
            "getShaderParameter" | "getProgramParameter" => GlValue::Bool(true),
            "getSyncParameter" => {
                GlValue::Number(self.0.signals.borrow_mut().pop_front().unwrap_or(gl::UNSIGNALED) as f64)
            }
            _ => GlValue::Null,
        })
    }
}
impl Host {
    fn count(&self, name: &str) -> usize {
        self.0.calls.borrow().iter().filter(|n| *n == name).count()
    }
    fn fail(&self, name: Option<&str>) {
        *self.0.fail.borrow_mut() = name.map(str::to_owned);
    }
}
fn ready<F: Future>(future: F) -> F::Output {
    let mut cx = TaskContext::from_waker(Waker::noop());
    match std::pin::pin!(future).poll(&mut cx) {
        Poll::Ready(v) => v,
        Poll::Pending => panic!("test scheduler unexpectedly suspended"),
    }
}
fn options(v: &Value) -> FramebufferManagerOptions {
    let mut o = FramebufferManagerOptions::default();
    macro_rules! flag {
        ($name:literal,$field:ident) => {
            if let Some(v) = v[$name].as_bool() {
                o.$field = v;
            }
        };
    }
    flag!("color", color);
    flag!("depth", depth);
    flag!("depthStencil", depth_stencil);
    flag!("supportsDepthTexture", supports_depth_texture);
    flag!("createColorAttachments", create_color_attachments);
    flag!("createDepthAttachments", create_depth_attachments);
    if let Some(v) = v["numSamples"].as_u64() {
        o.num_samples = v as u32;
    }
    if let Some(v) = v["colorAttachmentsLength"].as_u64() {
        o.color_attachments_length = v as usize;
    }
    o.pixel_format = v["pixelFormat"].as_u64().and_then(|v| PixelFormat::from_value(v as u32));
    o.pixel_datatype = v["pixelDatatype"].as_u64().and_then(|v| PixelDatatype::from_value(v as u32));
    o
}
#[test]
fn renderer_execution_reference() {
    run(
        "renderer",
        "RendererExecution",
        |_| EXACT,
        |f, a| match f {
            "viewportShader" => ok(VIEWPORT_QUAD_VERTEX_SHADER.to_owned()),
            "commandDefaults" => {
                let source = a[0]["fragmentShaderSource"].as_str();
                let command = ComputeCommand::<Host> {
                    persists: a[0]["persists"].as_bool().unwrap_or(false),
                    owner: a[0]["owner"].as_str().map(str::to_owned),
                    fragment_shader_source: source
                        .map(|s| ShaderSource { sources: vec![s.into()], ..Default::default() }),
                    ..Default::default()
                };
                ok(json!([
                    command.pass() as u32,
                    command.persists,
                    command.owner,
                    source,
                    null,
                    null,
                    null,
                    null,
                    null,
                    null,
                    null
                ]))
            }
            "managerDefaults" => {
                let o = options(&a[0]);
                let m = FramebufferManager::<Host>::new(o).unwrap();
                ok(json!([
                    m.num_samples(),
                    m.is_dirty(4, 2, None, None, None),
                    o.color,
                    o.depth,
                    o.depth_stencil,
                    o.supports_depth_texture,
                    o.create_color_attachments,
                    o.create_depth_attachments,
                    o.color_attachments_length,
                    o.pixel_datatype.map(|d| d as u32),
                    o.pixel_format.map(|f| f as u32)
                ]))
            }
            "syncPolling" => {
                let host = Host::default();
                *host.0.signals.borrow_mut() = a[0]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|v| if v.as_bool().unwrap() { gl::SIGNALED } else { gl::UNSIGNALED })
                    .collect();
                let ctx = Context::new_webgl2(host.clone());
                let sync = Sync::new(&ctx).unwrap();
                let scheduled = Cell::new(0);
                let result = ready(sync.wait_for_signal(
                    || {
                        scheduled.set(scheduled.get() + 1);
                        std::future::ready(Ok(()))
                    },
                    a[1].as_u64().map(|v| v as u32),
                ));
                ok(json!([
                    result.is_ok(),
                    host.count("getSyncParameter"),
                    scheduled.get(),
                    result.err().map(|e| e.message)
                ]))
            }
            _ => None,
        },
    );
}
#[test]
fn manager_resize_reuses_or_replaces_owned_resources_atomically() {
    let host = Host::default();
    let ctx = Context::new_webgl2(host.clone());
    let mut manager = FramebufferManager::new(FramebufferManagerOptions::default()).unwrap();
    assert!(manager.update(&ctx, 4, 2, None, None, None).unwrap());
    let old = manager.color_texture(0).unwrap().unwrap().clone();
    let framebuffer = manager.framebuffer().unwrap().clone();
    let created = host.count("createTexture");
    assert!(!manager.update(&ctx, 4, 2, None, None, None).unwrap());
    assert_eq!(host.count("createTexture"), created);
    host.fail(Some("framebufferTexture2D"));
    assert!(manager.update(&ctx, 8, 4, None, None, None).is_err());
    assert!(Rc::ptr_eq(&old, manager.color_texture(0).unwrap().unwrap()));
    assert!(!old.is_destroyed());
    assert!(!framebuffer.is_destroyed());
    host.fail(None);
    assert!(manager.update(&ctx, 8, 4, Some(4), None, None).unwrap());
    assert!(old.is_destroyed());
    assert!(framebuffer.is_destroyed());
    assert!(manager.color_renderbuffer(0).unwrap().is_some());
    assert_eq!(manager.num_samples(), 4);
    manager.prepare_textures(false).unwrap();
    assert_eq!(host.count("blitFramebuffer"), 1);
    let current = manager.color_texture(0).unwrap().unwrap().clone();
    manager.destroy_framebuffer().unwrap();
    assert!(!current.is_destroyed());
    assert!(manager.is_dirty(8, 4, Some(4), None, None));
    manager.destroy().unwrap();
    assert!(current.is_destroyed());
    assert!(manager.framebuffer().is_none());
    assert!(manager.update(&ctx, 2, 2, None, None, None).unwrap());
}
#[test]
fn manager_borrows_external_attachments_and_rejects_foreign_resources() {
    let host = Host::default();
    let ctx = Context::new_webgl2(host.clone());
    let mut manager = FramebufferManager::new(FramebufferManagerOptions {
        depth_stencil: true,
        create_color_attachments: false,
        create_depth_attachments: false,
        ..Default::default()
    })
    .unwrap();
    let texture = Rc::new(Texture::new(&ctx, TextureOptions::rgba(4, 4), None).unwrap());
    let mut d = TextureOptions::rgba(4, 4);
    d.pixel_format = PixelFormat::DepthStencil;
    d.pixel_datatype = PixelDatatype::UnsignedInt24_8;
    let depth = Rc::new(Texture::new(&ctx, d, None).unwrap());
    manager.set_color_texture(0, Some(texture.clone())).unwrap();
    manager.set_depth_stencil_texture(Some(depth.clone())).unwrap();
    manager.update(&ctx, 4, 4, None, None, None).unwrap();
    assert!(!manager.is_dirty(4, 4, None, None, None));
    manager.set_color_texture(0, Some(texture.clone())).unwrap();
    assert!(!manager.is_dirty(4, 4, None, None, None));
    let original = manager.framebuffer().unwrap().clone();
    let foreign = Context::new_webgl2(host.clone());
    let bad = Rc::new(Texture::new(&foreign, TextureOptions::rgba(4, 4), None).unwrap());
    manager.set_color_texture(0, Some(bad)).unwrap();
    assert!(manager.update(&ctx, 4, 4, None, None, None).is_err());
    assert!(!original.is_destroyed());
    assert!(!texture.is_destroyed());
    manager.set_color_texture(0, Some(texture.clone())).unwrap();
    manager.destroy().unwrap();
    assert!(!texture.is_destroyed());
    assert!(!depth.is_destroyed());
    assert!(manager.color_texture(0).unwrap().is_some());
    assert!(manager.set_color_texture(1, None).is_err());
    assert!(FramebufferManager::<Host>::new(FramebufferManagerOptions { color: false, ..Default::default() }).is_err());
    assert!(FramebufferManager::<Host>::new(FramebufferManagerOptions {
        depth: true,
        depth_stencil: true,
        ..Default::default()
    })
    .is_err());
}
#[test]
fn compute_prepares_before_validation_and_honors_persistence() {
    for persists in [false, true] {
        let host = Host::default();
        let ctx = Context::new_webgl2(host.clone());
        let quad = ViewportQuad::new(&ctx).unwrap();
        let va = quad.vertex_array().clone();
        let program = Rc::new(ShaderProgram::new(&ctx, "vs", "fs", &[]).unwrap());
        let output = Rc::new(Texture::new(&ctx, TextureOptions::rgba(3, 2), None).unwrap());
        let events = Rc::new(RefCell::new(vec![]));
        let before = events.clone();
        let source = program.clone();
        let v = va.clone();
        let o = output.clone();
        let mut command = ComputeCommand {
            persists,
            pre_execute: Some(Rc::new(move |c| {
                before.borrow_mut().push("pre");
                c.shader_program = Some(source.clone());
                c.vertex_array = Some(v.clone());
                c.output_texture = Some(o.clone());
                Ok(())
            })),
            ..Default::default()
        };
        let after = events.clone();
        let p = program.clone();
        command.post_execute = Some(Box::new(move |_| {
            after.borrow_mut().push("post");
            assert_eq!(p.is_destroyed(), !persists);
            Ok(())
        }));
        let during = events.clone();
        command.uniform_map = Some(Rc::new(move || {
            during.borrow_mut().push("uniform");
            Ok(vec![])
        }));
        let mut engine = ComputeEngine::new(&ctx);
        command.execute(&mut engine).unwrap();
        assert_eq!(&*events.borrow(), &["pre", "uniform", "post"]);
        assert_eq!(va.is_destroyed(), !persists);
        assert!(!output.is_destroyed());
        assert_eq!(host.count("deleteFramebuffer"), 1);
        engine.destroy().unwrap();
        assert_eq!(program.is_destroyed(), !persists);
        assert!(command.execute(&mut engine).is_err());
    }
}
#[test]
fn compute_reuses_quad_and_cleans_failure_targets() {
    let host = Host::default();
    let ctx = Context::new_webgl2(host.clone());
    let output = Rc::new(Texture::new(&ctx, TextureOptions::rgba(2, 2), None).unwrap());
    let mut engine = ComputeEngine::new(&ctx);
    let mut command = ComputeCommand {
        output_texture: Some(output.clone()),
        fragment_shader_source: Some(ShaderSource {
            sources: vec!["void main(){out_FragColor=vec4(1.);}".into()],
            ..Default::default()
        }),
        ..Default::default()
    };
    command.execute(&mut engine).unwrap();
    command.execute(&mut engine).unwrap();
    assert_eq!(host.count("createVertexArray"), 1);
    assert_eq!(host.count("createProgram"), 2);
    assert_eq!(host.count("deleteProgram"), 2);
    host.fail(Some("drawElements"));
    assert!(command.execute(&mut engine).is_err());
    assert_eq!(host.count("deleteFramebuffer"), 3);
    assert!(!output.is_destroyed());
    host.fail(None);
    engine.destroy().unwrap();
    assert_eq!(host.count("deleteVertexArray"), 1);
}
#[test]
fn compute_pre_callback_can_clear_itself() {
    let ctx = Context::new_webgl2(Host::default());
    let mut engine = ComputeEngine::new(&ctx);
    let calls = Rc::new(Cell::new(0));
    let counter = calls.clone();
    let mut command = ComputeCommand {
        pre_execute: Some(Rc::new(move |command| {
            counter.set(counter.get() + 1);
            command.pre_execute = None;
            Ok(())
        })),
        ..Default::default()
    };
    // The command deliberately fails validation after preExecute, as Cesium
    // does, but the callback's mutation must survive that failure.
    assert!(command.execute(&mut engine).is_err());
    assert!(command.pre_execute.is_none());
    assert!(command.execute(&mut engine).is_err());
    assert_eq!(calls.get(), 1);
}

#[test]
fn sync_allocation_scheduler_and_delete_failures_are_reported() {
    let host = Host::default();
    let ctx = Context::new_webgl2(host.clone());
    host.fail(Some("fenceSync"));
    assert!(Sync::new(&ctx).is_err());
    host.fail(None);
    let sync = Sync::new(&ctx).unwrap();
    let result = ready(sync.wait_for_signal(|| std::future::ready(Err(RuntimeError::new("scheduler failed"))), None));
    assert_eq!(result.unwrap_err().message, "scheduler failed");
    assert_eq!(host.count("getSyncParameter"), 0);
    host.fail(Some("deleteSync"));
    assert!(sync.destroy().is_err());
    assert!(!sync.is_destroyed());
    host.fail(None);
    sync.destroy().unwrap();
    assert!(sync.is_destroyed());
    assert!(sync.status().is_err());
}
