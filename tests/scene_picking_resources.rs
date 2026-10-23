use cesium_rs::{
    core::{webgl_constants as gl, BoundingRectangle, Cartesian2, JulianDate, RuntimeError, TimeStandard},
    renderer::*,
    scene::*,
};
use std::{
    cell::{Cell, RefCell},
    future::Future,
    rc::Rc,
    task::{Context as TaskContext, Poll, Waker},
};

#[derive(Default)]
struct State {
    next: Cell<u32>,
    calls: RefCell<Vec<String>>,
    signaled: Cell<bool>,
    read_error: Cell<bool>,
}
#[derive(Clone, Default)]
struct Host(Rc<State>);
impl Host {
    fn count(&self, name: &str) -> usize {
        self.0.calls.borrow().iter().filter(|n| *n == name).count()
    }
}
impl Gl for Host {
    type Object = u32;
    fn drawing_buffer_size(&self) -> Result<[u32; 2], RuntimeError> {
        Ok([8, 8])
    }
    fn invoke(&self, name: &str, args: &[GlValue<u32>]) -> Result<GlValue<u32>, RuntimeError> {
        self.0.calls.borrow_mut().push(name.into());
        if name.starts_with("create") || name == "fenceSync" {
            let id = self.0.next.get() + 1;
            self.0.next.set(id);
            return Ok(GlValue::Object(id));
        }
        Ok(match name {
            "getParameter" => match args.first() {
                Some(GlValue::Number(p))
                    if [gl::ALIASED_LINE_WIDTH_RANGE, gl::ALIASED_POINT_SIZE_RANGE].contains(&(*p as u32)) =>
                {
                    GlValue::Numbers(vec![1., 1.])
                }
                Some(GlValue::Number(p)) if *p == gl::MAX_VIEWPORT_DIMS as f64 => {
                    GlValue::Numbers(vec![16384., 16384.])
                }
                _ => GlValue::Number(16384.),
            },
            "getExtension" => GlValue::Object(0),
            "getSyncParameter" => {
                GlValue::Number(if self.0.signaled.get() { gl::SIGNALED } else { gl::UNSIGNALED } as f64)
            }
            "checkFramebufferStatus" => GlValue::Number(gl::FRAMEBUFFER_COMPLETE as f64),
            "readPixels" | "getBufferSubData" => {
                if self.0.read_error.get() {
                    return Err(RuntimeError::new("read error"));
                }
                let length = match args.last() {
                    Some(GlValue::Pixels { bytes, .. }) | Some(GlValue::Bytes(bytes)) => bytes.len(),
                    _ => return Ok(GlValue::Null),
                };
                GlValue::Bytes((0..length).map(|i| if i % 4 == 0 { 1 } else { 0 }).collect())
            }
            _ => GlValue::Null,
        })
    }
}
fn ready<F: Future>(future: F) -> F::Output {
    match std::pin::pin!(future).poll(&mut TaskContext::from_waker(Waker::noop())) {
        Poll::Ready(v) => v,
        Poll::Pending => panic!("immediate test scheduler suspended"),
    }
}
fn framebuffer(context: &Context<Host>) -> PickFramebuffer<Host> {
    let mut f = PickFramebuffer::new(context.clone()).unwrap();
    f.begin(&BoundingRectangle::new(0., 0., 1., 1.), Viewport { x: 0, y: 0, width: 8, height: 8 }).unwrap();
    f
}

#[test]
fn async_timeout_and_cancellation_release_transfer_resources() {
    let host = Host::default();
    let context = Context::new_webgl2(host.clone());
    let f = framebuffer(&context);
    let rectangle = BoundingRectangle::new(0., 0., 1., 1.);
    let polls = Cell::new(0);
    let request = f.end_async(&rectangle, 1).unwrap();
    let error = ready(request.wait(|| {
        polls.set(polls.get() + 1);
        std::future::ready(Ok(()))
    }))
    .err()
    .unwrap();
    assert_eq!(error.message, "Async Picking Request Timeout");
    assert_eq!(polls.get(), 11);
    assert_eq!(host.count("deleteSync"), 1);
    assert_eq!(host.count("deleteBuffer"), 1);
    let request = f.end_async(&rectangle, 1).unwrap();
    drop(request);
    assert_eq!(host.count("deleteSync"), 2);
    assert_eq!(host.count("deleteBuffer"), 2);
    let request = f.end_async(&rectangle, 1).unwrap();
    assert!(ready(request.wait(|| std::future::ready(Err(RuntimeError::new("cancelled host"))))).is_err());
    assert_eq!(host.count("deleteSync"), 3);
    assert_eq!(host.count("deleteBuffer"), 3);
    host.0.signaled.set(true);
    let request = f.end_async(&rectangle, 1).unwrap();
    host.0.read_error.set(true);
    let error = ready(request.wait(|| std::future::ready(Ok(())))).err().unwrap();
    assert_eq!(error.message, "Async Picking Request Timeout");
    assert_eq!(host.count("deleteSync"), 4);
    assert_eq!(host.count("deleteBuffer"), 4);
}

