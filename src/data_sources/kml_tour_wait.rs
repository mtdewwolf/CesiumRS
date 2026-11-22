//! CesiumJS `KmlTourWait`: timed pause entry in a [`super::KmlTour`].
use super::{KmlTourCamera, KmlTourCameraOptions, KmlTourPlaylistEntry};
use std::{
    any::Any,
    cell::{Cell, RefCell},
    rc::Rc,
};

/// Optional timer handle for driving [`KmlTourWait`] externally or in a host
/// event loop.
pub trait KmlTourTimer {
    fn schedule(&self, duration_ms: f64, callback: Box<dyn FnOnce()>) -> u64;
    fn cancel(&self, id: u64);
}

struct WaitState {
    callback: Option<Rc<dyn Fn(bool)>>,
    timeout_id: Option<u64>,
    cancelled: bool,
}

pub struct KmlTourWait {
    pub duration: f64,
    timer: Option<Rc<dyn KmlTourTimer>>,
    next_id: Cell<u64>,
    state: Rc<RefCell<WaitState>>,
}

impl KmlTourWait {
    pub const TYPE_NAME: &'static str = "KmlTourWait";

    pub fn new(duration: f64) -> Self {
        Self {
            duration,
            timer: None,
            next_id: Cell::new(1),
            state: Rc::new(RefCell::new(WaitState { callback: None, timeout_id: None, cancelled: false })),
        }
    }

    pub fn with_timer(duration: f64, timer: Rc<dyn KmlTourTimer>) -> Self {
        Self { timer: Some(timer), ..Self::new(duration) }
    }

    pub fn entry_type(&self) -> &'static str {
        Self::TYPE_NAME
    }

    pub fn blocking(&self) -> bool {
        true
    }

    pub fn has_active_callback(&self) -> bool {
        self.state.borrow().callback.is_some()
    }

    pub fn has_timeout(&self) -> bool {
        self.state.borrow().timeout_id.is_some()
    }

    pub fn play(&self, done: Rc<dyn Fn(bool)>) {
        let mut s = self.state.borrow_mut();
        s.callback = Some(done.clone());
        s.cancelled = false;
        let id = if let Some(timer) = &self.timer {
            let state = self.state.clone();
            timer.schedule(
                self.duration * 1000.0,
                Box::new(move || {
                    let cb = {
                        let mut st = state.borrow_mut();
                        if st.cancelled {
                            return;
                        }
                        st.callback.take()
                    };
                    if let Some(cb) = cb {
                        cb(false);
                    }
                }),
            )
        } else {
            let id = self.next_id.get();
            self.next_id.set(id + 1);
            id
        };
        s.timeout_id = Some(id);
    }

    /// Complete an active wait normally (simulates timer expiry).
    pub fn finish(&self) {
        let cb = {
            let mut s = self.state.borrow_mut();
            if s.cancelled {
                return;
            }
            s.callback.take()
        };
        if let Some(cb) = cb {
            cb(false);
        }
    }

    /// Cesium's `stop()`: cancels the timer and calls `activeCallback(true)`,
    /// matching `KmlTourWait.js` (which leaves `activeCallback` and `timeout`
    /// set on the entry).
    pub fn stop(&self) {
        let (timeout_id, cb) = {
            let mut s = self.state.borrow_mut();
            s.cancelled = true;
            (s.timeout_id, s.callback.clone())
        };
        if let (Some(timer), Some(id)) = (&self.timer, timeout_id) {
            timer.cancel(id);
        }
        if let Some(cb) = cb {
            cb(true);
        }
    }
}

impl KmlTourPlaylistEntry for KmlTourWait {
    fn as_any(&self) -> &dyn Any {
        self
    }
    fn blocking(&self) -> bool {
        true
    }
    fn play(
        &self,
        done: Rc<dyn Fn(bool)>,
        _camera: Option<Rc<dyn KmlTourCamera>>,
        _camera_options: Option<&KmlTourCameraOptions>,
    ) {
        KmlTourWait::play(self, done);
    }
    fn stop(&self) {
        KmlTourWait::stop(self);
    }
}
