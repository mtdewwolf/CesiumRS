//! Typed globe/effect boundaries: scene lifecycle wiring, height aggregation
//! and failure propagation through the native frame loop.
use cesium_rs::{core::*, renderer::*, scene::*};
use std::{
    cell::Cell,
    rc::Rc,
    sync::{
        atomic::{AtomicUsize, Ordering},
        Arc, Mutex,
    },
};

type Trace = Arc<Mutex<Vec<String>>>;
type Failure = Arc<Mutex<Option<String>>>;
type Destroyed = Rc<Cell<bool>>;

fn step(trace: &Trace, failure: &Failure, name: &str) -> Result<(), RuntimeError> {
    trace.lock().unwrap().push(name.into());
    if failure.lock().unwrap().as_deref() == Some(name) {
        return Err(RuntimeError::new("injected"));
    }
    Ok(())
}

#[derive(Clone)]
struct Host {
    trace: Trace,
    failure: Failure,
}
impl Host {
    fn step(&self, name: &str) -> Result<(), RuntimeError> {
        step(&self.trace, &self.failure, name)
    }
}
impl SceneBackend for Host {
    fn capabilities(&self) -> Result<ContextCapabilities, RuntimeError> {
        Ok(ContextCapabilities {
            webgl2: true,
            fragment_depth: true,
            depth_texture: true,
            color_buffer_float: true,
            ..Default::default()
        })
    }
    fn dimensions(&self) -> Result<[u32; 2], RuntimeError> {
        Ok([64, 64])
    }
    fn begin_frame(&mut self, _: &FrameState) -> Result<(), RuntimeError> {
        self.step("beginFrame")
    }
    fn clear(&mut self, _: Color, _: Option<f64>, _: Option<i32>) -> Result<(), RuntimeError> {
        self.step("clear")
    }
    fn set_frustum(&mut self, _: &FrameState, _: f64, _: f64) -> Result<(), RuntimeError> {
        Ok(())
    }
    fn resolve_framebuffers(&mut self, _: &FrameState) -> Result<(), RuntimeError> {
        self.step("resolveFramebuffers")
    }
    fn execute(&mut self, c: &SceneCommand, s: SceneSubmission) -> Result<(), RuntimeError> {
        (c.execute)(s)
    }
    fn end_frame(&mut self) -> Result<(), RuntimeError> {
        self.step("endFrame")
    }
    fn destroy(&mut self) -> Result<(), RuntimeError> {
        self.step("backendDestroy")
    }
}

struct MockPrimitive {
    trace: Trace,
    failure: Failure,
    height: Option<f64>,
    destroyed: Destroyed,
}
impl ScenePrimitive for MockPrimitive {
    fn pre_passes_update(&mut self, _: &mut FrameState) -> Result<(), RuntimeError> {
        step(&self.trace, &self.failure, "primitivePrePass")
    }
    fn update(&mut self, _: &mut FrameState) -> Result<(), RuntimeError> {
        step(&self.trace, &self.failure, "primitiveUpdate")
    }
    fn post_passes_update(&mut self, _: &mut FrameState) -> Result<(), RuntimeError> {
        step(&self.trace, &self.failure, "primitivePostPass")
    }
    fn update_for_pass(&mut self, _: &mut FrameState, _: Cesium3DTilePass) -> Result<(), RuntimeError> {
        Ok(())
    }
    fn height_at(&self, _: &Cartographic, _: &FrameState) -> Result<Option<f64>, RuntimeError> {
        Ok(self.height)
    }
    fn destroy(&mut self) -> Result<(), RuntimeError> {
        self.destroyed.set(true);
        Ok(())
    }
}

struct MockGlobe {
    trace: Trace,
    failure: Failure,
    height: Option<f64>,
    show: bool,
    destroyed: Destroyed,
}
impl SceneGlobe for MockGlobe {
    fn show(&self) -> bool {
        self.show
    }
    fn update(&mut self, _: &mut FrameState) -> Result<(), RuntimeError> {
        step(&self.trace, &self.failure, "globeUpdate")
    }
    fn render(&mut self, _: &mut FrameState) -> Result<(), RuntimeError> {
        step(&self.trace, &self.failure, "globeRender")
    }
    fn get_height(&self, _: &Cartographic) -> Option<f64> {
        self.height
    }
    fn destroy(&mut self) -> Result<(), RuntimeError> {
        self.destroyed.set(true);
        Ok(())
    }
}

struct MockEffect {
    trace: Trace,
    failure: Failure,
    destroyed: Destroyed,
}
impl SceneEffect for MockEffect {
    fn update(&mut self, _: &mut FrameState) -> Result<(), RuntimeError> {
        step(&self.trace, &self.failure, "effectUpdate")
    }
    fn render(&mut self, _: &mut FrameState) -> Result<(), RuntimeError> {
        step(&self.trace, &self.failure, "effectRender")
    }
    fn destroy(&mut self) -> Result<(), RuntimeError> {
        self.destroyed.set(true);
        Ok(())
    }
}

fn epoch() -> JulianDate {
    JulianDate::new(2451545., 0., TimeStandard::Tai)
}

struct Fixture {
    scene: Scene<Host>,
    trace: Trace,
    failure: Failure,
    primitive_destroyed: Destroyed,
    globe_destroyed: Destroyed,
    effect_destroyed: Destroyed,
}

