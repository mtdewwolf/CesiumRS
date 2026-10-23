//! CesiumJS `CameraFlightPath`: owned, deterministic camera flight tweens.
//! Time is in seconds; the caller drives `update` and lifecycle callbacks.
use super::{Camera, CameraDestination, CameraError, CameraFrustum, CameraOrientation, CameraView, SceneMode};
use crate::core::js_math::JsMath;
use crate::core::math::*;
use crate::core::{Cartesian3, Cartographic, EasingFunction, HeadingPitchRoll};
use std::{cell::RefCell, rc::Rc};

/// An easing curve, including application-provided curves.
#[derive(Clone)]
pub enum CameraEasing {
    Builtin(EasingFunction),
    Custom(Rc<dyn Fn(f64) -> f64>),
}
impl CameraEasing {
    pub fn evaluate(&self, time: f64) -> f64 {
        match self {
            Self::Builtin(f) => f.evaluate(time),
            Self::Custom(f) => f(time),
        }
    }
}
impl From<EasingFunction> for CameraEasing {
    fn from(f: EasingFunction) -> Self {
        Self::Builtin(f)
    }
}
/// Callbacks receive the camera so they may inspect it or start another flight.
pub type CameraFlightCallback = Box<dyn FnMut(&mut Camera)>;

pub struct CameraFlightPathOptions {
    pub destination: Cartesian3,
    pub orientation: HeadingPitchRoll,
    pub duration: Option<f64>,
    pub convert: bool,
    pub end_transform: Option<crate::core::Matrix4>,
    pub maximum_height: Option<f64>,
    pub pitch_adjust_height: Option<f64>,
    pub fly_over_longitude: Option<f64>,
    pub fly_over_longitude_weight: Option<f64>,
    pub easing_function: Option<CameraEasing>,
    pub complete: Option<CameraFlightCallback>,
    pub cancel: Option<CameraFlightCallback>,
}
impl CameraFlightPathOptions {
    pub fn new(destination: Cartesian3) -> Self {
        Self {
            destination,
            orientation: HeadingPitchRoll::new(0., -PI_OVER_TWO, 0.),
            duration: None,
            convert: true,
            end_transform: None,
            maximum_height: None,
            pitch_adjust_height: None,
            fly_over_longitude: None,
            fly_over_longitude_weight: None,
            easing_function: None,
            complete: None,
            cancel: None,
        }
    }
}

