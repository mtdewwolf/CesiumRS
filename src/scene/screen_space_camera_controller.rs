//! Camera navigation driven by aggregated input. Hosts supply input and time;
//! optional globe/depth picks use `Camera::surface`. See `ControllerEnvironment`
//! for scene values that do not belong to the camera itself.
use crate::core::*;
use crate::scene::{
    Camera, CameraCorrectionTween, CameraEventAggregator, CameraEventMovement, CameraEventType, CameraFrustum,
    CameraMovement, MapMode2D, SceneMode,
};
use std::f64::consts::{PI, TAU};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct CameraInputBinding {
    pub event_type: CameraEventType,
    pub modifier: Option<KeyboardEventModifier>,
}
impl From<CameraEventType> for CameraInputBinding {
    fn from(event_type: CameraEventType) -> Self {
        Self { event_type, modifier: None }
    }
}
impl CameraInputBinding {
    pub fn modified(event_type: CameraEventType, modifier: KeyboardEventModifier) -> Self {
        Self { event_type, modifier: Some(modifier) }
    }
}
#[derive(Clone, Copy, Debug)]
pub struct ControllerEnvironment {
    pub has_globe: bool,
    pub globe_height: Option<f64>,
    pub camera_underground: bool,
    pub vertical_exaggeration: f64,
    pub vertical_exaggeration_relative_height: f64,
}
impl Default for ControllerEnvironment {
    fn default() -> Self {
        Self {
            has_globe: false,
            globe_height: None,
            camera_underground: false,
            vertical_exaggeration: 1.,
            vertical_exaggeration_relative_height: 0.,
        }
    }
}
pub struct ScreenSpaceCameraController {
    pub enable_inputs: bool,
    pub enable_translate: bool,
    pub enable_zoom: bool,
    pub enable_rotate: bool,
    pub enable_tilt: bool,
    pub enable_look: bool,
    pub inertia_spin: f64,
    pub inertia_translate: f64,
    pub inertia_zoom: f64,
    pub maximum_movement_ratio: f64,
    pub bounce_animation_time: f64,
    pub minimum_zoom_distance: f64,
    pub maximum_zoom_distance: f64,
    pub zoom_factor: f64,
    pub translate_event_types: Vec<CameraInputBinding>,
    pub zoom_event_types: Vec<CameraInputBinding>,
    pub rotate_event_types: Vec<CameraInputBinding>,
    pub tilt_event_types: Vec<CameraInputBinding>,
    pub look_event_types: Vec<CameraInputBinding>,
    pub minimum_picking_terrain_height: f64,
    pub minimum_picking_terrain_distance_with_inertia: f64,
    pub minimum_collision_terrain_height: f64,
    pub minimum_track_ball_height: f64,
    pub enable_collision_detection: bool,
    pub maximum_tilt_angle: Option<f64>,
    pub aggregator: CameraEventAggregator,
    inertia_enabled: [bool; 4],
    correction: Option<(CameraCorrectionTween, f64)>,
    last_globe_height: f64,
    adjusted_height_for_terrain: bool,
    pan_mouse: Cartesian2,
    pan_world: Cartesian3,
    pan_start: Option<(Cartesian2, Ellipsoid)>,
    spin_looking: Option<Cartesian2>,
    spin_rotating: Option<Cartesian2>,
    strafe: Option<(Cartesian2, Cartesian3, Cartesian2)>,
    tilt_center: Option<(Cartesian2, Cartesian3)>,
    tilt_looking: bool,
    tilt_on_ellipsoid: bool,
    zoom_start: Option<Cartesian2>,
    zoom_world: Option<Cartesian3>,
    zooming_vector: bool,
    rotating_zoom: bool,
    zooming_underground: bool,
    pre_intersection_distance: f64,
    destroyed: bool,
}
#[derive(Clone, Copy)]
enum Action {
    Translate,
    Zoom,
    Spin,
    Tilt,
    Look,
    Twist,
    Rotate,
}
impl ScreenSpaceCameraController {
    pub fn new(camera: &Camera) -> Self {
        use CameraEventType::*;
        let e = camera.ellipsoid;
        let wgs = e == Ellipsoid::WGS84;
        let r = e.minimum_radius;
        Self {
            enable_inputs: true,
            enable_translate: true,
            enable_zoom: true,
            enable_rotate: true,
            enable_tilt: true,
            enable_look: true,
            inertia_spin: 0.9,
            inertia_translate: 0.9,
            inertia_zoom: 0.8,
            maximum_movement_ratio: 0.1,
            bounce_animation_time: 3.,
            minimum_zoom_distance: 1.,
            maximum_zoom_distance: f64::INFINITY,
            zoom_factor: 5.,
            translate_event_types: vec![LeftDrag.into()],
            zoom_event_types: vec![RightDrag.into(), Wheel.into(), Pinch.into()],
            rotate_event_types: vec![LeftDrag.into()],
            tilt_event_types: vec![
                MiddleDrag.into(),
                Pinch.into(),
                CameraInputBinding::modified(LeftDrag, KeyboardEventModifier::Ctrl),
                CameraInputBinding::modified(RightDrag, KeyboardEventModifier::Ctrl),
            ],
            look_event_types: vec![CameraInputBinding::modified(LeftDrag, KeyboardEventModifier::Shift)],
            minimum_picking_terrain_height: if wgs { 150000. } else { r * 0.025 },
            minimum_picking_terrain_distance_with_inertia: if wgs { 4000. } else { r * 0.00063 },
            minimum_collision_terrain_height: if wgs { 15000. } else { r * 0.0025 },
            minimum_track_ball_height: if wgs { 7500000. } else { r * 1.175 },
            enable_collision_detection: true,
            maximum_tilt_angle: None,
            aggregator: CameraEventAggregator::new(camera.client_width),
            inertia_enabled: [true; 4],
            correction: None,
            last_globe_height: 0.,
            adjusted_height_for_terrain: false,
            pan_mouse: Cartesian2::ZERO,
            pan_world: Cartesian3::ZERO,
            pan_start: None,
            spin_looking: None,
            spin_rotating: None,
            strafe: None,
            tilt_center: None,
            tilt_looking: false,
            tilt_on_ellipsoid: false,
            zoom_start: None,
            zoom_world: None,
            zooming_vector: false,
            rotating_zoom: false,
            zooming_underground: false,
            pre_intersection_distance: 0.,
            destroyed: false,
        }
    }
    pub fn process(&mut self, event: &ScreenInput) -> Result<bool, RuntimeError> {
        self.alive()?;
        self.aggregator.process(event)
    }
    pub fn on_map(&self, camera: &Camera) -> bool {
        let max = camera.map_projection().project(&Cartographic::new(PI, PI / 2., 0.));
        camera.mode() != SceneMode::ColumbusView || (camera.position.x.abs() < max.x && camera.position.y.abs() < max.y)
    }
    pub fn update(&mut self, camera: &mut Camera, time: f64) -> Result<(), RuntimeError> {
        self.update_with_environment(camera, time, ControllerEnvironment::default())
    }
    pub fn update_with_environment(
        &mut self,
        camera: &mut Camera,
        time: f64,
        environment: ControllerEnvironment,
    ) -> Result<(), RuntimeError> {
        self.alive()?;
        self.aggregator.set_client_width(camera.client_width);
        self.adjusted_height_for_terrain = false;
        let previous = (camera.position_wc(), camera.direction_wc());
        let mode = camera.mode();
        let transformed = camera.transform() != Matrix4::IDENTITY;
        let mut actions = Vec::new();
        let mut add = |enabled, bindings: Vec<CameraInputBinding>, action, coefficient, index| {
            if enabled {
                actions.push((bindings, action, coefficient, index));
            }
        };
        match mode {
            SceneMode::Scene2D => {
                if !transformed {
                    add(
                        self.enable_translate,
                        self.translate_event_types.clone(),
                        Action::Translate,
                        self.inertia_translate,
                        Some(2),
                    );
                }
                add(self.enable_zoom, self.zoom_event_types.clone(), Action::Zoom, self.inertia_zoom, Some(1));
                if camera.map_mode_2d == MapMode2D::Rotate {
                    add(
                        self.enable_rotate,
                        if transformed { self.translate_event_types.clone() } else { self.tilt_event_types.clone() },
                        Action::Twist,
                        self.inertia_spin,
                        Some(0),
                    );
                }
            }
            SceneMode::ColumbusView if !transformed => {
                add(self.enable_tilt, self.tilt_event_types.clone(), Action::Tilt, self.inertia_spin, Some(3));
                add(
                    self.enable_translate,
                    self.translate_event_types.clone(),
                    Action::Translate,
                    self.inertia_translate,
                    Some(2),
                );
                add(self.enable_zoom, self.zoom_event_types.clone(), Action::Zoom, self.inertia_zoom, Some(1));
                add(self.enable_look, self.look_event_types.clone(), Action::Look, 1., None);
            }
            SceneMode::ColumbusView | SceneMode::Scene3D => {
                add(
                    self.enable_rotate,
                    self.rotate_event_types.clone(),
                    if transformed { Action::Rotate } else { Action::Spin },
                    self.inertia_spin,
                    Some(0),
                );
                add(self.enable_zoom, self.zoom_event_types.clone(), Action::Zoom, self.inertia_zoom, Some(1));
                if mode == SceneMode::Scene3D {
                    add(self.enable_tilt, self.tilt_event_types.clone(), Action::Tilt, self.inertia_spin, Some(3));
                    add(self.enable_look, self.look_event_types.clone(), Action::Look, 1., None);
                }
            }
            _ => {}
        }
        if self.enable_inputs && camera.enable_inputs {
            for (bindings, action, coefficient, inertia) in actions {
                for binding in bindings {
                    let t = binding.event_type;
                    let m = binding.modifier;
                    let start = self.aggregator.get_start_mouse_position(t, m);
                    if self.aggregator.is_moving(t, m) {
                        let movement = self.aggregator.get_movement(t, m);
                        self.act(camera, start, movement, action, environment, false);
                        if let Some(i) = inertia {
                            self.inertia_enabled[i] = true;
                            if i == 1 {
                                for j in [0, 2, 3] {
                                    self.inertia_enabled[j] = false;
                                }
                            }
                            if i == 3 {
                                for j in [0, 2] {
                                    self.inertia_enabled[j] = false;
                                }
                            }
                        }
                    } else if coefficient < 1. && inertia.is_some_and(|i| self.inertia_enabled[i]) {
                        if let (Some(press), Some(release), Some(last)) = (
                            self.aggregator.get_button_press_time(t, m),
                            self.aggregator.get_button_release_time(t, m),
                            self.aggregator.get_last_movement(t, m),
                        ) {
                            if release - press < 0.4 && !self.aggregator.is_button_down(t, m) {
                                let d = if time < release {
                                    0.
                                } else {
                                    (-((1. - coefficient) * 25.) * ((time * 1000.).trunc() / 1000. - release)).exp()
                                };
                                let end = last
                                    .start_position
                                    .add(last.end_position.subtract(last.start_position).multiply_by_scalar(d * 0.5));
                                if last.start_position.distance(end) >= 0.5 {
                                    self.act(
                                        camera,
                                        start,
                                        CameraEventMovement {
                                            movement: CameraMovement {
                                                start_position: last.start_position,
                                                end_position: end,
                                            },
                                            ..Default::default()
                                        },
                                        action,
                                        environment,
                                        true,
                                    );
                                }
                            }
                        }
                    }
                }
            }
        }
        if mode == SceneMode::ColumbusView && !transformed {
            if self.aggregator.any_button_down() {
                self.correction = None;
            } else {
                if self.correction.is_none() {
                    self.correction =
                        camera.create_correct_position_tween(self.bounce_animation_time).map(|t| (t, time));
                }
                if let Some((t, start)) = &self.correction {
                    t.update(
                        camera,
                        if t.duration == 0. {
                            1.
                        } else {
                            t.easing_function.evaluate(((time - start) / t.duration).min(1.))
                        },
                    );
                }
                if self.correction.as_ref().is_some_and(|(t, start)| time - start >= t.duration) {
                    self.correction = None;
                }
            }
        }
        if self.enable_collision_detection && !self.adjusted_height_for_terrain {
            let changed = previous != (camera.position_wc(), camera.direction_wc());
            self.adjust_height(camera, environment, changed);
        }
        self.aggregator.reset();
        Ok(())
    }
    fn ellipsoid(&self, c: &Camera) -> Ellipsoid {
        if c.transform() == Matrix4::IDENTITY {
            c.ellipsoid
        } else {
            Ellipsoid::UNIT_SPHERE
        }
    }
    fn act(
        &mut self,
        c: &mut Camera,
        start: Cartesian2,
        event: CameraEventMovement,
        action: Action,
        env: ControllerEnvironment,
        inertia: bool,
    ) {
        match action {
            Action::Zoom => self.zoom(c, start, event.distance.unwrap_or(event.movement), env, inertia),
            Action::Translate => self.translate(c, start, event.movement, env),
            Action::Spin => self.spin(c, start, event.movement, env, inertia),
            Action::Rotate => self.rotate(c, event.movement, None),
            Action::Tilt => self.tilt(c, start, event.angle_and_height.unwrap_or(event.movement), env),
            Action::Look => self.look(
                c,
                event.movement,
                if c.mode() == SceneMode::ColumbusView { Some(Cartesian3::UNIT_Z) } else { None },
            ),
            Action::Twist => self.twist(c, event),
        }
    }
    fn translate(&mut self, c: &mut Camera, start: Cartesian2, m: CameraMovement, env: ControllerEnvironment) {
        if c.mode() == SceneMode::ColumbusView && self.continue_strafe(c, start, m) {
            return;
        }
        let (Some(a), Some(b)) = (c.get_pick_ray(m.start_position), c.get_pick_ray(m.end_position)) else {
            return;
        };
        let diff = if c.mode() == SceneMode::Scene2D {
            a.origin.subtract(b.origin)
        } else {
            let point = if c.position.z < self.exaggerated(self.minimum_picking_terrain_height, env) {
                pick(c, m.start_position, env.has_globe)
            } else {
                None
            };
            let height = point.map_or(0., |p| p.x);
            let underground = env.camera_underground && env.has_globe;
            if underground || (point.is_some() && height > c.position.z) {
                let point = if underground {
                    let distance = point.map(|p| a.origin.distance(p));
                    let surface = (env.globe_height.unwrap_or(0.) - c.position.z).abs();
                    a.get_point(distance.filter(|v| *v <= 10000.).unwrap_or(surface))
                } else {
                    point.unwrap()
                };
                self.strafe = Some((start, point, start));
                strafe_move(c, m.end_position, point);
                return;
            }
            let Some(p) = plane_hit(a, height) else {
                self.look(c, m, Some(Cartesian3::UNIT_Z));
                return;
            };
            let Some(q) = plane_hit(b, height) else {
                self.look(c, m, Some(Cartesian3::UNIT_Z));
                return;
            };
            p.subtract(q)
        };
        let local = Cartesian3::new(diff.y, diff.z, diff.x);
        if local.magnitude() > 0. {
            c.move_direction(local.normalize(), local.magnitude());
        }
    }
    fn continue_strafe(&mut self, c: &mut Camera, start: Cartesian2, m: CameraMovement) -> bool {
        if let Some((gesture, point, previous_end)) = self.strafe.filter(|v| v.0 == start) {
            let end = previous_end.add(m.end_position.subtract(m.start_position));
            self.strafe = Some((gesture, point, end));
            strafe_move(c, end, point);
            true
        } else {
            false
        }
    }
    fn twist(&self, c: &mut Camera, e: CameraEventMovement) {
        if let Some(m) = e.angle_and_height {
            let ratio = ((m.end_position.x - m.start_position.x) / c.client_width).min(self.maximum_movement_ratio);
            c.twist_right(Some(ratio * 4. * PI));
        } else {
            let m = e.movement;
            let a = Cartesian2::new(
                2. * m.start_position.x / c.client_width - 1.,
                2. * (c.client_height - m.start_position.y) / c.client_height - 1.,
            )
            .normalize();
            let b = Cartesian2::new(
                2. * m.end_position.x / c.client_width - 1.,
                2. * (c.client_height - m.end_position.y) / c.client_height - 1.,
            )
            .normalize();
            let theta = |p: Cartesian2| {
                let a = p.x.clamp(-1., 1.).acos();
                if p.y < 0. {
                    TAU - a
                } else {
                    a
                }
            };
            c.twist_right(Some(theta(b) - theta(a)));
        }
    }
    fn rotate(&self, c: &mut Camera, m: CameraMovement, axis: Option<Cartesian3>) {
        self.rotate_axes(c, m, axis, false, false);
    }
    fn rotate_axes(
        &self,
        c: &mut Camera,
        m: CameraMovement,
        axis: Option<Cartesian3>,
        vertical_only: bool,
        horizontal_only: bool,
    ) {
        let radius = self.ellipsoid(c).maximum_radius;
        let rate = ((c.position.magnitude() - radius) / radius).clamp(1. / 5000., 1.77);
        let phi =
            ((m.start_position.x - m.end_position.x) / c.client_width).min(self.maximum_movement_ratio) * rate * TAU;
        let mut theta =
            ((m.start_position.y - m.end_position.y) / c.client_height).min(self.maximum_movement_ratio) * rate * PI;
        if let (Some(axis), Some(max)) = (axis, self.maximum_tilt_angle) {
            let tilt = PI - c.direction.dot(axis).clamp(-1., 1.).acos() + theta;
            if tilt > max {
                theta -= tilt - max;
            }
        }
        let old = c.constrained_axis;
        if axis.is_some() {
            c.constrained_axis = axis;
        }
        if !vertical_only {
            c.rotate_right(Some(phi));
        }
        if !horizontal_only {
            c.rotate_up(Some(theta));
        }
        c.constrained_axis = old;
    }
    fn spin(
        &mut self,
        c: &mut Camera,
        start: Cartesian2,
        m: CameraMovement,
        env: ControllerEnvironment,
        inertia: bool,
    ) {
        let mut ellipsoid = self.ellipsoid(c);
        if c.transform() != Matrix4::IDENTITY {
            self.rotate(c, m, None);
            return;
        }
        if self.continue_strafe(c, start, m) {
            return;
        }
        if self.spin_looking == Some(start) {
            self.look(c, m, ellipsoid.geodetic_surface_normal(c.position));
            return;
        }
        if self.spin_rotating == Some(start) {
            self.rotate(c, m, None);
            return;
        }
        if let Some((old, e)) = self.pan_start {
            if old == start && c.position.magnitude() < e.maximum_radius {
                return;
            }
        }
        if let Some((old, e)) = self.pan_start {
            if old == start {
                ellipsoid = e;
            }
        }
        let p = c.pick_ellipsoid(m.start_position, &ellipsoid);
        if p.is_none() {
            let height = ellipsoid.cartesian_to_cartographic(c.position).map_or(0., |v| v.height);
            if height > self.exaggerated(self.minimum_track_ball_height, env) {
                self.spin_rotating = Some(start);
                self.rotate(c, m, None);
            } else {
                self.spin_looking = Some(start);
                let up = ellipsoid.geodetic_surface_normal(c.position);
                self.look(c, m, up);
            }
            return;
        }
        // Terrain/depth hits select a trackball or a camera-facing strafe plane.
        if self.pan_start.map(|v| v.0) != Some(start)
            && env.has_globe
            && c.position_cartographic()
                .is_some_and(|p| p.height < self.exaggerated(self.minimum_picking_terrain_height, env))
        {
            let Some(point) = pick(c, m.start_position, true) else {
                self.spin_looking = Some(start);
                self.look(c, m, ellipsoid.geodetic_surface_normal(c.position));
                return;
            };
            let Some(ray) = c.get_pick_ray(m.start_position) else {
                return;
            };
            let underground = env.camera_underground;
            let tangent = ellipsoid.geodetic_surface_normal(point).is_some_and(|n| ray.direction.dot(n).abs() < 0.05);
            if underground || tangent || c.position.magnitude() < point.magnitude() {
                let point = if underground {
                    let distance = ray.origin.distance(point);
                    let height = c.position_cartographic().map_or(0., |v| v.height);
                    ray.get_point(if distance > 10000. {
                        (env.globe_height.unwrap_or(0.) - height).abs()
                    } else {
                        distance
                    })
                } else {
                    point
                };
                self.strafe = Some((start, point, start));
                strafe_move(c, m.end_position, point);
                return;
            }
            let r = point.magnitude();
            ellipsoid = Ellipsoid::new(r, r, r);
        }
        let height = c.position_cartographic().map_or(0., |v| v.height);
        let mut pair = None;
        if !inertia && height < self.exaggerated(self.minimum_picking_terrain_height, env) && !env.has_globe {
            let point = if m.start_position != self.pan_mouse {
                pick(c, m.start_position, false)
            } else {
                Some(self.pan_world)
            };
            if let Some(point) = point {
                let position = c.position_wc();
                let direction = c.direction_wc();
                let right = c.right_wc();
                let up = c.up_wc();
                let to_center = point.subtract(position);
                let distance = direction.multiply_by_scalar(direction.dot(to_center)).magnitude();
                let dimensions = c.frustum.get_pixel_dimensions(
                    c.drawing_buffer_width,
                    c.drawing_buffer_height,
                    distance,
                    c.pixel_ratio,
                );
                let delta = m.end_position.subtract(m.start_position);
                if let Some(ray) = c.get_pick_ray(m.end_position) {
                    let normal = position.normalize();
                    let projection = ray.direction.subtract(right.multiply_by_scalar(ray.direction.dot(right)));
                    let angle = projection.angle_between(direction);
                    let forward =
                        if matches!(c.frustum, CameraFrustum::Perspective(_)) { angle.tan().max(0.1) } else { 1. };
                    let magnitude = -delta.y * dimensions.y * 2. / forward.sqrt() * (1. - direction.dot(normal).abs());
                    let end = point
                        .add(right.multiply_by_scalar(delta.x * dimensions.x))
                        .add(ray.direction.multiply_by_scalar(magnitude))
                        .add(up.multiply_by_scalar(-delta.y * (1. - up.dot(normal).abs()) * dimensions.y));
                    self.pan_world = end;
                    self.pan_mouse = m.end_position;
                    pair = Some((point, end));
                }
            }
        }
        let pair = pair.or_else(|| {
            Some((c.pick_ellipsoid(m.start_position, &ellipsoid)?, c.pick_ellipsoid(m.end_position, &ellipsoid)?))
        });
        let Some((p, q)) = pair else {
            self.spin_rotating = Some(start);
            self.rotate(c, m, None);
            return;
        };
        let pan_radius = p.magnitude();
        let p = c.world_to_camera_coordinates_point(p);
        let q = c.world_to_camera_coordinates_point(q);
        if let Some(basis) = c.constrained_axis {
            let b1 = basis.most_orthogonal_axis().cross(basis).normalize();
            let b2 = basis.cross(b1);
            let theta = |v: Cartesian3| (basis.dot(v) / v.magnitude()).clamp(-1., 1.).acos();
            let phi = |v: Cartesian3| {
                let v = v.subtract(basis.multiply_by_scalar(basis.dot(v))).normalize();
                let a = v.dot(b1).clamp(-1., 1.).acos();
                if v.dot(b2) < 0. {
                    TAU - a
                } else {
                    a
                }
            };
            let east = if basis.equals_epsilon(c.position, 0.01) { c.right } else { basis.cross(c.position) };
            let normal = basis.cross(east);
            let s0 = normal.dot(p.subtract(basis));
            let s1 = normal.dot(q.subtract(basis));
            let dt = if s0 > 0. && s1 > 0. {
                theta(q) - theta(p)
            } else if s0 > 0. && s1 <= 0. {
                if c.position.dot(basis) > 0. {
                    -theta(p) - theta(q)
                } else {
                    theta(p) + theta(q)
                }
            } else {
                theta(p) - theta(q)
            };
            c.rotate_right(Some(phi(p) - phi(q)));
            c.rotate_up(Some(dt));
        } else {
            let p = p.normalize();
            let q = q.normalize();
            let axis = p.cross(q);
            let dot = p.dot(q);
            if dot < 1. && axis.magnitude() > 1e-14 {
                c.rotate(axis, Some(dot.clamp(-1., 1.).acos()));
            }
        }
        if self.pan_start.map(|v| v.0) != Some(start) {
            self.pan_start = Some((start, Ellipsoid::new(pan_radius, pan_radius, pan_radius)));
        }
    }
    fn look(&self, c: &mut Camera, m: CameraMovement, axis: Option<Cartesian3>) {
        let ray_vector = |c: &mut Camera, p: Cartesian2| -> Option<Cartesian3> {
            let r = c.get_pick_ray(p)?;
            Some(if matches!(c.frustum, CameraFrustum::Orthographic(_)) {
                r.origin.add(c.direction).subtract(c.position).normalize()
            } else {
                r.direction
            })
        };
        let (Some(a), Some(b)) = (
            ray_vector(c, Cartesian2::new(m.start_position.x, 0.)),
            ray_vector(c, Cartesian2::new(m.end_position.x, 0.)),
        ) else {
            return;
        };
        let angle = if a.dot(b) < 1. { a.dot(b).clamp(-1., 1.).acos() } else { 0. }
            * if m.start_position.x > m.end_position.x { -1. } else { 1. };
        if let Some(axis) = axis {
            c.look(axis, Some(-angle));
        } else {
            c.look_left(Some(angle));
        }
        let (Some(a), Some(b)) = (
            ray_vector(c, Cartesian2::new(0., m.start_position.y)),
            ray_vector(c, Cartesian2::new(0., m.end_position.y)),
        ) else {
            return;
        };
        let mut angle = if a.dot(b) < 1. { a.dot(b).clamp(-1., 1.).acos() } else { 0. }
            * if m.start_position.y > m.end_position.y { -1. } else { 1. };
        if let Some(axis) = axis {
            let north = c.direction.equals_epsilon(axis, 0.01);
            let south = c.direction.equals_epsilon(axis.negate(), 0.01);
            if !north && !south {
                let to_axis = c.direction.dot(axis).clamp(-1., 1.).acos();
                if angle > 0. && angle > to_axis {
                    angle = to_axis - 1e-4;
                }
                let to_axis = c.direction.dot(axis.negate()).clamp(-1., 1.).acos();
                if angle < 0. && -angle > to_axis {
                    angle = -to_axis + 1e-4;
                }
                c.look(axis.cross(c.direction), Some(angle));
            } else if (north && angle < 0.) || (south && angle > 0.) {
                c.look(c.right, Some(-angle));
            }
        } else {
            c.look_up(Some(angle));
        }
    }
    fn tilt(&mut self, c: &mut Camera, start: Cartesian2, m: CameraMovement, env: ControllerEnvironment) {
        if self.tilt_center.map(|v| v.0) != Some(start) {
            self.tilt_looking = false;
            self.tilt_on_ellipsoid = false;
        }
        if self.tilt_looking {
            let axis = if c.mode() == SceneMode::Scene3D {
                c.ellipsoid.geodetic_surface_normal(c.position)
            } else {
                Some(Cartesian3::UNIT_Z)
            };
            self.look(c, m, axis);
            return;
        }
        let height = if c.mode() == SceneMode::Scene3D {
            c.position_cartographic().map_or(0., |p| p.height)
        } else {
            c.position.z.abs()
        };
        let terrain = if c.mode() == SceneMode::Scene3D {
            !self.tilt_on_ellipsoid && height <= self.exaggerated(self.minimum_collision_terrain_height, env)
        } else {
            self.on_map(c) && height <= self.exaggerated(self.minimum_picking_terrain_height, env)
        };
        if terrain {
            self.tilt_on_terrain(c, start, m, env);
            return;
        }
        self.tilt_on_ellipsoid = true;
        if c.transform() != Matrix4::IDENTITY {
            return;
        }
        let ellipsoid = c.ellipsoid;
        let center_pixel = Cartesian2::new(c.client_width * 0.5, c.client_height * 0.5);
        let center = if c.mode() == SceneMode::Scene3D {
            let height = c.position_cartographic().map_or(0., |p| p.height);
            if height - self.minimum_zoom_distance * 0.25 - 1. < 1e-3 && m.end_position.y < m.start_position.y {
                return;
            }
            let hit = c.pick_ellipsoid(center_pixel, &ellipsoid);
            if let Some(p) = hit {
                p
            } else {
                let Some(ray) = c.get_pick_ray(center_pixel) else {
                    return;
                };
                if height > self.exaggerated(self.minimum_track_ball_height, env) {
                    let Some(p) = intersection_tests::grazing_altitude_location(&ray, &ellipsoid) else {
                        return;
                    };
                    let Some(mut cart) = ellipsoid.cartesian_to_cartographic(p) else {
                        return;
                    };
                    cart.height = 0.;
                    ellipsoid.cartographic_to_cartesian(&cart)
                } else {
                    self.tilt_looking = true;
                    self.tilt_center = Some((start, Cartesian3::ZERO));
                    self.look(c, m, ellipsoid.geodetic_surface_normal(c.position));
                    return;
                }
            }
        } else {
            let Some(ray) = c.get_pick_ray(center_pixel) else {
                return;
            };
            let Some(p) = plane_hit(ray, 0.) else {
                self.tilt_looking = true;
                self.tilt_center = Some((start, Cartesian3::ZERO));
                self.look(c, m, Some(Cartesian3::UNIT_Z));
                return;
            };
            let cart = c.map_projection().unproject(Cartesian3::new(p.y, p.z, p.x));
            ellipsoid.cartographic_to_cartesian(&cart)
        };
        let transform = Transforms::east_north_up_to_fixed_frame(center, &ellipsoid);
        let old = c.transform();
        if c.set_transform(transform).is_ok() {
            self.rotate(c, m, Some(Cartesian3::UNIT_Z));
            let _ = c.set_transform(old);
        }
    }
    fn tilt_on_terrain(&mut self, c: &mut Camera, start: Cartesian2, m: CameraMovement, env: ControllerEnvironment) {
        if c.transform() != Matrix4::IDENTITY {
            return;
        }
        let mode = c.mode();
        let ellipsoid = c.ellipsoid;
        let underground = env.camera_underground && env.has_globe;
        let center = if let Some((s, p)) = self.tilt_center.filter(|(s, _)| *s == start) {
            let _ = s;
            p
        } else {
            let Some(ray) = c.get_pick_ray(start) else {
                return;
            };
            let picked = if mode == SceneMode::Scene3D
                || c.position.z < self.exaggerated(self.minimum_picking_terrain_height, env)
            {
                pick(c, start, env.has_globe)
            } else {
                None
            };
            let point = if let Some(p) = picked {
                Some(p)
            } else if mode == SceneMode::Scene3D {
                intersection_tests::ray_ellipsoid(&ray, &ellipsoid).map(|v| ray.get_point(v.start))
            } else {
                plane_hit(ray, 0.)
            };
            let Some(mut point) = point else {
                self.tilt_looking = true;
                self.tilt_center = Some((start, Cartesian3::ZERO));
                self.look(
                    c,
                    m,
                    if mode == SceneMode::Scene3D {
                        ellipsoid.geodetic_surface_normal(c.position)
                    } else {
                        Some(Cartesian3::UNIT_Z)
                    },
                );
                return;
            };
            if underground {
                let height = if mode == SceneMode::Scene3D {
                    c.position_cartographic().map_or(0., |v| v.height)
                } else {
                    c.position.z
                };
                let distance_from_surface = (env.globe_height.unwrap_or(0.) - height).abs();
                let mut distance = ray.origin.distance(point);
                let maximum = (distance_from_surface * 5.).clamp(2000., 10000.);
                if distance > maximum {
                    distance = distance.min(distance_from_surface / 5.).max(100.);
                }
                point = ray.get_point(distance);
            }
            self.tilt_center = Some((start, point));
            point
        };
        let Some(ray) = c.get_pick_ray(Cartesian2::new(c.client_width * 0.5, start.y)) else {
            return;
        };
        let (transform, vertical_transform, vertical_center) = if mode == SceneMode::Scene3D {
            let r = center.magnitude();
            let sphere = Ellipsoid::new(r, r, r);
            let Some(hit) = intersection_tests::ray_ellipsoid(&ray, &sphere) else {
                return;
            };
            let vertical = ray.get_point(if ray.origin.magnitude() > r { hit.start } else { hit.stop });
            (
                Transforms::east_north_up_to_fixed_frame(center, &ellipsoid),
                Transforms::east_north_up_to_fixed_frame(vertical, &sphere),
                vertical,
            )
        } else {
            let to_fixed = |p: Cartesian3| {
                let cart = c.map_projection().unproject(Cartesian3::new(p.y, p.z, p.x));
                ellipsoid.cartographic_to_cartesian(&cart)
            };
            let center = to_fixed(center);
            let vertical = plane_hit(ray, self.tilt_center.unwrap().1.x).map(to_fixed).unwrap_or(center);
            (
                Transforms::east_north_up_to_fixed_frame(center, &ellipsoid),
                Transforms::east_north_up_to_fixed_frame(vertical, &ellipsoid),
                vertical,
            )
        };
        let old_transform = c.transform();
        let mut axis = Some(Cartesian3::UNIT_Z);
        let dot = if mode == SceneMode::Scene3D {
            if c.set_transform(vertical_transform).is_err() {
                return;
            }
            let position = c.position_wc();
            let tangent = vertical_center.cross(position);
            c.right_wc().dot(tangent)
        } else {
            if c.set_transform(transform).is_err() {
                return;
            }
            let tangent = Cartesian3::UNIT_Z.cross(c.position.normalize());
            let dot = c.right.dot(tangent);
            self.rotate_axes(c, m, axis, false, true);
            let _ = c.set_transform(vertical_transform);
            dot
        };
        if dot < 0. {
            let delta = m.start_position.y - m.end_position.y;
            if (underground && delta < 0.) || (!underground && delta > 0.) {
                axis = None;
            }
            let old = c.constrained_axis;
            c.constrained_axis = None;
            self.rotate_axes(c, m, axis, true, false);
            c.constrained_axis = old;
        } else {
            self.rotate_axes(c, m, axis, true, false);
        }
        if mode == SceneMode::Scene3D {
            let _ = c.set_transform(transform);
            self.rotate_axes(c, m, axis, false, true);
        }
        if let Some(axis) = c.constrained_axis {
            let mut right = c.direction.cross(axis);
            if right.magnitude() > 1e-6 {
                if right.dot(c.right) < 0. {
                    right = right.negate();
                }
                c.up = right.cross(c.direction).normalize();
                c.right = c.direction.cross(c.up).normalize();
            }
        }
        let _ = c.set_transform(old_transform);
        let original = c.position_wc();
        if self.enable_collision_detection {
            self.adjust_height(c, env, true);
        }
        if c.position_wc() != original {
            let _ = c.set_transform(vertical_transform);
            let original = c.world_to_camera_coordinates_point(original);
            if c.position.magnitude_squared() > original.magnitude_squared() {
                c.position = c.position.normalize().multiply_by_scalar(original.magnitude());
            }
            let angle = original.angle_between(c.position);
            let axis = original.cross(c.position).normalize();
            let rotation = Matrix3::from_quaternion(&Quaternion::from_axis_angle(axis, angle));
            c.direction = rotation.multiply_by_vector(c.direction);
            c.up = rotation.multiply_by_vector(c.up);
            c.right = c.direction.cross(c.up);
            c.up = c.right.cross(c.direction);
            let _ = c.set_transform(old_transform);
        }
    }
    fn zoom(
        &mut self,
        c: &mut Camera,
        start: Cartesian2,
        m: CameraMovement,
        env: ControllerEnvironment,
        inertia: bool,
    ) {
        let mode = c.mode();
        let ellipsoid = self.ellipsoid(c);
        let has_globe = env.has_globe && c.transform() == Matrix4::IDENTITY;
        let underground = env.camera_underground && has_globe;
        let height = if mode == SceneMode::Scene3D {
            ellipsoid.cartesian_to_cartographic(c.position).map_or(0., |v| v.height)
        } else {
            c.position.z
        };
        let percentage =
            if mode == SceneMode::Scene3D { c.position.normalize().dot(c.direction).abs().clamp(0.25, 1.) } else { 1. };
        let window = if underground { start } else { Cartesian2::new(c.client_width * 0.5, c.client_height * 0.5) };
        let ray = c.get_pick_ray(window);
        let threshold = self.exaggerated(self.minimum_picking_terrain_height, env);
        let need_pick = if inertia && mode == SceneMode::Scene3D {
            self.pre_intersection_distance.abs() < self.minimum_picking_terrain_distance_with_inertia
        } else {
            height < threshold
        };
        let intersection = if need_pick && mode != SceneMode::Scene2D { pick(c, window, has_globe) } else { None };
        let mut measure = intersection.and_then(|p| ray.map(|r| r.origin.distance(p)));
        if let Some(d) = measure {
            self.pre_intersection_distance = d;
        }
        if underground {
            if let Some(ray) = ray {
                let distance = (env.globe_height.unwrap_or(0.) - height).abs()
                    * ray.origin.normalize().dot(ray.direction).abs().max(0.5)
                    * 2.;
                measure = Some(measure.map_or(distance, |v| v.min(distance)));
            }
        }
        let measure = if mode == SceneMode::Scene2D {
            c.get_magnitude().unwrap_or(0.)
        } else {
            measure.unwrap_or_else(|| {
                if mode == SceneMode::ColumbusView {
                    ray.map_or(0., |r| -r.origin.x / r.direction.x)
                } else {
                    height
                }
            })
        };
        let diff = m.end_position.y - m.start_position.y;
        let min = if diff > 0. { self.minimum_zoom_distance * percentage } else { 0. };
        let rate = (self.zoom_factor * (measure - min)).clamp(20., 5906376272000.);
        let mut distance = rate * (diff / c.client_height).min(self.maximum_movement_ratio);
        if self.enable_collision_detection || self.minimum_zoom_distance == 0. || !has_globe {
            if distance > 0. && (measure - min).abs() < 1. {
                return;
            }
            if distance < 0. && (measure - self.maximum_zoom_distance).abs() < 1. {
                return;
            }
            if measure - distance < min {
                distance = measure - min - 1.;
            } else if measure - distance > self.maximum_zoom_distance {
                distance = measure - self.maximum_zoom_distance;
            }
        }
        let orientation = c.heading_pitch_roll();
        let same_start = inertia || self.zoom_start == Some(start);
        if !same_start {
            self.zoom_start = Some(start);
            self.zoom_world = if has_globe {
                if mode == SceneMode::Scene2D {
                    c.get_pick_ray(start).map(|r| Cartesian3::new(r.origin.y, r.origin.z, r.origin.x))
                } else {
                    pick(c, start, has_globe)
                }
            } else {
                None
            };
            self.zooming_vector = false;
            self.rotating_zoom = false;
            self.zooming_underground = underground;
        }
        let Some(target) = self.zoom_world else {
            c.zoom_in(Some(distance));
            return;
        };
        let mut vector = mode == SceneMode::ColumbusView;
        let cartographic_height = c.position_cartographic().map_or(0., |v| v.height);
        if cartographic_height < 2e6 {
            self.rotating_zoom = true;
        }
        if !same_start || self.rotating_zoom {
            if mode == SceneMode::Scene2D {
                let max = c.map_projection().project(&Cartographic::new(PI, PI / 2., 0.)).x;
                if target != c.position && cartographic_height < max * 2. {
                    let saved_x = c.position.x;
                    let delta = target.subtract(c.position);
                    let amount = delta.magnitude() * distance / c.get_magnitude().unwrap_or(1.);
                    c.move_direction(delta.normalize(), amount);
                    if c.position.x * saved_x < 0. {
                        self.zoom_world =
                            c.get_pick_ray(start).map(|r| Cartesian3::new(r.origin.y, r.origin.z, r.origin.x));
                    }
                }
            } else if mode == SceneMode::Scene3D {
                let normal = c.position.normalize();
                if underground
                    || self.zooming_underground
                    || (cartographic_height < 3000. && c.direction.dot(normal).abs() < 0.6)
                {
                    vector = true;
                } else {
                    let center = pick(c, Cartesian2::new(c.client_width * 0.5, c.client_height * 0.5), has_globe);
                    if let Some(center) = center {
                        if cartographic_height < 1e6 {
                            if c.direction.dot(normal) >= -0.5 {
                                vector = true;
                            } else {
                                if target.normalize().dot(normal) < 0. {
                                    return;
                                }
                                let position = c.position;
                                let mut center = position.add(c.direction.multiply_by_scalar(1000.));
                                let to_target = target.subtract(position);
                                let target_normal = to_target.normalize();
                                let alpha_dot = normal.dot(target_normal);
                                if alpha_dot >= 0. {
                                    self.zoom_start = None;
                                    return;
                                }
                                let alpha = (-alpha_dot).clamp(-1., 1.).acos();
                                let remaining = position.magnitude() - distance;
                                let gamma =
                                    (to_target.magnitude() / target.magnitude() * alpha.sin()).clamp(-1., 1.).asin();
                                let delta = (remaining / target.magnitude() * alpha.sin()).clamp(-1., 1.).asin();
                                let beta = gamma - delta + alpha;
                                let right = target_normal.cross(normal).normalize();
                                let forward = normal.cross(right).normalize();
                                center = center.normalize().multiply_by_scalar(center.magnitude() - distance);
                                c.position = normal.multiply_by_scalar(remaining).add(
                                    normal
                                        .multiply_by_scalar(beta.cos() - 1.)
                                        .add(forward.multiply_by_scalar(beta.sin()))
                                        .multiply_by_scalar(remaining),
                                );
                                let up = center.normalize();
                                let forward = up.cross(right).normalize();
                                center = center.add(
                                    up.multiply_by_scalar(beta.cos() - 1.)
                                        .add(forward.multiply_by_scalar(beta.sin()))
                                        .multiply_by_scalar(center.magnitude()),
                                );
                                c.direction = center.subtract(c.position).normalize();
                                c.right = c.direction.cross(c.up);
                                c.up = c.right.cross(c.direction);
                                preserve_orientation(c, orientation);
                                return;
                            }
                        } else {
                            let picked_normal = target.normalize();
                            let dot = picked_normal.dot(center.normalize());
                            if dot > 0. && dot < 1. {
                                let angle = dot.clamp(-1., 1.).acos();
                                let axis = picked_normal.cross(center.normalize());
                                let denominator = if angle.abs() > 20_f64.to_radians() {
                                    cartographic_height * 0.75
                                } else {
                                    cartographic_height - distance
                                };
                                c.rotate(axis, Some(angle * distance / denominator));
                            }
                        }
                    } else {
                        vector = true;
                    }
                }
            }
            self.rotating_zoom = !vector;
        }
        if (!same_start && vector) || self.zooming_vector {
            let pixel = if mode == SceneMode::Scene3D && self.zoom_start == Some(start) {
                world_pixel(c, target).unwrap_or(start)
            } else {
                start
            };
            if let Some(ray) = c.get_pick_ray(pixel) {
                let direction = if matches!(mode, SceneMode::ColumbusView | SceneMode::Scene2D) {
                    Cartesian3::new(ray.direction.y, ray.direction.z, ray.direction.x)
                } else {
                    ray.direction
                };
                c.move_direction(direction, distance);
                self.zooming_vector = true;
            }
        } else {
            c.zoom_in(Some(distance));
        }
        if !underground {
            preserve_orientation(c, orientation);
        }
    }
    fn exaggerated(&self, height: f64, env: ControllerEnvironment) -> f64 {
        (height - env.vertical_exaggeration_relative_height) * env.vertical_exaggeration
            + env.vertical_exaggeration_relative_height
    }
    fn adjust_height(&mut self, c: &mut Camera, env: ControllerEnvironment, changed: bool) {
        self.adjusted_height_for_terrain = true;
        if matches!(c.mode(), SceneMode::Scene2D | SceneMode::Morphing) {
            return;
        }
        let Some(globe_height) = env.globe_height else {
            return;
        };
        let threshold = (self.minimum_collision_terrain_height - env.vertical_exaggeration_relative_height)
            * env.vertical_exaggeration
            + env.vertical_exaggeration_relative_height;
        let transform = c.transform();
        let mag = c.position.magnitude();
        if transform != Matrix4::IDENTITY {
            let _ = c.set_transform(Matrix4::IDENTITY);
        }
        let cart = if c.mode() == SceneMode::Scene3D {
            c.ellipsoid.cartesian_to_cartographic(c.position)
        } else {
            Some(c.map_projection().unproject(c.position))
        };
        let mut adjusted = false;
        if let Some(mut cart) = cart {
            if cart.height < threshold {
                let difference = globe_height - self.last_globe_height;
                let stable = changed || (difference / self.last_globe_height).abs() <= 0.1;
                if stable && cart.height < globe_height + self.minimum_zoom_distance {
                    cart.height = globe_height + self.minimum_zoom_distance;
                    c.position = if c.mode() == SceneMode::Scene3D {
                        c.ellipsoid.cartographic_to_cartesian(&cart)
                    } else {
                        c.map_projection().project(&cart)
                    };
                    adjusted = true;
                }
                self.last_globe_height = if stable { globe_height } else { self.last_globe_height + difference * 0.1 };
            }
        }
        if transform != Matrix4::IDENTITY {
            let _ = c.set_transform(transform);
            if adjusted {
                c.direction = c.position.normalize().negate();
                c.position = c.position.normalize().multiply_by_scalar(mag.max(self.minimum_zoom_distance));
                c.right = c.direction.cross(c.up);
                c.up = c.right.cross(c.direction);
            }
        }
    }
    pub fn is_destroyed(&self) -> bool {
        self.destroyed
    }
    pub fn destroy(&mut self) {
        self.destroyed = true;
        self.aggregator.destroy();
        self.correction = None;
    }
    fn alive(&self) -> Result<(), RuntimeError> {
        if self.destroyed {
            Err(RuntimeError::new("This object was destroyed, i.e., destroy() was called."))
        } else {
            Ok(())
        }
    }
}
fn plane_hit(ray: Ray, height: f64) -> Option<Cartesian3> {
    if ray.direction.x.abs() < 1e-15 {
        return None;
    }
    let t = (height - ray.origin.x) / ray.direction.x;
    (t >= 0.).then(|| ray.origin.add(ray.direction.multiply_by_scalar(t)))
}
fn pick(c: &mut Camera, p: Cartesian2, has_globe: bool) -> Option<Cartesian3> {
    let ray = c.get_pick_ray(p)?;
    let camera_position = c.position_wc();
    let surface = c.surface.as_ref()?;
    let globe = if has_globe { surface.pick_globe(&ray) } else { None };
    let depth = surface.pick_depth(p);
    match (globe, depth) {
        (Some(a), Some(b)) => Some(if camera_position.distance(a) < camera_position.distance(b) { a } else { b }),
        (a, b) => a.or(b),
    }
}