fn fixture(globe_show: bool, primitive_height: Option<f64>, globe_height: Option<f64>) -> Fixture {
    let host = Host { trace: Default::default(), failure: Default::default() };
    let trace = Arc::clone(&host.trace);
    let failure = Arc::clone(&host.failure);
    let mut scene = Scene::new(host, CameraOptions::new(64., 64.), SceneOptions::default(), epoch()).unwrap();
    let primitive_destroyed = Rc::new(Cell::new(false));
    let globe_destroyed = Rc::new(Cell::new(false));
    let effect_destroyed = Rc::new(Cell::new(false));
    scene.primitives.push(Box::new(MockPrimitive {
        trace: Arc::clone(&trace),
        failure: Arc::clone(&failure),
        height: primitive_height,
        destroyed: Rc::clone(&primitive_destroyed),
    }));
    scene.globe = Some(Box::new(MockGlobe {
        trace: Arc::clone(&trace),
        failure: Arc::clone(&failure),
        height: globe_height,
        show: globe_show,
        destroyed: Rc::clone(&globe_destroyed),
    }));
    scene.effects.push(Box::new(MockEffect {
        trace: Arc::clone(&trace),
        failure: Arc::clone(&failure),
        destroyed: Rc::clone(&effect_destroyed),
    }));
    Fixture { scene, trace, failure, primitive_destroyed, globe_destroyed, effect_destroyed }
}

#[test]
fn globe_and_effect_lifecycle_orders_around_draw() {
    let mut f = fixture(true, None, None);
    f.scene.frame_state.frame_number = 1;
    f.scene.render(epoch()).unwrap();
    let trace = f.trace.lock().unwrap().clone();
    let at = |name: &str| trace.iter().position(|s| s == name).unwrap_or_else(|| panic!("missing {name}: {trace:?}"));
    assert!(at("primitivePrePass") < at("globeUpdate"), "{trace:?}");
    assert!(at("globeUpdate") < at("effectUpdate"), "{trace:?}");
    assert!(at("effectUpdate") < at("beginFrame"), "{trace:?}");
    assert!(at("primitiveUpdate") < at("globeRender"), "{trace:?}");
    assert!(at("globeRender") < at("clear"), "{trace:?}");
    assert!(at("clear") < at("resolveFramebuffers"), "{trace:?}");
    assert!(at("resolveFramebuffers") < at("effectRender"), "{trace:?}");
    assert!(at("effectRender") < at("endFrame"), "{trace:?}");
    assert!(at("endFrame") < at("primitivePostPass"), "{trace:?}");
}

#[test]
fn get_height_takes_the_greatest_source_and_honours_height_reference() {
    let cartographic = Cartographic::from_degrees(10., 20., 0.);
    let f = fixture(true, Some(123.), Some(45.));
    assert_eq!(f.scene.get_height(Some(&cartographic), None).unwrap(), Some(123.));
    assert_eq!(f.scene.get_height(Some(&cartographic), Some(HeightReference::ClampToGround)).unwrap(), Some(123.));
    assert_eq!(f.scene.get_height(Some(&cartographic), Some(HeightReference::ClampToTerrain)).unwrap(), Some(45.));
    assert_eq!(f.scene.get_height(Some(&cartographic), Some(HeightReference::RelativeTo3DTile)).unwrap(), Some(123.));
    assert_eq!(f.scene.get_height(None, None).unwrap(), None);
}

#[test]
fn get_height_ignores_hidden_globes_and_uncovered_positions() {
    let cartographic = Cartographic::from_degrees(10., 20., 0.);
    let hidden = fixture(false, None, Some(45.));
    assert_eq!(hidden.scene.get_height(Some(&cartographic), None).unwrap(), None);
    let uncovered = fixture(true, None, None);
    assert_eq!(uncovered.scene.get_height(Some(&cartographic), None).unwrap(), None);
    let globe_only = fixture(true, Some(-100.), Some(45.));
    assert_eq!(globe_only.scene.get_height(Some(&cartographic), None).unwrap(), Some(45.));
}

#[test]
fn globe_and_effect_failures_propagate_from_each_phase() {
    for failing in ["globeUpdate", "globeRender", "effectUpdate", "effectRender"] {
        let mut f = fixture(true, None, None);
        f.scene.rethrow_render_errors = true;
        *f.failure.lock().unwrap() = Some(failing.into());
        assert!(f.scene.render(epoch()).is_err(), "{failing} must fail the frame");
    }
    // Without rethrow the same failures are reported as RenderError events.
    let mut f = fixture(true, None, None);
    let errors = Arc::new(AtomicUsize::new(0));
    let counter = Arc::clone(&errors);
    f.scene.events.add_event_listener(move |event| {
        if event.kind == SceneEventKind::RenderError {
            counter.fetch_add(1, Ordering::Relaxed);
        }
    });
    *f.failure.lock().unwrap() = Some("effectRender".into());
    assert!(f.scene.render(epoch()).is_ok());
    assert_eq!(errors.load(Ordering::Relaxed), 1);
}

#[test]
fn destroy_releases_globe_effects_and_primitives_once() {
    let mut f = fixture(true, None, None);
    f.scene.destroy().unwrap();
    assert!(f.primitive_destroyed.get());
    assert!(f.globe_destroyed.get());
    assert!(f.effect_destroyed.get());
    assert!(f.trace.lock().unwrap().iter().any(|s| s == "backendDestroy"));
    assert!(f.scene.globe.is_none());
    assert!(f.scene.effects.is_empty());
    assert!(f.scene.is_destroyed());
    f.scene.destroy().unwrap();
}

#[test]
fn backend_failures_keep_the_globe_and_effects_boundary_alive() {
    let mut f = fixture(true, Some(7.), Some(3.));
    f.scene.rethrow_render_errors = true;
    *f.failure.lock().unwrap() = Some("clear".into());
    assert!(f.scene.render(epoch()).is_err());
    *f.failure.lock().unwrap() = None;
    let cartographic = Cartographic::from_degrees(0., 0., 0.);
    assert_eq!(f.scene.get_height(Some(&cartographic), None).unwrap(), Some(7.));
    f.scene.render(epoch()).unwrap();
}