#[derive(Clone, Copy)]
enum Height {
    Linear(f64, f64),
    Arc { start: f64, end: f64, altitude: f64 },
}
impl Height {
    fn new(camera: &Camera, destination: Cartesian3, start: f64, end: f64, altitude: Option<f64>) -> Self {
        let altitude = altitude.unwrap_or_else(|| {
            let diff = camera.position - destination;
            let vertical = camera.up.multiply_by_scalar(diff.dot(camera.up)).magnitude();
            let horizontal = camera.right.multiply_by_scalar(diff.dot(camera.right)).magnitude();
            let altitude = match camera.frustum {
                CameraFrustum::Perspective(f) => {
                    let top = f.near * (f.fovy() * 0.5).js_tan();
                    let right = f.aspect_ratio * top;
                    ((vertical * f.near) / right).max((horizontal * f.near) / top)
                }
                _ => vertical.max(horizontal),
            };
            (altitude * 0.2).min(1_000_000_000.)
        });
        if start.max(end) < altitude {
            Self::Arc {
                start: -((altitude - start) * 1_000_000.).powf(1. / 8.),
                end: ((altitude - end) * 1_000_000.).powf(1. / 8.),
                altitude,
            }
        } else {
            Self::Linear(start, end)
        }
    }
    fn value(self, time: f64) -> f64 {
        match self {
            Self::Linear(a, b) => lerp(a, b, time),
            Self::Arc { start, end, altitude } => -(time * (end - start) + start).powf(8.) / 1_000_000. + altitude,
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct PathSettings {
    mode: SceneMode,
    destination: Cartesian3,
    orientation: HeadingPitchRoll,
    maximum_height: Option<f64>,
    pitch_adjust_height: Option<f64>,
    fly_over_longitude: Option<f64>,
    fly_over_longitude_weight: Option<f64>,
}
struct FlightPath {
    mode: SceneMode,
    start: Cartesian3,
    destination: Cartesian3,
    start_hpr: HeadingPitchRoll,
    end_hpr: HeadingPitchRoll,
    height: Height,
    pitch_adjust: Option<(f64, f64, f64)>,
    ellipsoid: crate::core::Ellipsoid,
}
fn adjust_angle(mut start: f64, end: f64) -> f64 {
    if equals_epsilon(start, TWO_PI, EPSILON11, EPSILON11) {
        start = 0.;
    }
    if end > start + PI {
        start += TWO_PI;
    } else if end < start - PI {
        start -= TWO_PI;
    }
    start
}
impl FlightPath {
    fn new(camera: &mut Camera, options: PathSettings) -> Result<Self, CameraError> {
        let mut start = camera.position;
        let mut destination = options.destination;
        let start_cartographic = if options.mode == SceneMode::Scene3D { camera.position_cartographic() } else { None };
        // Read in the same order as Cesium (these getters change reference frames).
        let pitch = if options.mode == SceneMode::Scene2D { 0. } else { camera.pitch().unwrap_or(f64::NAN) };
        let heading = adjust_angle(camera.heading().unwrap_or(f64::NAN), options.orientation.heading);
        let roll = if options.mode == SceneMode::Scene2D {
            0.
        } else {
            adjust_angle(camera.roll().unwrap_or(f64::NAN), options.orientation.roll)
        };
        let ellipsoid = *camera.map_projection().ellipsoid();
        if options.mode == SceneMode::Scene3D {
            let s = start_cartographic.ok_or(CameraError("camera must have a cartographic position"))?;
            let d = ellipsoid
                .cartesian_to_cartographic(destination)
                .ok_or(CameraError("destination must have a cartographic position"))?;
            start = Cartesian3::new(zero_to_two_pi(s.longitude), s.latitude, s.height);
            destination = Cartesian3::new(zero_to_two_pi(d.longitude), d.latitude, d.height);
            let mut long = false;
            if let Some(lon) = options.fly_over_longitude {
                let lon = zero_to_two_pi(lon);
                let inside = lon >= start.x.min(destination.x) && lon <= start.x.max(destination.x);
                long = if let Some(weight) = options.fly_over_longitude_weight {
                    let distance_in = (start.x - destination.x).abs();
                    let distance_out = TWO_PI - distance_in;
                    let (hit, miss) = if inside { (distance_in, distance_out) } else { (distance_out, distance_in) };
                    hit < miss * weight && !inside
                } else {
                    !inside
                };
            }
            if long {
                if start.x < destination.x {
                    start.x += TWO_PI;
                } else {
                    destination.x += TWO_PI;
                }
            } else {
                let diff = start.x - destination.x;
                if diff < -PI {
                    start.x += TWO_PI;
                } else if diff > PI {
                    destination.x += TWO_PI;
                }
            }
        } else if options.mode == SceneMode::Scene2D {
            let f = camera.frustum.orthographic().expect("2D frustum");
            start.z = f.right - f.left;
        }
        let height = Height::new(camera, options.destination, start.z, destination.z, options.maximum_height);
        // Cesium's shared positional dispatch passes flyOverLongitude as the
        // CV pitch-adjust argument; preserve it rather than silently fixing it.
        let threshold = if options.mode == SceneMode::ColumbusView {
            options.fly_over_longitude
        } else {
            options.pitch_adjust_height
        };
        let pitch_adjust = threshold
            .filter(|h| height.value(0.5) > *h)
            .map(|_| (height.value(0.), height.value(1.), height.value(0.5)));
        Ok(Self {
            mode: options.mode,
            start,
            destination,
            start_hpr: HeadingPitchRoll::new(heading, pitch, roll),
            end_hpr: options.orientation,
            height,
            pitch_adjust,
            ellipsoid,
        })
    }
    fn update(&self, camera: &mut Camera, time: f64) -> Result<(), CameraError> {
        let pitch = if let Some((start, end, middle)) = self.pitch_adjust {
            let altitude = self.height.value(time);
            if time <= 0.5 {
                lerp(self.start_hpr.pitch, -PI_OVER_TWO, (altitude - start) / (middle - start))
            } else {
                lerp(-PI_OVER_TWO, self.end_hpr.pitch, 1. - (altitude - end) / (middle - end))
            }
        } else {
            lerp(self.start_hpr.pitch, self.end_hpr.pitch, time)
        };
        let hpr = HeadingPitchRoll::new(
            lerp(self.start_hpr.heading, self.end_hpr.heading, time),
            pitch,
            lerp(self.start_hpr.roll, self.end_hpr.roll, time),
        );
        if self.mode == SceneMode::Scene3D {
            let p = self.ellipsoid.cartographic_to_cartesian(&Cartographic::new(
                lerp(self.start.x, self.destination.x, time),
                lerp(self.start.y, self.destination.y, time),
                self.height.value(time),
            ));
            camera.set_view(CameraView {
                destination: Some(CameraDestination::Cartesian(p)),
                orientation: Some(CameraOrientation::HeadingPitchRoll(hpr)),
                ..CameraView::default()
            })?;
        } else {
            let hpr = if self.mode == SceneMode::Scene2D {
                HeadingPitchRoll::new(hpr.heading, -PI_OVER_TWO, 0.)
            } else {
                hpr
            };
            camera.set_view(CameraView {
                orientation: Some(CameraOrientation::HeadingPitchRoll(hpr)),
                ..CameraView::default()
            })?;
            camera.position.x = lerp(self.start.x, self.destination.x, time);
            camera.position.y = lerp(self.start.y, self.destination.y, time);
            if self.mode == SceneMode::Scene2D {
                if let CameraFrustum::OrthographicOffCenter(f) = &mut camera.frustum {
                    let ratio = f.top / f.right;
                    let increment = (self.height.value(time) - (f.right - f.left)) * 0.5;
                    f.right += increment;
                    f.left -= increment;
                    f.top = ratio * f.right;
                    f.bottom = -f.top;
                }
            } else {
                camera.position.z = self.height.value(time);
            }
        }
        Ok(())
    }
}

enum TweenPath {
    Empty,
    Flight(Box<FlightPath>),
    Deferred(PathSettings),
}
/// An executable Cesium flight descriptor. `update` accepts the tween's
/// interpolated `time` value; it does not apply easing a second time.
pub struct CameraFlightTween {
    pub duration: f64,
    pub easing_function: Option<CameraEasing>,
    pub start_time: Option<f64>,
    pub stop_time: Option<f64>,
    path: TweenPath,
    complete: RefCell<Option<CameraFlightCallback>>,
    cancel: RefCell<Option<CameraFlightCallback>>,
    wrapped: bool,
}
impl CameraFlightTween {
    pub fn update(&self, camera: &mut Camera, time: f64) -> Result<(), CameraError> {
        if let TweenPath::Flight(path) = &self.path {
            path.update(camera, time / self.duration)?;
        }
        Ok(())
    }
    fn callback(slot: &RefCell<Option<CameraFlightCallback>>, camera: &mut Camera) {
        // Do not hold a RefCell borrow across user code: callbacks may start,
        // complete or cancel another flight on this camera.
        let callback = slot.borrow_mut().take();
        if let Some(mut callback) = callback {
            callback(camera);
            *slot.borrow_mut() = Some(callback);
        }
    }
    pub fn complete(&self, camera: &mut Camera) -> Result<(), CameraError> {
        if let TweenPath::Deferred(settings) = self.path {
            FlightPath::new(camera, settings)?.update(camera, 1.)?;
        }
        Self::callback(&self.complete, camera);
        if self.wrapped {
            camera.enable_inputs = true;
        }
        Ok(())
    }
    pub fn cancel(&self, camera: &mut Camera) {
        Self::callback(&self.cancel, camera);
        if self.wrapped {
            camera.enable_inputs = true;
        }
    }
}

pub struct CameraFlightPath;
impl CameraFlightPath {
    pub fn create_tween(
        camera: &mut Camera,
        mut options: CameraFlightPathOptions,
    ) -> Result<CameraFlightTween, CameraError> {
        let empty = |path, wrapped, complete, cancel| CameraFlightTween {
            duration: 0.,
            easing_function: None,
            start_time: None,
            stop_time: None,
            path,
            complete: RefCell::new(complete),
            cancel: RefCell::new(cancel),
            wrapped,
        };
        if camera.mode() == SceneMode::Morphing {
            return Ok(empty(TweenPath::Empty, false, None, None));
        }
        if options.convert && camera.mode() != SceneMode::Scene3D {
            let cartographic = camera
                .map_projection()
                .ellipsoid()
                .cartesian_to_cartographic(options.destination)
                .ok_or(CameraError("destination must have a cartographic position"))?;
            options.destination = camera.map_projection().project(&cartographic);
        }
        if let Some(transform) = options.end_transform {
            camera.set_transform(transform)?;
        }
        let duration = options
            .duration
            .unwrap_or_else(|| ((camera.position.distance(options.destination) / 1_000_000.).ceil() + 2.).min(3.));
        camera.enable_inputs = false;
        let position_empty = if camera.mode() == SceneMode::Scene2D {
            let f = camera.frustum.orthographic().expect("2D frustum");
            equals_epsilon(camera.position.x, options.destination.x, EPSILON6, EPSILON6)
                && equals_epsilon(camera.position.y, options.destination.y, EPSILON6, EPSILON6)
                && equals_epsilon((f.right - f.left).max(f.top - f.bottom), options.destination.z, EPSILON6, EPSILON6)
        } else {
            camera.position.equals_epsilon(options.destination, EPSILON10)
        };
        let hpr = options.orientation;
        let is_empty = position_empty
            && equals_epsilon(
                negative_pi_to_pi(hpr.heading),
                negative_pi_to_pi(camera.heading().unwrap_or(f64::NAN)),
                EPSILON10,
                EPSILON10,
            )
            && equals_epsilon(
                negative_pi_to_pi(hpr.pitch),
                negative_pi_to_pi(camera.pitch().unwrap_or(f64::NAN)),
                EPSILON10,
                EPSILON10,
            )
            && equals_epsilon(
                negative_pi_to_pi(hpr.roll),
                negative_pi_to_pi(camera.roll().unwrap_or(f64::NAN)),
                EPSILON10,
                EPSILON10,
            );
        if is_empty {
            return Ok(empty(TweenPath::Empty, true, options.complete, options.cancel));
        }
        let settings = PathSettings {
            mode: camera.mode(),
            destination: options.destination,
            orientation: hpr,
            maximum_height: options.maximum_height,
            pitch_adjust_height: options.pitch_adjust_height,
            fly_over_longitude: options.fly_over_longitude,
            fly_over_longitude_weight: options.fly_over_longitude_weight,
        };
        if duration <= 0. {
            return Ok(empty(TweenPath::Deferred(settings), true, options.complete, options.cancel));
        }
        let path = FlightPath::new(camera, settings)?;
        let easing = options.easing_function.unwrap_or_else(|| {
            let start = camera.position_cartographic().map_or(f64::NAN, |p| p.height);
            let end = if camera.mode() == SceneMode::Scene3D {
                camera
                    .map_projection()
                    .ellipsoid()
                    .cartesian_to_cartographic(options.destination)
                    .map_or(f64::NAN, |p| p.height)
            } else {
                options.destination.z
            };
            CameraEasing::Builtin(if start > end && start > 11_500. {
                EasingFunction::CubicOut
            } else {
                EasingFunction::QuinticInOut
            })
        });
        Ok(CameraFlightTween {
            duration,
            easing_function: Some(easing),
            start_time: Some(0.),
            stop_time: Some(duration),
            path: TweenPath::Flight(Box::new(path)),
            complete: RefCell::new(options.complete),
            cancel: RefCell::new(options.cancel),
            wrapped: true,
        })
    }
}

pub(crate) struct ScheduledFlight {
    pub id: u64,
    pub tween: Rc<CameraFlightTween>,
    pub started: Option<f64>,
}
#[derive(Clone)]
pub(crate) struct CurrentFlight {
    pub id: u64,
    pub tween: Rc<CameraFlightTween>,
    pub destination: Cartesian3,
    pub orientation: HeadingPitchRoll,
}