fn preserve_orientation(c: &mut Camera, orientation: Option<HeadingPitchRoll>) {
    if let Some(orientation) = orientation {
        let _ = c.set_view(crate::scene::CameraView {
            orientation: Some(crate::scene::CameraOrientation::HeadingPitchRoll(orientation)),
            ..Default::default()
        });
    }
}
fn world_pixel(c: &mut Camera, p: Cartesian3) -> Option<Cartesian2> {
    let view = c.view_matrix().multiply_by_vector(Cartesian4::new(p.x, p.y, p.z, 1.));
    let clip = c.frustum.uniform_frustum().parameters().0.multiply_by_vector(view);
    (clip.w > 0.).then(|| {
        Cartesian2::new((clip.x / clip.w + 1.) * c.client_width * 0.5, (1. - clip.y / clip.w) * c.client_height * 0.5)
    })
}

fn strafe_move(c: &mut Camera, end: Cartesian2, point: Cartesian3) {
    let Some(ray) = c.get_pick_ray(end) else {
        return;
    };
    let normal = if c.mode() == SceneMode::ColumbusView {
        Cartesian3::new(c.direction.z, c.direction.x, c.direction.y)
    } else {
        c.direction
    };
    let dot = normal.dot(ray.direction);
    if dot.abs() < 1e-15 {
        return;
    }
    let t = normal.dot(point.subtract(ray.origin)) / dot;
    if t < 0. {
        return;
    }
    let direction = point.subtract(ray.get_point(t));
    let direction = if c.mode() == SceneMode::ColumbusView {
        Cartesian3::new(direction.y, direction.z, direction.x)
    } else {
        direction
    };
    c.position = c.position.add(direction);
}
