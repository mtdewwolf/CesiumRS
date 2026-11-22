//! CesiumJS `KmlTourFlyTo`: camera flight entry in a [`super::KmlTour`].
use super::{KmlCamera, KmlLookAt, KmlTourPlaylistEntry};
use crate::core::{BoundingSphere, Cartesian3, EasingFunction, HeadingPitchRange, HeadingPitchRoll};
use std::{any::Any, cell::RefCell, rc::Rc};

pub type KmlTourDoneCallback = Rc<dyn Fn(Option<bool>)>;

/// Target view for a [`KmlTourFlyTo`] entry.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum KmlTourView {
    Camera(KmlCamera),
    LookAt(KmlLookAt),
}

impl From<KmlCamera> for KmlTourView {
    fn from(v: KmlCamera) -> Self {
        Self::Camera(v)
    }
}

impl From<KmlLookAt> for KmlTourView {
    fn from(v: KmlLookAt) -> Self {
        Self::LookAt(v)
    }
}

/// Camera flight options produced by [`KmlTourFlyTo::get_camera_options`].
#[derive(Clone, Default)]
pub struct KmlTourCameraOptions {
    pub duration: Option<f64>,
    pub complete: Option<KmlTourDoneCallback>,
    pub easing_function: Option<EasingFunction>,
    pub destination: Option<Cartesian3>,
    pub orientation: Option<HeadingPitchRoll>,
    pub offset: Option<HeadingPitchRange>,
}

/// Camera interface driven by [`KmlTourFlyTo`].
pub trait KmlTourCamera {
    fn fly_to(&self, options: KmlTourCameraOptions);
    fn fly_to_bounding_sphere(&self, target: BoundingSphere, options: KmlTourCameraOptions);
    fn cancel_flight(&self);
}

struct ActiveState {
    camera: Option<Rc<dyn KmlTourCamera>>,
    callback: Option<KmlTourDoneCallback>,
}

pub struct KmlTourFlyTo {
    pub duration: f64,
    pub fly_to_mode: String,
    pub view: KmlTourView,
    active: Rc<RefCell<ActiveState>>,
}

impl KmlTourFlyTo {
    pub const TYPE_NAME: &'static str = "KmlTourFlyTo";

    pub fn new(duration: f64, fly_to_mode: impl Into<String>, view: impl Into<KmlTourView>) -> Self {
        Self {
            duration,
            fly_to_mode: fly_to_mode.into(),
            view: view.into(),
            active: Rc::new(RefCell::new(ActiveState { camera: None, callback: None })),
        }
    }

    pub fn entry_type(&self) -> &'static str {
        Self::TYPE_NAME
    }

    pub fn blocking(&self) -> bool {
        true
    }

    pub fn has_active_camera(&self) -> bool {
        self.active.borrow().camera.is_some()
    }

    pub fn has_active_callback(&self) -> bool {
        self.active.borrow().callback.is_some()
    }

    /// Cesium's `getCameraOptions(cameraOptions)`: existing generated options
    /// win over `camera_options`, matching `combine(options, cameraOptions)`.
    pub fn get_camera_options(&self, camera_options: Option<&KmlTourCameraOptions>) -> KmlTourCameraOptions {
        let mut options = KmlTourCameraOptions {
            duration: Some(self.duration),
            complete: self.active.borrow().callback.clone(),
            easing_function: (self.fly_to_mode == "smooth").then_some(EasingFunction::LinearNone),
            destination: None,
            orientation: None,
            offset: None,
        };
        match self.view {
            KmlTourView::Camera(c) => {
                options.destination = Some(c.position);
                options.orientation = Some(c.heading_pitch_roll);
            }
            KmlTourView::LookAt(l) => {
                options.offset = Some(l.heading_pitch_range);
            }
        }
        if let Some(extra) = camera_options {
            if options.duration.is_none() {
                options.duration = extra.duration;
            }
            if options.complete.is_none() {
                options.complete = extra.complete.clone();
            }
            if options.easing_function.is_none() {
                options.easing_function = extra.easing_function;
            }
            if options.destination.is_none() {
                options.destination = extra.destination;
            }
            if options.orientation.is_none() {
                options.orientation = extra.orientation;
            }
            if options.offset.is_none() {
                options.offset = extra.offset;
            }
        }
        options
    }

    pub fn play(
        &self,
        done: Option<Rc<dyn Fn(bool)>>,
        camera: Rc<dyn KmlTourCamera>,
        camera_options: Option<&KmlTourCameraOptions>,
    ) {
        {
            let mut active = self.active.borrow_mut();
            active.camera = Some(camera.clone());
            active.callback = done.map(|done| {
                let state = self.active.clone();
                Rc::new(move |terminated: Option<bool>| {
                    {
                        let mut s = state.borrow_mut();
                        s.callback = None;
                        s.camera = None;
                    }
                    // Upstream CesiumJS quirk (KmlTourFlyTo.js:44):
                    // `done(defined(terminated) ? false : terminated)` passes
                    // `false` whenever a boolean argument is supplied.
                    let arg = match terminated {
                        Some(_) => false,
                        None => false,
                    };
                    done(arg);
                }) as KmlTourDoneCallback
            });
        }
        let options = self.get_camera_options(camera_options);
        match self.view {
            KmlTourView::Camera(_) => camera.fly_to(options),
            KmlTourView::LookAt(l) => {
                let target = BoundingSphere::new(l.position, 0.0);
                camera.fly_to_bounding_sphere(target, options);
            }
        }
    }

    pub fn stop(&self) {
        let camera = self.active.borrow().camera.clone();
        if let Some(camera) = camera {
            camera.cancel_flight();
        }
        let callback = self.active.borrow().callback.clone();
        if let Some(cb) = callback {
            cb(Some(true));
        }
    }
}

impl KmlTourPlaylistEntry for KmlTourFlyTo {
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn blocking(&self) -> bool {
        true
    }
    fn play(
        &self,
        done: Rc<dyn Fn(bool)>,
        camera: Option<Rc<dyn KmlTourCamera>>,
        camera_options: Option<&KmlTourCameraOptions>,
    ) {
        if let Some(camera) = camera {
            KmlTourFlyTo::play(self, Some(done), camera, camera_options);
        }
    }
    fn stop(&self) {
        KmlTourFlyTo::stop(self);
    }
}