#[test]
fn async_readback_and_webgl1_snapshot_have_source_object_lifetimes() {
    for webgl2 in [false, true] {
        let host = Host::default();
        host.0.signaled.set(true);
        let context = if webgl2 { Context::new_webgl2(host.clone()) } else { Context::new_webgl1(host.clone()) };
        let f = framebuffer(&context);
        let a = Rc::new("a");
        let id = context.create_pick_id(a.clone()).unwrap();
        let request = f.end_async(&BoundingRectangle::new(0., 0., 1., 1.), 1).unwrap();
        let b = Rc::new("b");
        id.set_object(b.clone()).unwrap();
        let polls = Cell::new(0);
        let objects = ready(request.wait(|| {
            polls.set(polls.get() + 1);
            std::future::ready(Ok(()))
        }))
        .unwrap();
        let object = objects[0].clone().downcast::<&str>().unwrap();
        assert!(Rc::ptr_eq(&object, if webgl2 { &b } else { &a }));
        assert_eq!(polls.get(), usize::from(webgl2));
        assert_eq!(host.count("fenceSync"), usize::from(webgl2));
    }
}

#[test]
fn framebuffer_center_edge_padding_and_destroy() {
    let context = Context::new_webgl2(Host::default());
    let mut f = framebuffer(&context);
    let id = context.create_pick_id(Rc::new(42)).unwrap();
    assert_eq!(f.read_center_pixel(&BoundingRectangle::new(0., 0., 3., 3.)).unwrap(), [1, 0, 0, 0]);
    let hit = f.end(&BoundingRectangle::new(7., 7., 3., 3.), 1).unwrap();
    assert_eq!(*hit[0].clone().downcast::<i32>().unwrap(), 42);
    assert!(f.end(&BoundingRectangle::new(8., 8., 3., 3.), 1).unwrap().is_empty());
    f.destroy().unwrap();
    assert!(f.is_destroyed());
    f.destroy().unwrap();
    assert!(f.end(&BoundingRectangle::new(0., 0., 1., 1.), 1).is_err());
    assert!(f.begin(&BoundingRectangle::new(0., 0., 1., 1.), Viewport { x: 0, y: 0, width: 8, height: 8 }).is_err());
    assert!(id.object().is_some());
}

#[test]
fn scene_pick_failure_restores_the_callers_pass_state() {
    struct FailingPrimitive;
    impl ScenePrimitive for FailingPrimitive {
        fn pre_passes_update(&mut self, _: &mut FrameState) -> Result<(), RuntimeError> {
            Ok(())
        }
        fn post_passes_update(&mut self, _: &mut FrameState) -> Result<(), RuntimeError> {
            Ok(())
        }
        fn update_for_pass(&mut self, _: &mut FrameState, _: Cesium3DTilePass) -> Result<(), RuntimeError> {
            Ok(())
        }
        fn update(&mut self, _: &mut FrameState) -> Result<(), RuntimeError> {
            Err(RuntimeError::new("injected primitive failure"))
        }
        fn destroy(&mut self) -> Result<(), RuntimeError> {
            Ok(())
        }
    }
    let context = Context::new_webgl2(Host::default());
    let mut external =
        FramebufferManager::new(FramebufferManagerOptions { depth_stencil: true, ..Default::default() }).unwrap();
    external.update(&context, 8, 8, None, None, None).unwrap();
    let mut scene = Scene::new(
        RendererSceneBackend::new(context),
        CameraOptions::new(8., 8.),
        SceneOptions::default(),
        JulianDate::new(2451545., 0., TimeStandard::Tai),
    )
    .unwrap();
    scene.framebuffer = Some(FrameResource::new(external.framebuffer().unwrap().clone()));
    scene.viewport = Some(Viewport { x: 1, y: 2, width: 4, height: 5 });
    scene.scissor_test = Some(ScissorTest { enabled: true, rectangle: Viewport { x: 2, y: 3, width: 1, height: 2 } });
    scene.blending_enabled = Some(true);
    scene.frame_state.invert_classification = true;
    let before = (scene.framebuffer.clone(), scene.viewport, scene.scissor_test.clone(), scene.blending_enabled);
    scene.primitives.push(Box::new(FailingPrimitive));
    let error = scene.pick(Cartesian2::new(4., 4.), None, None).err().unwrap();
    assert_eq!(error.message, "injected primitive failure");
    assert_eq!(before, (scene.framebuffer.clone(), scene.viewport, scene.scissor_test.clone(), scene.blending_enabled));
    assert!(scene.frame_state.invert_classification);
    scene.primitives.clear();
    assert!(scene.pick(Cartesian2::new(4., 4.), None, None).unwrap().is_none());
    assert_eq!(before, (scene.framebuffer.clone(), scene.viewport, scene.scissor_test.clone(), scene.blending_enabled));
}
