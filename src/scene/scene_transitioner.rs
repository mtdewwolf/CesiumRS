//! Explicit-time scene morph lifecycle and camera/frustum animations.
//!
//! Scene owns the renderer; this adapter owns morph state and events. A host
//! copies `mode`/`morph_time` into its frame state and forwards user input.
use crate::{
    core::*,
    scene::{Camera, CameraDestination, CameraFlightOptions, CameraFrustum, SceneMode},
};
use std::{collections::VecDeque, f64::consts::FRAC_PI_3};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct MorphEvent {
    pub previous_mode: SceneMode,
    pub mode: SceneMode,
    pub was_morphing: bool,
}
#[derive(Clone, Copy)]
struct Pose {
    position: Cartesian3,
    direction: Cartesian3,
    up: Cartesian3,
    frustum: CameraFrustum,
}
impl Pose {
    fn of(c: &Camera) -> Self {
        Self { position: c.position, direction: c.direction, up: c.up, frustum: c.frustum }
    }
    fn transformed(self, m: Matrix4) -> Self {
        Self {
            position: m.multiply_by_point(self.position),
            direction: m.multiply_by_point_as_vector(self.direction),
            up: m.multiply_by_point_as_vector(self.up),
            ..self
        }
    }
    fn apply(self, c: &mut Camera) {
        c.position = self.position;
        c.direction = self.direction;
        c.up = self.up;
        c.right = self.direction.cross(self.up).normalize();
    }
}
#[derive(Clone, Copy)]
enum Phase {
    Wait,
    Pose { end: Pose, duration: f64, set_z: bool, frustum: bool },
    Morph { from: f64, to: f64, duration: f64 },
    ToOrthographic { end: Pose, duration: f64, x_height: bool },
    ToPerspective { end: Pose, duration: f64 },
    Flight { duration: f64 },
}
struct Active {
    phase: Phase,
    start: f64,
    pose: Pose,
    d: f64,
    start_fov: f64,
}
pub struct SceneTransitioner {
    pub mode: SceneMode,
    pub morph_time: f64,
    pub complete_morph_on_user_input: bool,
    pub use_log_depth: bool,
    pub morph_start: Event<MorphEvent>,
    pub morph_complete: Event<MorphEvent>,
    previous_mode: SceneMode,
    target: Option<(SceneMode, Pose)>,
    phases: VecDeque<Phase>,
    active: Option<Active>,
    morph_to_orthographic: bool,
    handler_active: bool,
    destroyed: bool,
}
impl SceneTransitioner {
    pub fn new(camera: &Camera) -> Self {
        Self {
            mode: camera.mode(),
            morph_time: camera.mode().get_morph_time().unwrap_or(1.),
            complete_morph_on_user_input: true,
            use_log_depth: false,
            morph_start: Event::new(),
            morph_complete: Event::new(),
            previous_mode: camera.mode(),
            target: None,
            phases: VecDeque::new(),
            active: None,
            morph_to_orthographic: false,
            handler_active: false,
            destroyed: false,
        }
    }
    pub fn is_morphing(&self) -> bool {
        self.target.is_some()
    }
    pub fn morph_to_2d(&mut self, camera: &mut Camera, duration: f64) -> Result<bool, RuntimeError> {
        self.begin(camera, SceneMode::Scene2D, duration)
    }
    pub fn morph_to_columbus_view(&mut self, camera: &mut Camera, duration: f64) -> Result<bool, RuntimeError> {
        self.begin(camera, SceneMode::ColumbusView, duration)
    }
    pub fn morph_to_3d(&mut self, camera: &mut Camera, duration: f64) -> Result<bool, RuntimeError> {
        self.begin(camera, SceneMode::Scene3D, duration)
    }
    fn begin(&mut self, c: &mut Camera, to: SceneMode, duration: f64) -> Result<bool, RuntimeError> {
        self.alive()?;
        if duration < 0. || !duration.is_finite() {
            return Err(RuntimeError::new("duration must be finite and nonnegative."));
        }
        if self.target.is_some() {
            self.complete_morph(c)?;
        }
        self.previous_mode = self.mode;
        if to == self.mode || self.mode == SceneMode::Morphing {
            return Ok(false);
        }
        let from = self.mode;
        self.handler_active = self.complete_morph_on_user_input;
        if to == SceneMode::Scene2D {
            self.morph_to_orthographic = matches!(c.frustum, CameraFrustum::Orthographic(_));
        }
        self.morph_start.raise_event(&MorphEvent { previous_mode: from, mode: to, was_morphing: true });
        c.set_transform(Matrix4::IDENTITY).map_err(camera_error)?;
        let radius = c.ellipsoid.maximum_radius;
        let aspect = c.drawing_buffer_width / c.drawing_buffer_height;
        let perspective = CameraFrustum::Perspective(PerspectiveFrustum::new(FRAC_PI_3, aspect));
        let width = frustum_width(c.frustum);
        let cv_frustum = if self.morph_to_orthographic {
            CameraFrustum::Orthographic(OrthographicFrustum::new(width, aspect))
        } else {
            perspective
        };
        let mut end = Pose::of(c);
        match to {
            SceneMode::Scene2D => {
                end.position = if duration > 0. {
                    Cartesian3::new(0., 0., 5. * radius)
                } else if from == SceneMode::Scene3D {
                    let position = c.position_wc();
                    let cart = c
                        .ellipsoid
                        .cartesian_to_cartographic(position)
                        .ok_or_else(|| RuntimeError::new("camera position must be away from the ellipsoid center"))?;
                    c.map_projection().project(&cart)
                } else {
                    c.position
                };
                if duration == 0. && from == SceneMode::ColumbusView {
                    let ray = Ray::new(
                        Camera::TRANSFORM_2D.multiply_by_point(c.position),
                        Camera::TRANSFORM_2D.multiply_by_point_as_vector(c.direction),
                    );
                    if let Some(hit) = c.surface.as_ref().and_then(|surface| surface.pick_globe(&ray)) {
                        end.position = Camera::TRANSFORM_2D_INVERSE.multiply_by_point(hit);
                        end.position.z += c.position.distance(end.position);
                    }
                }
                end.direction = Cartesian3::UNIT_Z.negate();
                end.up = Cartesian3::UNIT_Y;
                end.frustum = CameraFrustum::OrthographicOffCenter(OrthographicOffCenterFrustum {
                    left: -end.position.z * 0.5,
                    right: end.position.z * 0.5,
                    top: end.position.z * 0.5 / aspect,
                    bottom: -end.position.z * 0.5 / aspect,
                    near: 1.,
                    far: 500_000_000.,
                });
                let d = duration * 0.5;
                if from == SceneMode::Scene3D {
                    self.phases.push_back(Phase::Pose {
                        end: end.transformed(Camera::TRANSFORM_2D),
                        duration: d,
                        set_z: false,
                        frustum: false,
                    });
                    self.phases.push_back(Phase::Morph { from: 1., to: 0., duration: d });
                } else {
                    self.phases.push_back(Phase::Pose { end, duration: d, set_z: false, frustum: false });
                }
                // Upstream returns without completing for an OrthographicFrustum;
                // retain the morph until completeMorph or user input.
                if !matches!(c.frustum, CameraFrustum::Orthographic(_)) {
                    self.phases.push_back(Phase::ToOrthographic {
                        end,
                        duration: d,
                        x_height: from == SceneMode::Scene3D,
                    });
                } else {
                    self.phases.push_back(Phase::Wait);
                }
            }
            SceneMode::ColumbusView => {
                if duration > 0. {
                    end.position = Cartesian3::new(0., -1., 1.).normalize().multiply_by_scalar(5. * radius);
                    end.direction = end.position.normalize().negate();
                    end.up = Cartesian3::UNIT_X.cross(end.direction);
                } else if from == SceneMode::Scene2D {
                    end.position.z = width;
                    end.direction = Cartesian3::UNIT_Z.negate();
                    end.up = Cartesian3::UNIT_Y;
                } else {
                    end = to_projected(c)?;
                }
                end.frustum = cv_frustum;
                if from == SceneMode::Scene3D {
                    self.phases.push_back(Phase::Pose {
                        end: end.transformed(Camera::TRANSFORM_2D),
                        duration,
                        set_z: false,
                        frustum: false,
                    });
                    self.phases.push_back(Phase::Morph { from: 1., to: 0., duration });
                } else {
                    if !self.morph_to_orthographic {
                        self.phases.push_back(Phase::ToPerspective { end, duration: 0. });
                    }
                    self.phases.push_back(Phase::Pose { end, duration: duration * 0.5, set_z: true, frustum: true });
                }
            }
            SceneMode::Scene3D => {
                if duration > 0. {
                    end.position = c.ellipsoid.cartographic_to_cartesian(&Cartographic::new(0., 0., 5. * radius));
                    end.direction = end.position.normalize().negate();
                    end.up = Cartesian3::UNIT_Z;
                } else {
                    let mut position = c.position;
                    if from == SceneMode::Scene2D {
                        position.z = width;
                        c.position.z = width;
                    }
                    let cart = c.map_projection().unproject(position);
                    end.position = c.ellipsoid.cartographic_to_cartesian(&cart);
                    let surface = c
                        .ellipsoid
                        .scale_to_geodetic_surface(end.position)
                        .ok_or_else(|| RuntimeError::new("camera position must be away from the ellipsoid center"))?;
                    let enu = Transforms::east_north_up_to_fixed_frame(surface, &c.ellipsoid);
                    end.direction = enu.multiply_by_point_as_vector(c.direction);
                    end.up = enu.multiply_by_point_as_vector(c.up);
                }
                end.frustum = if from == SceneMode::ColumbusView {
                    match c.frustum {
                        CameraFrustum::Orthographic(_) => c.frustum,
                        _ => perspective,
                    }
                } else if self.morph_to_orthographic {
                    CameraFrustum::Orthographic(OrthographicFrustum::new(width, aspect))
                } else {
                    perspective
                };
                let d = if from == SceneMode::Scene2D { duration / 3. } else { duration };
                if from == SceneMode::Scene2D {
                    if duration > 0. {
                        self.phases.push_back(Phase::Flight { duration: d });
                    }
                    if !self.morph_to_orthographic {
                        self.phases.push_back(Phase::ToPerspective { end, duration: d });
                    }
                }
                self.phases.push_back(Phase::Pose {
                    end: end.transformed(Camera::TRANSFORM_2D_INVERSE),
                    duration: d * 0.5,
                    set_z: false,
                    frustum: false,
                });
                self.phases.push_back(Phase::Morph { from: 0., to: 1., duration: d * 0.5 });
            }
            _ => unreachable!(),
        }
        self.mode = if matches!(self.phases.front(), Some(Phase::Flight { .. })) {
            SceneMode::Scene2D
        } else {
            SceneMode::Morphing
        };
        c.update(self.mode).map_err(camera_error)?;
        self.target = Some((to, end));
        if duration == 0.
            && to == SceneMode::Scene2D
            && matches!(c.frustum, CameraFrustum::Orthographic(_))
            && !self.handler_active
        {
            self.phases.clear();
            self.phases.push_back(Phase::Wait);
            self.prepare(c)?;
        } else if duration == 0. {
            self.finish(c)?;
        } else {
            self.prepare(c)?;
            // A zero-duration perspective conversion invokes its completion
            // synchronously before the first timed camera phase is scheduled.
            if matches!(self.active.as_ref().map(|a| a.phase), Some(Phase::ToPerspective { duration: 0., .. })) {
                self.active = None;
                self.prepare(c)?;
            }
        }
        Ok(true)
    }
    fn prepare(&mut self, c: &mut Camera) -> Result<(), RuntimeError> {
        let Some(phase) = self.phases.pop_front() else {
            self.finish(c)?;
            return Ok(());
        };
        let mut pose = Pose::of(c);
        let mut d = 0.;
        let mut start_fov = 0.;
        match phase {
            Phase::Pose { end, set_z, frustum, .. } => {
                if set_z {
                    pose.position.z = end.position.z;
                }
                if frustum {
                    c.frustum = end.frustum;
                }
            }
            Phase::ToOrthographic { end, .. } => {
                if let CameraFrustum::Perspective(mut f) = c.frustum {
                    start_fov = f.fov;
                    d = end.position.z * (f.fov * 0.5).tan();
                    f.far = d / (0.5_f64.to_radians() * 0.5).tan() + 10000000.;
                    c.frustum = CameraFrustum::Perspective(f);
                }
            }
            Phase::ToPerspective { end, .. } => {
                let height = frustum_width(c.frustum);
                c.frustum = end.frustum;
                if let CameraFrustum::Perspective(mut f) = c.frustum {
                    start_fov = 0.5_f64.to_radians();
                    d = height * (f.fov * 0.5).tan();
                    f.far = d / (start_fov * 0.5).tan() + 10000000.;
                    f.fov = start_fov;
                    c.frustum = CameraFrustum::Perspective(f);
                }
            }
            Phase::Flight { duration } => {
                c.update(SceneMode::Scene2D).map_err(camera_error)?;
                let destination =
                    c.ellipsoid.cartographic_to_cartesian(&Cartographic::new(0., 0., 5. * c.ellipsoid.maximum_radius));
                let mut options = CameraFlightOptions::new(CameraDestination::Cartesian(destination));
                options.duration = Some(duration);
                c.fly_to(options).map_err(camera_error)?;
            }
            _ => {}
        }
        self.active = Some(Active { phase, start: f64::NAN, pose, d, start_fov });
        Ok(())
    }
    /// Advance phases at frame time in seconds. Each new phase starts in the
    /// same update as its predecessor's completion, matching TweenCollection.
    pub fn update(&mut self, c: &mut Camera, time: f64) -> Result<(), RuntimeError> {
        self.alive()?;
        while self.target.is_some() {
            if self.active.is_none() {
                self.prepare(c)?;
            }
            if self.active.is_none() {
                break;
            }
            if self.active.as_ref().unwrap().start.is_nan() {
                self.active.as_mut().unwrap().start = time;
            }
            let active = self.active.as_ref().unwrap();
            if matches!(active.phase, Phase::Wait) {
                break;
            }
            let duration = match active.phase {
                Phase::Wait => unreachable!(),
                Phase::Pose { duration, .. }
                | Phase::Morph { duration, .. }
                | Phase::ToOrthographic { duration, .. }
                | Phase::ToPerspective { duration, .. }
                | Phase::Flight { duration } => duration,
            };
            let t = if duration == 0. { 1. } else { ((time - active.start) / duration).clamp(0., 1.) };
            let value = EasingFunction::QuarticOut.evaluate(t);
            if duration > 0. {
                match active.phase {
                    Phase::Wait => unreachable!(),
                    Phase::Pose { end, .. } => {
                        c.position = active.pose.position.lerp(end.position, value);
                        c.direction = active.pose.direction.lerp(end.direction, value);
                        c.up = active.pose.up.lerp(end.up, value);
                        c.right = c.direction.cross(c.up).normalize();
                        if matches!(c.frustum, CameraFrustum::Orthographic(_))
                            && (self.previous_mode == SceneMode::Scene3D
                                || self.target.is_some_and(|(mode, _)| mode == SceneMode::Scene2D))
                        {
                            let width = c.position_cartographic().map_or(f64::NAN, |p| p.height.max(0.));
                            if let CameraFrustum::Orthographic(f) = &mut c.frustum {
                                f.width = width;
                            }
                        }
                    }
                    Phase::Morph { from, to, .. } => {
                        self.morph_time = from + (to - from) * value;
                    }
                    Phase::ToOrthographic { x_height, .. } => {
                        if let CameraFrustum::Perspective(mut f) = c.frustum {
                            f.fov = active.start_fov + (0.5_f64.to_radians() - active.start_fov) * value;
                            let height = active.d / (f.fov * 0.5).tan();
                            if x_height {
                                c.position.x = height;
                            } else {
                                c.position.z = height;
                            }
                            c.frustum = CameraFrustum::Perspective(f);
                        }
                    }
                    Phase::ToPerspective { end, .. } => {
                        if let (CameraFrustum::Perspective(mut f), CameraFrustum::Perspective(end)) =
                            (c.frustum, end.frustum)
                        {
                            f.fov = active.start_fov + (end.fov - active.start_fov) * value;
                            c.position.z = active.d / (f.fov * 0.5).tan();
                            c.frustum = CameraFrustum::Perspective(f);
                        }
                    }
                    Phase::Flight { .. } => {
                        c.update_flights(time).map_err(camera_error)?;
                    }
                }
            }
            if t < 1. {
                break;
            }
            let active = self.active.take().unwrap();
            if let Phase::ToOrthographic { end, .. } = active.phase {
                c.frustum = end.frustum;
            }
            if let Phase::Flight { .. } = active.phase {
                c.complete_flight().map_err(camera_error)?;
                self.mode = SceneMode::Morphing;
                c.update(SceneMode::Morphing).map_err(camera_error)?;
            }
        }
        Ok(())
    }
    pub fn complete_morph(&mut self, c: &mut Camera) -> Result<bool, RuntimeError> {
        self.alive()?;
        if self.target.is_some() && self.handler_active {
            c.cancel_flight();
            self.finish(c)?;
            Ok(true)
        } else {
            Ok(false)
        }
    }
    pub fn process_input(
        &mut self,
        c: &mut Camera,
        kind: ScreenSpaceEventType,
        modifier: Option<KeyboardEventModifier>,
    ) -> Result<bool, RuntimeError> {
        self.alive()?;
        if modifier.is_none()
            && matches!(
                kind,
                ScreenSpaceEventType::LeftDown
                    | ScreenSpaceEventType::RightDown
                    | ScreenSpaceEventType::MiddleDown
                    | ScreenSpaceEventType::Wheel
            )
        {
            self.complete_morph(c)
        } else {
            Ok(false)
        }
    }
    fn finish(&mut self, c: &mut Camera) -> Result<(), RuntimeError> {
        let Some((mode, mut end)) = self.target.take() else {
            return Ok(());
        };
        self.phases.clear();
        self.active = None;
        if mode == SceneMode::Scene2D {
            end.position.z = c.ellipsoid.maximum_radius * 2.;
        }
        end.apply(c);
        if mode != SceneMode::ColumbusView || matches!(c.frustum, CameraFrustum::OrthographicOffCenter(_)) {
            c.frustum = end.frustum;
        }
        if self.use_log_depth && mode != SceneMode::Scene2D {
            match &mut c.frustum {
                CameraFrustum::Perspective(f) => {
                    f.near = 0.1;
                    f.far = 1e10;
                }
                CameraFrustum::Orthographic(f) => {
                    f.near = 0.1;
                    f.far = 1e10;
                }
                _ => {}
            }
        }
        self.mode = mode;
        self.morph_time = mode.get_morph_time().unwrap();
        c.update(mode).map_err(camera_error)?;
        self.morph_complete.raise_event(&MorphEvent {
            previous_mode: self.previous_mode,
            mode,
            was_morphing: self.handler_active,
        });
        Ok(())
    }
    pub fn is_destroyed(&self) -> bool {
        self.destroyed
    }
    pub fn destroy(&mut self) {
        self.destroyed = true;
        self.phases.clear();
        self.active = None;
        self.target = None;
    }
    fn alive(&self) -> Result<(), RuntimeError> {
        if self.destroyed {
            Err(RuntimeError::new("This object was destroyed, i.e., destroy() was called."))
        } else {
            Ok(())
        }
    }
}
fn frustum_width(f: CameraFrustum) -> f64 {
    match f {
        CameraFrustum::OrthographicOffCenter(f) => f.right - f.left,
        CameraFrustum::Orthographic(f) => f.width,
        CameraFrustum::Perspective(_) => f64::NAN,
    }
}
fn camera_error(e: crate::scene::CameraError) -> RuntimeError {
    RuntimeError::new(e.0)
}
fn to_projected(c: &mut Camera) -> Result<Pose, RuntimeError> {
    let mut pose = Pose::of(c);
    let p = c.position_wc();
    let direction = c.direction_wc();
    let up = c.up_wc();
    let surface = c
        .ellipsoid
        .scale_to_geodetic_surface(p)
        .ok_or_else(|| RuntimeError::new("camera position must be away from the ellipsoid center"))?;
    let enu = Transforms::east_north_up_to_fixed_frame(surface, &c.ellipsoid).inverse_transformation();
    let cart = c.ellipsoid.cartesian_to_cartographic(p).ok_or_else(|| RuntimeError::new("invalid camera position"))?;
    pose.position = c.map_projection().project(&cart);
    pose.direction = enu.multiply_by_point_as_vector(direction);
    pose.up = enu.multiply_by_point_as_vector(up);
    Ok(pose)
}
