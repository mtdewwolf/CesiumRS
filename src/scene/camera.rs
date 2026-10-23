//! CesiumJS `Camera`: reference frames, navigation, view fitting and picking.
//!
//! The camera owns its view state. Canvas dimensions and projection replace
//! the reference to a JavaScript Scene; optional surface queries are supplied
//! by the host. Getters take `&mut self` because, as in Cesium, reading derived
//! state also normalizes a changed orientation. Flight time and controller
//! input state are explicit; see docs/scene.md.

use super::camera_flight_path::{
    CameraEasing, CameraFlightCallback, CameraFlightPath, CameraFlightPathOptions, CurrentFlight, ScheduledFlight,
};
use super::{MapMode2D, SceneMode};
use crate::core::js_math::JsMath;
use crate::core::math::*;
use crate::core::{intersection_tests, *};
use crate::renderer::{UniformCamera, UniformFrustum};
use std::rc::Rc;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CameraError(pub &'static str);
impl std::fmt::Display for CameraError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.0)
    }
}
impl std::error::Error for CameraError {}

/// Frustums accepted by Camera. In 2D use `OrthographicOffCenter`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum CameraFrustum {
    Perspective(PerspectiveFrustum),
    Orthographic(OrthographicFrustum),
    OrthographicOffCenter(OrthographicOffCenterFrustum),
}
impl CameraFrustum {
    pub fn uniform_frustum(self) -> UniformFrustum {
        match self {
            Self::Perspective(f) => UniformFrustum::Perspective(f),
            Self::Orthographic(f) => UniformFrustum::Orthographic(f),
            Self::OrthographicOffCenter(f) => UniformFrustum::OrthographicOffCenter(f),
        }
    }
    pub fn compute_culling_volume(&self, position: Cartesian3, direction: Cartesian3, up: Cartesian3) -> CullingVolume {
        match self {
            Self::Perspective(f) => f.compute_culling_volume(position, direction, up),
            Self::Orthographic(f) => f.compute_culling_volume(position, direction, up),
            Self::OrthographicOffCenter(f) => f.compute_culling_volume(position, direction, up),
        }
    }
    pub fn get_pixel_dimensions(&self, width: f64, height: f64, distance: f64, pixel_ratio: f64) -> Cartesian2 {
        match self {
            Self::Perspective(f) => f.get_pixel_dimensions(width, height, distance, pixel_ratio),
            Self::Orthographic(f) => f.get_pixel_dimensions(width, height, distance, pixel_ratio),
            Self::OrthographicOffCenter(f) => f.get_pixel_dimensions(width, height, distance, pixel_ratio),
        }
    }
    pub(crate) fn orthographic(&self) -> Option<OrthographicOffCenterFrustum> {
        match self {
            Self::Orthographic(f) => Some(f.off_center_frustum()),
            Self::OrthographicOffCenter(f) => Some(*f),
            _ => None,
        }
    }
}

/// Optional globe/depth queries used when fitting an orthographic frustum.
/// The camera selects the closest successful hit, matching Cesium.
pub trait CameraSurface {
    fn pick_globe(&self, ray: &Ray) -> Option<Cartesian3>;
    fn pick_depth(&self, window_position: Cartesian2) -> Option<Cartesian3>;
}

pub struct CameraOptions {
    pub drawing_buffer_width: f64,
    pub drawing_buffer_height: f64,
    pub client_width: f64,
    pub client_height: f64,
    pub pixel_ratio: f64,
    pub map_projection: Rc<dyn MapProjection>,
    /// Scene ellipsoid used by lookAt and sphere flights. Projection math
    /// uses the projection's ellipsoid, which may be different.
    pub ellipsoid: Ellipsoid,
    pub map_mode_2d: MapMode2D,
    pub minimum_zoom_distance: f64,
    pub maximum_zoom_distance: f64,
    pub surface: Option<Rc<dyn CameraSurface>>,
}
impl CameraOptions {
    /// Canvas and drawing buffer initially have the same dimensions.
    pub fn new(width: f64, height: f64) -> Self {
        Self {
            drawing_buffer_width: width,
            drawing_buffer_height: height,
            client_width: width,
            client_height: height,
            pixel_ratio: 1.0,
            map_projection: Rc::new(GeographicProjection::default()),
            ellipsoid: Ellipsoid::WGS84,
            map_mode_2d: MapMode2D::InfiniteScroll,
            minimum_zoom_distance: 1.0,
            maximum_zoom_distance: f64::INFINITY,
            surface: None,
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub enum CameraDestination {
    Cartesian(Cartesian3),
    Rectangle(Rectangle),
}
#[derive(Clone, Copy, Debug)]
pub enum CameraOrientation {
    HeadingPitchRoll(HeadingPitchRoll),
    DirectionUp { direction: Cartesian3, up: Cartesian3 },
}
#[derive(Clone, Copy, Debug)]
pub struct CameraView {
    pub destination: Option<CameraDestination>,
    pub orientation: Option<CameraOrientation>,
    pub convert: bool,
    pub end_transform: Option<Matrix4>,
}
impl Default for CameraView {
    fn default() -> Self {
        Self { destination: None, orientation: None, convert: true, end_transform: None }
    }
}
#[derive(Clone, Copy, Debug)]
pub enum CameraOffset {
    Cartesian(Cartesian3),
    HeadingPitchRange(HeadingPitchRange),
}

#[derive(Default)]
pub struct CameraFlightOptions {
    pub view: CameraView,
    pub duration: Option<f64>,
    pub maximum_height: Option<f64>,
    pub pitch_adjust_height: Option<f64>,
    pub fly_over_longitude: Option<f64>,
    pub fly_over_longitude_weight: Option<f64>,
    pub easing_function: Option<CameraEasing>,
    pub complete: Option<CameraFlightCallback>,
    pub cancel: Option<CameraFlightCallback>,
}
impl CameraFlightOptions {
    pub fn new(destination: CameraDestination) -> Self {
        Self { view: CameraView { destination: Some(destination), ..CameraView::default() }, ..Self::default() }
    }
}

/// The position-correction tween returned by `create_correct_position_tween`.
/// Its update input is the eased fraction, as in Cesium's descriptor.
#[derive(Clone, Copy, Debug)]
pub struct CameraCorrectionTween {
    pub duration: f64,
    pub easing_function: EasingFunction,
    start: Cartesian3,
    stop: Cartesian3,
}
impl CameraCorrectionTween {
    pub fn update(&self, camera: &mut Camera, time: f64) {
        let position = self.start.lerp(self.stop, time);
        camera.position = camera.world_to_camera_coordinates_point(position);
    }
}

#[derive(Clone, Copy)]
struct Members {
    position: Cartesian3,
    direction: Cartesian3,
    up: Cartesian3,
    right: Cartesian3,
    position_wc: Cartesian3,
    direction_wc: Cartesian3,
    up_wc: Cartesian3,
    right_wc: Cartesian3,
    cartographic: Option<Cartographic>,
    inverse_transform: Matrix4,
    actual_transform: Matrix4,
    actual_inverse_transform: Matrix4,
    view: Matrix4,
    inverse_view: Matrix4,
}

pub struct Camera {
    pub position: Cartesian3,
    pub direction: Cartesian3,
    pub up: Cartesian3,
    pub right: Cartesian3,
    pub frustum: CameraFrustum,
    pub default_move_amount: f64,
    pub default_look_amount: f64,
    pub default_rotate_amount: f64,
    pub default_zoom_amount: f64,
    pub constrained_axis: Option<Cartesian3>,
    pub maximum_zoom_factor: f64,
    pub percentage_changed: f64,
    pub position_wc_delta_magnitude: f64,
    pub position_wc_delta_magnitude_last_frame: f64,
    pub time_since_moved: f64,
    pub move_start: Event<()>,
    pub move_end: Event<()>,
    pub changed: Event<f64>,
    pub drawing_buffer_width: f64,
    pub drawing_buffer_height: f64,
    pub client_width: f64,
    pub client_height: f64,
    pub pixel_ratio: f64,
    pub map_mode_2d: MapMode2D,
    pub minimum_zoom_distance: f64,
    pub maximum_zoom_distance: f64,
    pub surface: Option<Rc<dyn CameraSurface>>,
    pub ellipsoid: Ellipsoid,
    /// The Scene/controller reads this state to gate user input during flight.
    pub enable_inputs: bool,
    /// The Scene transitioner handles this event for `fly_home` while morphing.
    pub morph_completion_requested: Event<()>,
    pub preload_flight_camera: Option<Box<Camera>>,
    pub preload_flight_culling_volume: Option<CullingVolume>,
    projection: Rc<dyn MapProjection>,
    mode: SceneMode,
    mode_changed: bool,
    transform: Matrix4,
    transform_changed: bool,
    max_coord: Cartesian3,
    members: Members,
    old_position_wc: Option<Cartesian3>,
    last_moved_timestamp: f64,
    changed_heading: Option<f64>,
    changed_roll: Option<f64>,
    changed_position: Option<Cartesian3>,
    changed_direction: Option<Cartesian3>,
    changed_frustum: Option<OrthographicOffCenterFrustum>,
    current_flight: Option<CurrentFlight>,
    scheduled_flights: Vec<ScheduledFlight>,
    next_flight_id: u64,
}

impl Camera {
    pub const TRANSFORM_2D: Matrix4 = Matrix4::new(0., 0., 1., 0., 1., 0., 0., 0., 0., 1., 0., 0., 0., 0., 0., 1.);
    pub const TRANSFORM_2D_INVERSE: Matrix4 =
        Matrix4::new(0., 1., 0., 0., 0., 0., 1., 0., 1., 0., 0., 0., 0., 0., 0., 1.);
    pub const DEFAULT_VIEW_FACTOR: f64 = 0.5;
    pub const DEFAULT_OFFSET: HeadingPitchRange = HeadingPitchRange { heading: 0.0, pitch: -PI_OVER_FOUR, range: 0.0 };

    pub fn default_view_rectangle() -> Rectangle {
        Rectangle::from_degrees(-95.0, -20.0, -70.0, 90.0)
    }

    pub fn new(options: CameraOptions) -> Self {
        let projection = options.map_projection;
        let max_coord = projection.project(&Cartographic::new(PI, PI_OVER_TWO, 0.0));
        let mut camera = Self {
            position: Cartesian3::ZERO,
            direction: Cartesian3::ZERO,
            up: Cartesian3::ZERO,
            right: Cartesian3::ZERO,
            frustum: CameraFrustum::Perspective(PerspectiveFrustum::new(
                to_radians(60.0),
                options.drawing_buffer_width / options.drawing_buffer_height,
            )),
            default_move_amount: 100_000.0,
            default_look_amount: PI / 60.0,
            default_rotate_amount: PI / 3600.0,
            default_zoom_amount: 100_000.0,
            constrained_axis: None,
            maximum_zoom_factor: 1.5,
            percentage_changed: 0.5,
            position_wc_delta_magnitude: 0.0,
            position_wc_delta_magnitude_last_frame: 0.0,
            time_since_moved: 0.0,
            move_start: Event::new(),
            move_end: Event::new(),
            changed: Event::new(),
            drawing_buffer_width: options.drawing_buffer_width,
            drawing_buffer_height: options.drawing_buffer_height,
            client_width: options.client_width,
            client_height: options.client_height,
            pixel_ratio: options.pixel_ratio,
            map_mode_2d: options.map_mode_2d,
            minimum_zoom_distance: options.minimum_zoom_distance,
            maximum_zoom_distance: options.maximum_zoom_distance,
            surface: options.surface,
            ellipsoid: options.ellipsoid,
            projection,
            mode: SceneMode::Scene3D,
            mode_changed: true,
            transform: Matrix4::IDENTITY,
            transform_changed: false,
            max_coord,
            members: Members {
                position: Cartesian3::ZERO,
                direction: Cartesian3::ZERO,
                up: Cartesian3::ZERO,
                right: Cartesian3::ZERO,
                position_wc: Cartesian3::ZERO,
                direction_wc: Cartesian3::ZERO,
                up_wc: Cartesian3::ZERO,
                right_wc: Cartesian3::ZERO,
                cartographic: Some(Cartographic::default()),
                inverse_transform: Matrix4::IDENTITY,
                actual_transform: Matrix4::IDENTITY,
                actual_inverse_transform: Matrix4::IDENTITY,
                view: Matrix4::ZERO,
                inverse_view: Matrix4::ZERO,
            },
            old_position_wc: None,
            last_moved_timestamp: 0.0,
            changed_heading: None,
            changed_roll: None,
            changed_position: None,
            changed_direction: None,
            changed_frustum: None,
            enable_inputs: true,
            morph_completion_requested: Event::new(),
            preload_flight_camera: None,
            preload_flight_culling_volume: None,
            current_flight: None,
            scheduled_flights: Vec::new(),
            next_flight_id: 0,
        };
        let (position, direction, up, right) = camera.rectangle_position_3d(Self::default_view_rectangle());
        let magnitude = position.magnitude();
        camera.position = position.normalize().multiply_by_scalar(magnitude + magnitude * Self::DEFAULT_VIEW_FACTOR);
        camera.direction = direction;
        camera.up = up;
        camera.right = right;
        camera
    }

    pub fn mode(&self) -> SceneMode {
        self.mode
    }
    pub fn map_projection(&self) -> &dyn MapProjection {
        self.projection.as_ref()
    }
    pub fn transform(&self) -> Matrix4 {
        self.transform
    }
    pub fn inverse_transform(&mut self) -> Matrix4 {
        self.update_members();
        self.members.inverse_transform
    }
    pub fn view_matrix(&mut self) -> Matrix4 {
        self.update_members();
        self.members.view
    }
    pub fn inverse_view_matrix(&mut self) -> Matrix4 {
        self.update_members();
        self.members.inverse_view
    }
    pub fn position_wc(&mut self) -> Cartesian3 {
        self.update_members();
        self.members.position_wc
    }
    pub fn direction_wc(&mut self) -> Cartesian3 {
        self.update_members();
        self.members.direction_wc
    }
    pub fn up_wc(&mut self) -> Cartesian3 {
        self.update_members();
        self.members.up_wc
    }
    pub fn right_wc(&mut self) -> Cartesian3 {
        self.update_members();
        self.members.right_wc
    }
    pub fn position_cartographic(&mut self) -> Option<Cartographic> {
        self.update_members();
        self.members.cartographic
    }

    fn validate_transform(&self, transform: Matrix4, mode: SceneMode) -> Result<(), CameraError> {
        if matches!(mode, SceneMode::Scene2D | SceneMode::ColumbusView)
            && transform != Matrix4::IDENTITY
            && self.projection.ellipsoid().cartesian_to_cartographic(transform.get_translation()).is_none()
        {
            return Err(CameraError("map transform must have a cartographic origin"));
        }
        Ok(())
    }

    fn actual_transform(&self) -> Matrix4 {
        if !matches!(self.mode, SceneMode::Scene2D | SceneMode::ColumbusView) {
            return self.transform;
        }
        if self.transform == Matrix4::IDENTITY {
            return Self::TRANSFORM_2D;
        }
        if self.mode == SceneMode::ColumbusView {
            return Transforms::basis_to_2d(self.projection.as_ref(), &self.transform);
        }
        let origin = self.transform.get_translation();
        let project = |p| {
            let cartographic =
                self.projection.ellipsoid().cartesian_to_cartographic(p).expect("validated map transform origin");
            let p = self.projection.project(&cartographic);
            Cartesian3::new(p.z, p.x, p.y)
        };
        let new_origin = project(origin);
        let z = Cartesian3::UNIT_X;
        let mut x = project(origin + self.transform.multiply_by_point_as_vector(Cartesian3::UNIT_X)) - new_origin;
        x.x = 0.0;
        let mut y;
        if x.magnitude_squared() > EPSILON10 {
            y = z.cross(x);
        } else {
            y = project(origin + self.transform.multiply_by_point_as_vector(Cartesian3::UNIT_Y)) - new_origin;
            y.x = 0.0;
            if y.magnitude_squared() < EPSILON10 {
                y = Cartesian3::UNIT_Z;
            }
        }
        x = y.cross(z).normalize();
        y = z.cross(x).normalize();
        Matrix4::from_rotation_translation(&Matrix3::new(x.x, y.x, z.x, x.y, y.y, z.y, x.z, y.z, z.z), new_origin)
    }

    fn update_members(&mut self) {
        let height = self.frustum.orthographic().map_or(0.0, |f| f.right - f.left);
        let height_changed =
            self.mode == SceneMode::Scene2D && self.members.cartographic.map(|p| p.height) != Some(height);
        let position_changed = self.members.position != self.position || height_changed;
        let direction_changed = self.members.direction != self.direction;
        let up_changed = self.members.up != self.up;
        let right_changed = self.members.right != self.right;
        if position_changed {
            self.members.position = self.position;
        }
        if direction_changed {
            self.direction = self.direction.normalize();
            self.members.direction = self.direction;
        }
        if up_changed {
            self.up = self.up.normalize();
            self.members.up = self.up;
        }
        if right_changed {
            self.right = self.right.normalize();
            self.members.right = self.right;
        }
        let transform_changed = self.transform_changed || self.mode_changed;
        if transform_changed {
            self.members.inverse_transform = self.transform.inverse_transformation();
            self.members.actual_transform = self.actual_transform();
            self.members.actual_inverse_transform = self.members.actual_transform.inverse_transformation();
            self.transform_changed = false;
            self.mode_changed = false;
        }
        if position_changed || transform_changed {
            self.members.position_wc = self.members.actual_transform.multiply_by_point(self.position);
            self.members.cartographic = if matches!(self.mode, SceneMode::Scene3D | SceneMode::Morphing) {
                self.projection.ellipsoid().cartesian_to_cartographic(self.members.position_wc)
            } else {
                let p = self.members.position_wc;
                Some(self.projection.unproject(Cartesian3::new(
                    p.y,
                    p.z,
                    if self.mode == SceneMode::Scene2D { height } else { p.x },
                )))
            };
        }
        if direction_changed || up_changed || right_changed {
            let det = self.direction.dot(self.up.cross(self.right));
            if (1.0 - det).abs() > EPSILON2 {
                let scalar = self.up.dot(self.direction) * (1.0 / self.up.magnitude_squared());
                self.up = (self.up - self.direction.multiply_by_scalar(scalar)).normalize();
                self.right = self.direction.cross(self.up);
                self.members.up = self.up;
                self.members.right = self.right;
            }
        }
        if direction_changed || transform_changed {
            self.members.direction_wc =
                self.members.actual_transform.multiply_by_point_as_vector(self.direction).normalize();
        }
        if up_changed || transform_changed {
            self.members.up_wc = self.members.actual_transform.multiply_by_point_as_vector(self.up).normalize();
        }
        if right_changed || transform_changed {
            self.members.right_wc = self.members.actual_transform.multiply_by_point_as_vector(self.right).normalize();
        }
        if position_changed || direction_changed || up_changed || right_changed || transform_changed {
            self.members.view = Matrix4::compute_view(self.position, self.direction, self.up, self.right)
                .multiply(&self.members.actual_inverse_transform);
            self.members.inverse_view = self.members.view.inverse_transformation();
        }
    }

    /// Changes mode without converting position/orientation. The Scene transition
    /// owns that conversion. Frustum requirements match Cesium's debug checks.
    pub fn update(&mut self, mode: SceneMode) -> Result<(), CameraError> {
        match (mode, self.frustum) {
            (SceneMode::Scene2D, CameraFrustum::OrthographicOffCenter(_)) => (),
            (SceneMode::Scene2D, _) => return Err(CameraError("An OrthographicOffCenterFrustum is required in 2D.")),
            (SceneMode::Scene3D | SceneMode::ColumbusView, CameraFrustum::OrthographicOffCenter(_)) => {
                return Err(CameraError(
                    "A PerspectiveFrustum or OrthographicFrustum is required in 3D and Columbus view",
                ))
            }
            _ => (),
        }
        self.validate_transform(self.transform, mode)?;
        if self.mode != mode {
            self.mode = mode;
            self.mode_changed = mode != SceneMode::Morphing;
        }
        if mode == SceneMode::Scene2D {
            self.clamp_move_2d();
        }
        Ok(())
    }

    /// Changes reference frame while preserving world position and orientation.
    pub fn set_transform(&mut self, transform: Matrix4) -> Result<(), CameraError> {
        self.validate_transform(transform, self.mode)?;
        self.update_members();
        let p = self.members.position_wc;
        let d = self.members.direction_wc;
        let u = self.members.up_wc;
        self.transform = transform;
        self.transform_changed = true;
        self.update_members();
        let inverse = self.members.actual_inverse_transform;
        self.position = inverse.multiply_by_point(p);
        self.direction = inverse.multiply_by_point_as_vector(d);
        self.up = inverse.multiply_by_point_as_vector(u);
        self.right = self.direction.cross(self.up);
        self.update_members();
        Ok(())
    }

    pub fn world_to_camera_coordinates(&mut self, p: Cartesian4) -> Cartesian4 {
        self.update_members();
        self.members.actual_inverse_transform.multiply_by_vector(p)
    }
    pub fn world_to_camera_coordinates_point(&mut self, p: Cartesian3) -> Cartesian3 {
        self.update_members();
        self.members.actual_inverse_transform.multiply_by_point(p)
    }
    pub fn world_to_camera_coordinates_vector(&mut self, p: Cartesian3) -> Cartesian3 {
        self.update_members();
        self.members.actual_inverse_transform.multiply_by_point_as_vector(p)
    }
    pub fn camera_to_world_coordinates(&mut self, p: Cartesian4) -> Cartesian4 {
        self.update_members();
        self.members.actual_transform.multiply_by_vector(p)
    }
    pub fn camera_to_world_coordinates_point(&mut self, p: Cartesian3) -> Cartesian3 {
        self.update_members();
        self.members.actual_transform.multiply_by_point(p)
    }
    pub fn camera_to_world_coordinates_vector(&mut self, p: Cartesian3) -> Cartesian3 {
        self.update_members();
        self.members.actual_transform.multiply_by_point_as_vector(p)
    }

    fn heading_for(direction: Cartesian3, up: Cartesian3) -> f64 {
        let p = if equals_epsilon(direction.z.abs(), 1.0, EPSILON3, EPSILON3) { up } else { direction };
        TWO_PI - zero_to_two_pi(p.y.js_atan2(p.x) - PI_OVER_TWO)
    }
    fn pitch_for(direction: Cartesian3) -> f64 {
        PI_OVER_TWO - acos_clamped(direction.z)
    }
    fn roll_for(direction: Cartesian3, up: Cartesian3, right: Cartesian3) -> f64 {
        if equals_epsilon(direction.z.abs(), 1.0, EPSILON3, EPSILON3) {
            0.0
        } else {
            zero_to_two_pi((-right.z).js_atan2(up.z) + TWO_PI)
        }
    }
    pub fn heading_pitch_roll(&mut self) -> Option<HeadingPitchRoll> {
        if self.mode == SceneMode::Morphing {
            return None;
        }
        let old = self.transform;
        let p = self.position_wc();
        let local = Transforms::east_north_up_to_fixed_frame(p, self.projection.ellipsoid());
        self.set_transform(local).ok()?;
        let hpr = HeadingPitchRoll::new(
            Self::heading_for(self.direction, self.up),
            Self::pitch_for(self.direction),
            Self::roll_for(self.direction, self.up, self.right),
        );
        self.set_transform(old).ok()?;
        Some(hpr)
    }
    pub fn heading(&mut self) -> Option<f64> {
        self.heading_pitch_roll().map(|p| p.heading)
    }
    pub fn pitch(&mut self) -> Option<f64> {
        self.heading_pitch_roll().map(|p| p.pitch)
    }
    pub fn roll(&mut self) -> Option<f64> {
        self.heading_pitch_roll().map(|p| p.roll)
    }

    pub fn uniform_camera(&mut self) -> UniformCamera {
        self.update_members();
        UniformCamera {
            view: self.members.view,
            inverse_view: self.members.inverse_view,
            position: self.members.position_wc,
            direction: self.members.direction_wc,
            up: self.members.up_wc,
            right: self.members.right_wc,
            cartographic: self.members.cartographic,
            frustum: self.frustum.uniform_frustum(),
        }
    }

    fn clamp_move_2d(&mut self) {
        let x = self.max_coord.x;
        let y = self.max_coord.y;
        let rotating = self.map_mode_2d == MapMode2D::Rotate;
        let max_x = if rotating { x } else { self.position.x - x * 2.0 };
        let min_x = if rotating { -x } else { self.position.x + x * 2.0 };
        if self.position.x > x {
            self.position.x = max_x;
        }
        if self.position.x < -x {
            self.position.x = min_x;
        }
        self.position.y = self.position.y.clamp(-y, y);
    }

    fn orthographic_width(&mut self) -> f64 {
        if self.transform != Matrix4::IDENTITY {
            return self.position.magnitude();
        }
        let window = Cartesian2::new(
            self.drawing_buffer_width / self.pixel_ratio / 2.0,
            self.drawing_buffer_height / self.pixel_ratio / 2.0,
        );
        let ray = self.get_pick_ray(window);
        let position = self.position_wc();
        if let Some(surface) = &self.surface {
            let globe = ray.as_ref().and_then(|r| surface.pick_globe(r));
            let depth = surface.pick_depth(window);
            if globe.is_some() || depth.is_some() {
                return globe
                    .map_or(f64::INFINITY, |p| p.distance(position))
                    .min(depth.map_or(f64::INFINITY, |p| p.distance(position)));
            }
        }
        self.position_cartographic().map_or(f64::NAN, |p| p.height.max(0.0))
    }

    fn adjust_orthographic_frustum(&mut self, zooming: bool) {
        if !matches!(self.frustum, CameraFrustum::Orthographic(_)) {
            return;
        }
        if !zooming && self.members.cartographic.is_some_and(|p| p.height < 150_000.0) {
            return;
        }
        let width = self.orthographic_width();
        if let CameraFrustum::Orthographic(f) = &mut self.frustum {
            f.width = width;
        }
    }

    pub fn move_direction(&mut self, direction: Cartesian3, amount: f64) {
        self.position += direction.multiply_by_scalar(amount);
        if self.mode == SceneMode::Scene2D {
            self.clamp_move_2d();
        }
        self.adjust_orthographic_frustum(true);
    }
    pub fn move_forward(&mut self, amount: Option<f64>) {
        let amount = amount.unwrap_or(self.default_move_amount);
        if self.mode == SceneMode::Scene2D {
            self.zoom_2d(amount);
        } else {
            self.move_direction(self.direction, amount);
        }
    }
    pub fn move_backward(&mut self, amount: Option<f64>) {
        self.move_forward(Some(-amount.unwrap_or(self.default_move_amount)));
    }
    pub fn move_up(&mut self, amount: Option<f64>) {
        self.move_direction(self.up, amount.unwrap_or(self.default_move_amount));
    }
    pub fn move_down(&mut self, amount: Option<f64>) {
        self.move_direction(self.up, -amount.unwrap_or(self.default_move_amount));
    }
    pub fn move_right(&mut self, amount: Option<f64>) {
        self.move_direction(self.right, amount.unwrap_or(self.default_move_amount));
    }
    pub fn move_left(&mut self, amount: Option<f64>) {
        self.move_direction(self.right, -amount.unwrap_or(self.default_move_amount));
    }

    pub fn look(&mut self, axis: Cartesian3, angle: Option<f64>) {
        let q = Quaternion::from_axis_angle(axis, -angle.unwrap_or(self.default_look_amount));
        let rotation = Matrix3::from_quaternion(&q);
        self.direction = rotation.multiply_by_vector(self.direction);
        self.up = rotation.multiply_by_vector(self.up);
        self.right = rotation.multiply_by_vector(self.right);
    }
    pub fn look_left(&mut self, amount: Option<f64>) {
        if self.mode != SceneMode::Scene2D {
            self.look(self.up, Some(-amount.unwrap_or(self.default_look_amount)));
        }
    }
    pub fn look_right(&mut self, amount: Option<f64>) {
        if self.mode != SceneMode::Scene2D {
            self.look(self.up, amount);
        }
    }
    pub fn look_up(&mut self, amount: Option<f64>) {
        if self.mode != SceneMode::Scene2D {
            self.look(self.right, Some(-amount.unwrap_or(self.default_look_amount)));
        }
    }
    pub fn look_down(&mut self, amount: Option<f64>) {
        if self.mode != SceneMode::Scene2D {
            self.look(self.right, amount);
        }
    }
    pub fn twist_left(&mut self, amount: Option<f64>) {
        self.look(self.direction, amount);
    }
    pub fn twist_right(&mut self, amount: Option<f64>) {
        self.look(self.direction, Some(-amount.unwrap_or(self.default_look_amount)));
    }

    pub fn rotate(&mut self, axis: Cartesian3, angle: Option<f64>) {
        let q = Quaternion::from_axis_angle(axis, -angle.unwrap_or(self.default_rotate_amount));
        let rotation = Matrix3::from_quaternion(&q);
        self.position = rotation.multiply_by_vector(self.position);
        self.direction = rotation.multiply_by_vector(self.direction);
        self.up = rotation.multiply_by_vector(self.up);
        self.right = self.direction.cross(self.up);
        self.up = self.right.cross(self.direction);
        self.adjust_orthographic_frustum(false);
    }
    fn rotate_vertical(&mut self, mut angle: f64) {
        if let Some(axis) = self.constrained_axis.filter(|_| !self.position.equals_epsilon(Cartesian3::ZERO, EPSILON2))
        {
            let p = self.position.normalize();
            let north = p.equals_epsilon(axis, EPSILON2);
            let south = p.equals_epsilon(axis.negate(), EPSILON2);
            if !north && !south {
                let a = axis.normalize();
                let to_north = acos_clamped(p.dot(a));
                if angle > 0.0 && angle > to_north {
                    angle = to_north - EPSILON4;
                }
                let to_south = acos_clamped(p.dot(a.negate()));
                if angle < 0.0 && -angle > to_south {
                    angle = -to_south + EPSILON4;
                }
                self.rotate(a.cross(p), Some(angle));
            } else if (north && angle < 0.0) || (south && angle > 0.0) {
                self.rotate(self.right, Some(angle));
            }
        } else {
            self.rotate(self.right, Some(angle));
        }
    }
    pub fn rotate_down(&mut self, angle: Option<f64>) {
        self.rotate_vertical(angle.unwrap_or(self.default_rotate_amount));
    }
    pub fn rotate_up(&mut self, angle: Option<f64>) {
        self.rotate_vertical(-angle.unwrap_or(self.default_rotate_amount));
    }
    pub fn rotate_right(&mut self, angle: Option<f64>) {
        self.rotate(self.constrained_axis.unwrap_or(self.up), Some(-angle.unwrap_or(self.default_rotate_amount)));
    }
    pub fn rotate_left(&mut self, angle: Option<f64>) {
        self.rotate(self.constrained_axis.unwrap_or(self.up), angle);
    }

    fn zoom_2d(&mut self, amount: f64) {
        let CameraFrustum::OrthographicOffCenter(f) = &mut self.frustum else {
            unreachable!("2D frustum checked by update")
        };
        let amount = amount * 0.5;
        let factor = if self.map_mode_2d == MapMode2D::Rotate { self.maximum_zoom_factor } else { 1.0 };
        if f.top.abs() + f.bottom.abs() > f.left.abs() + f.right.abs() {
            let mut top = f.top - amount;
            let mut bottom = f.bottom + amount;
            let max_bottom = self.max_coord.y * factor;
            if bottom > max_bottom {
                bottom = max_bottom;
                top = -max_bottom;
            }
            if top <= bottom {
                top = 1.0;
                bottom = -1.0;
            }
            let ratio = f.right / f.top;
            f.top = top;
            f.bottom = bottom;
            f.right = f.top * ratio;
            f.left = -f.right;
        } else {
            let mut right = f.right - amount;
            let mut left = f.left + amount;
            let max_right = self.max_coord.x * factor;
            if right > max_right {
                right = max_right;
                left = -max_right;
            }
            if right <= left {
                right = 1.0;
                left = -1.0;
            }
            let ratio = f.top / f.right;
            f.right = right;
            f.left = left;
            f.top = f.right * ratio;
            f.bottom = -f.top;
        }
    }
    pub fn zoom_in(&mut self, amount: Option<f64>) {
        let amount = amount.unwrap_or(self.default_zoom_amount);
        if self.mode == SceneMode::Scene2D {
            self.zoom_2d(amount);
        } else {
            self.move_direction(self.direction, amount);
        }
    }
    pub fn zoom_out(&mut self, amount: Option<f64>) {
        self.zoom_in(Some(-amount.unwrap_or(self.default_zoom_amount)));
    }
    pub fn get_magnitude(&self) -> Option<f64> {
        match self.mode {
            SceneMode::Scene3D => Some(self.position.magnitude()),
            SceneMode::ColumbusView => Some(self.position.z.abs()),
            SceneMode::Scene2D => self.frustum.orthographic().map(|f| (f.right - f.left).max(f.top - f.bottom)),
            SceneMode::Morphing => None,
        }
    }

    pub fn switch_to_perspective_frustum(&mut self) {
        if self.mode != SceneMode::Scene2D && !matches!(self.frustum, CameraFrustum::Perspective(_)) {
            self.frustum = CameraFrustum::Perspective(PerspectiveFrustum::new(
                to_radians(60.0),
                self.drawing_buffer_width / self.drawing_buffer_height,
            ));
        }
    }
    pub fn switch_to_orthographic_frustum(&mut self) {
        if self.mode != SceneMode::Scene2D && !matches!(self.frustum, CameraFrustum::Orthographic(_)) {
            let width = self.orthographic_width();
            self.frustum = CameraFrustum::Orthographic(OrthographicFrustum::new(
                width,
                self.drawing_buffer_width / self.drawing_buffer_height,
            ));
        }
    }

    pub fn set_view(&mut self, view: CameraView) -> Result<(), CameraError> {
        if self.mode == SceneMode::Morphing {
            return Ok(());
        }
        if let Some(transform) = view.end_transform {
            self.set_transform(transform)?;
        }
        let mut convert = view.convert;
        let destination = match view.destination {
            Some(CameraDestination::Cartesian(p)) => p,
            Some(CameraDestination::Rectangle(r)) => {
                convert = false;
                self.get_rectangle_camera_coordinates(r).expect("settled mode")
            }
            None => self.position_wc(),
        };
        if destination.x.is_nan() || destination.y.is_nan() || destination.z.is_nan() {
            return Err(CameraError("position has a NaN component"));
        }
        let mut hpr = match view.orientation {
            None => HeadingPitchRoll::new(0.0, -PI_OVER_TWO, 0.0),
            Some(CameraOrientation::HeadingPitchRoll(hpr)) => hpr,
            Some(CameraOrientation::DirectionUp { mut direction, mut up }) => {
                if self.mode == SceneMode::Scene3D {
                    let inv = Transforms::east_north_up_to_fixed_frame(destination, self.projection.ellipsoid())
                        .inverse_transformation();
                    direction = inv.multiply_by_point_as_vector(direction);
                    up = inv.multiply_by_point_as_vector(up);
                }
                HeadingPitchRoll::new(
                    Self::heading_for(direction, up),
                    Self::pitch_for(direction),
                    Self::roll_for(direction, up, direction.cross(up)),
                )
            }
        };
        let current_transform = self.transform;
        if self.mode == SceneMode::Scene3D {
            let local = Transforms::east_north_up_to_fixed_frame(destination, self.projection.ellipsoid());
            self.set_transform(local)?;
            self.position = Cartesian3::ZERO;
        } else {
            self.set_transform(Matrix4::IDENTITY)?;
            if destination != self.position_wc() {
                let p = if convert {
                    let cartographic = self
                        .projection
                        .ellipsoid()
                        .cartesian_to_cartographic(destination)
                        .ok_or(CameraError("destination must have a cartographic position"))?;
                    self.projection.project(&cartographic)
                } else {
                    destination
                };
                if self.mode == SceneMode::Scene2D {
                    self.position.x = p.x;
                    self.position.y = p.y;
                    if let CameraFrustum::OrthographicOffCenter(f) = &mut self.frustum {
                        let right = p.z * 0.5;
                        if right > -right {
                            let ratio = f.top / f.right;
                            f.right = right;
                            f.left = -right;
                            f.top = right * ratio;
                            f.bottom = -f.top;
                        }
                    }
                } else {
                    self.position = p;
                }
            }
        }
        if self.mode != SceneMode::Scene2D || self.map_mode_2d == MapMode2D::Rotate {
            hpr.heading -= PI_OVER_TWO;
            if self.mode == SceneMode::Scene2D {
                hpr.pitch = -PI_OVER_TWO;
                hpr.roll = 0.0;
            }
            let rot = Matrix3::from_quaternion(&Quaternion::from_heading_pitch_roll(&hpr));
            if self.mode != SceneMode::Scene2D {
                self.direction = Cartesian3::new(rot.0[0], rot.0[1], rot.0[2]);
            }
            self.up = Cartesian3::new(rot.0[6], rot.0[7], rot.0[8]);
            self.right = self.direction.cross(self.up);
        }
        self.set_transform(current_transform)?;
        if self.mode != SceneMode::Scene2D {
            self.adjust_orthographic_frustum(true);
        }
        Ok(())
    }

    pub fn offset_from_heading_pitch_range(offset: HeadingPitchRange) -> Cartesian3 {
        let pitch = offset.pitch.clamp(-PI_OVER_TWO, PI_OVER_TWO);
        let heading = zero_to_two_pi(offset.heading) - PI_OVER_TWO;
        let pitch_q = Quaternion::from_axis_angle(Cartesian3::UNIT_Y, -pitch);
        let heading_q = Quaternion::from_axis_angle(Cartesian3::UNIT_Z, -heading);
        let rotation = Matrix3::from_quaternion(&heading_q.multiply(&pitch_q));
        rotation.multiply_by_vector(Cartesian3::UNIT_X).negate().multiply_by_scalar(offset.range)
    }
    pub fn look_at(&mut self, target: Cartesian3, offset: CameraOffset) -> Result<(), CameraError> {
        let transform = Transforms::east_north_up_to_fixed_frame(target, &self.ellipsoid);
        self.look_at_transform(transform, Some(offset))
    }
    pub fn look_at_transform(&mut self, transform: Matrix4, offset: Option<CameraOffset>) -> Result<(), CameraError> {
        if self.mode == SceneMode::Morphing {
            return Err(CameraError("lookAtTransform is not supported while morphing."));
        }
        self.set_transform(transform)?;
        let offset = match offset {
            None => return Ok(()),
            Some(CameraOffset::Cartesian(p)) => p,
            Some(CameraOffset::HeadingPitchRange(p)) => Self::offset_from_heading_pitch_range(p),
        };
        if self.mode == SceneMode::Scene2D {
            self.position.x = 0.0;
            self.position.y = 0.0;
            self.up = Cartesian3::new(-offset.x, -offset.y, 0.0);
            if self.up.magnitude_squared() < EPSILON10 {
                self.up = Cartesian3::UNIT_Y;
            }
            self.up = self.up.normalize();
            self.set_transform(Matrix4::IDENTITY)?;
            self.direction = Cartesian3::UNIT_Z.negate();
            self.right = self.direction.cross(self.up).normalize();
            if let CameraFrustum::OrthographicOffCenter(f) = &mut self.frustum {
                let ratio = f.top / f.right;
                f.right = offset.magnitude() * 0.5;
                f.left = -f.right;
                f.top = ratio * f.right;
                f.bottom = -f.top;
            }
            self.set_transform(transform)?;
        } else {
            self.position = offset;
            self.direction = self.position.negate().normalize();
            self.right = self.direction.cross(Cartesian3::UNIT_Z);
            if self.right.magnitude_squared() < EPSILON10 {
                self.right = Cartesian3::UNIT_X;
            }
            self.right = self.right.normalize();
            self.up = self.right.cross(self.direction).normalize();
            self.adjust_orthographic_frustum(true);
        }
        Ok(())
    }

    pub fn view_bounding_sphere(
        &mut self,
        sphere: &BoundingSphere,
        offset: Option<HeadingPitchRange>,
    ) -> Result<(), CameraError> {
        if self.mode == SceneMode::Morphing {
            return Err(CameraError("viewBoundingSphere is not supported while morphing."));
        }
        let offset = self.bounding_sphere_offset(sphere, offset);
        self.look_at(sphere.center, CameraOffset::HeadingPitchRange(offset))
    }

    fn bounding_sphere_offset(&self, sphere: &BoundingSphere, offset: Option<HeadingPitchRange>) -> HeadingPitchRange {
        let mut offset = offset.unwrap_or(Self::DEFAULT_OFFSET);
        if offset.range == 0.0 {
            offset.range = if sphere.radius == 0.0 {
                100.0
            } else if let Some(f) = self.frustum.orthographic() {
                let ratio = f.right / f.top;
                let (right, top) = if sphere.radius > sphere.radius * ratio {
                    (sphere.radius, sphere.radius / ratio)
                } else {
                    (sphere.radius * ratio, sphere.radius)
                };
                right.max(top) * 1.5
            } else if let CameraFrustum::Perspective(f) = self.frustum {
                let tan_phi = (f.fovy() * 0.5).js_tan();
                (sphere.radius / (f.aspect_ratio * tan_phi)).max(sphere.radius / tan_phi)
            } else {
                unreachable!()
            };
            offset.range = offset.range.clamp(self.minimum_zoom_distance, self.maximum_zoom_distance);
        }
        offset
    }

    fn rectangle_position_3d(&self, rectangle: Rectangle) -> (Cartesian3, Cartesian3, Cartesian3, Cartesian3) {
        let ellipsoid = self.projection.ellipsoid();
        let Rectangle { west, mut east, south, north } = rectangle;
        if west > east {
            east += TWO_PI;
        }
        let longitude = (west + east) * 0.5;
        let latitude = if south < -PI_OVER_TWO + RADIANS_PER_DEGREE && north > PI_OVER_TWO - RADIANS_PER_DEGREE {
            0.0
        } else {
            let geodesic = EllipsoidGeodesic::new(
                &Cartographic::new(longitude, north, 0.0),
                &Cartographic::new(longitude, south, 0.0),
                ellipsoid,
            )
            .expect("rectangle latitude endpoints must not be antipodal");
            geodesic.interpolate_using_fraction(0.5).latitude
        };
        let center = ellipsoid.cartographic_to_cartesian(&Cartographic::new(longitude, latitude, 0.0));
        let point = |lon, lat| ellipsoid.cartographic_to_cartesian(&Cartographic::new(lon, lat, 0.0)) - center;
        let ne = point(east, north);
        let nw = point(west, north);
        let se = point(east, south);
        let sw = point(west, south);
        let nc = point(longitude, north);
        let sc = point(longitude, south);
        let direction = ellipsoid.geodetic_surface_normal(center).expect("ellipsoid surface point").negate();
        let right = direction.cross(Cartesian3::UNIT_Z).normalize();
        let up = right.cross(direction);
        let distance = match self.frustum {
            CameraFrustum::Orthographic(f) => {
                let width = ne.distance(nw).max(se.distance(sw));
                let height = ne.distance(se).max(nw.distance(sw));
                let off = f.off_center_frustum();
                let ratio = off.right / off.top;
                if width > height * ratio {
                    width.max(width / ratio)
                } else {
                    height.max(height * ratio)
                }
            }
            CameraFrustum::Perspective(f) => {
                let tan_phi = (f.fovy() * 0.5).js_tan();
                let tan_theta = f.aspect_ratio * tan_phi;
                let compute = |axis: Cartesian3, corner: Cartesian3, tangent| {
                    axis.dot(corner).abs() / tangent - direction.dot(corner)
                };
                let mut d = f64::NEG_INFINITY;
                for p in [nw, se, ne, sw, nc, sc] {
                    d = d.max(compute(up, p, tan_phi));
                }
                for p in [nw, se, ne, sw, nc, sc] {
                    d = d.max(compute(right, p, tan_theta));
                }
                if south < 0.0 && north > 0.0 {
                    for lon in [west, east] {
                        let p = point(lon, 0.0);
                        d = d.max(compute(up, p, tan_phi)).max(compute(right, p, tan_theta));
                    }
                }
                d
            }
            CameraFrustum::OrthographicOffCenter(_) => unreachable!("3D frustum checked by update"),
        };
        (center + direction.multiply_by_scalar(-distance), direction, up, right)
    }

    pub fn get_rectangle_camera_coordinates(&mut self, mut rectangle: Rectangle) -> Option<Cartesian3> {
        match self.mode {
            SceneMode::Morphing => None,
            SceneMode::Scene3D => Some(self.rectangle_position_3d(rectangle).0),
            SceneMode::ColumbusView => {
                self.update_members();
                if rectangle.west > rectangle.east {
                    rectangle = Rectangle::MAX_VALUE;
                }
                let project = |lon, lat| {
                    let p = self.projection.project(&Cartographic::new(lon, lat, 0.0));
                    self.members
                        .actual_inverse_transform
                        .multiply_by_point(self.members.actual_transform.multiply_by_point(p))
                };
                let ne = project(rectangle.east, rectangle.north);
                let sw = project(rectangle.west, rectangle.south);
                let z = if let CameraFrustum::Perspective(f) = self.frustum {
                    let tan_phi = (f.fovy() * 0.5).js_tan();
                    let tan_theta = f.aspect_ratio * tan_phi;
                    ((ne.x - sw.x) / tan_theta).max((ne.y - sw.y) / tan_phi) * 0.5
                } else {
                    (ne.x - sw.x).max(ne.y - sw.y)
                };
                Some(Cartesian3::new((ne.x - sw.x) * 0.5 + sw.x, (ne.y - sw.y) * 0.5 + sw.y, z))
            }
            SceneMode::Scene2D => {
                if rectangle.west > rectangle.east {
                    if self.map_mode_2d == MapMode2D::InfiniteScroll {
                        rectangle.east += TWO_PI;
                    } else {
                        rectangle = Rectangle::MAX_VALUE;
                    }
                }
                let ne = self.projection.project(&Cartographic::new(rectangle.east, rectangle.north, 0.0));
                let sw = self.projection.project(&Cartographic::new(rectangle.west, rectangle.south, 0.0));
                let width = (ne.x - sw.x).abs() * 0.5;
                let height = (ne.y - sw.y).abs() * 0.5;
                let f = self.frustum.orthographic().expect("2D frustum");
                let ratio = f.right / f.top;
                let (right, top) =
                    if width > height * ratio { (width, width / ratio) } else { (height * ratio, height) };
                let p = Cartesian3::new((ne.x - sw.x) * 0.5 + sw.x, (ne.y - sw.y) * 0.5 + sw.y, 0.0);
                let mut cartographic = self.projection.unproject(p);
                cartographic.height = (2.0 * right).max(2.0 * top);
                Some(self.projection.project(&cartographic))
            }
        }
    }

    pub fn get_pick_ray(&mut self, window: Cartesian2) -> Option<Ray> {
        if self.client_width <= 0.0 || self.client_height <= 0.0 {
            return None;
        }
        self.update_members();
        let x = (2.0 / self.client_width) * window.x - 1.0;
        let y = (2.0 / self.client_height) * (self.client_height - window.y) - 1.0;
        let position = self.members.position_wc;
        let direction = self.members.direction_wc;
        if let CameraFrustum::Perspective(f) = self.frustum {
            let tan_phi = (f.fovy() * 0.5).js_tan();
            let tan_theta = f.aspect_ratio * tan_phi;
            let center = position + direction.multiply_by_scalar(f.near);
            let xd = self.members.right_wc.multiply_by_scalar(x * f.near * tan_theta);
            let yd = self.members.up_wc.multiply_by_scalar(y * f.near * tan_phi);
            Some(Ray { origin: position, direction: ((center + xd + yd) - position).normalize() })
        } else {
            let f = self.frustum.orthographic().expect("orthographic frustum");
            let x = x * (f.right - f.left) * 0.5;
            let y = y * (f.top - f.bottom) * 0.5;
            let mut origin = self.members.right_wc.multiply_by_scalar(x) + position;
            origin = self.members.up_wc.multiply_by_scalar(y) + origin;
            if self.mode == SceneMode::Scene2D && self.map_mode_2d == MapMode2D::InfiniteScroll {
                origin.y = modulo(origin.y + self.max_coord.x, 2.0 * self.max_coord.x) - self.max_coord.x;
            }
            Some(Ray { origin, direction })
        }
    }

    pub fn pick_ellipsoid(&mut self, window: Cartesian2, ellipsoid: &Ellipsoid) -> Option<Cartesian3> {
        if self.mode == SceneMode::Morphing {
            return None;
        }
        let ray = self.get_pick_ray(window)?;
        if self.mode == SceneMode::Scene3D {
            let interval = intersection_tests::ray_ellipsoid(&ray, ellipsoid)?;
            return Some(ray.get_point(if interval.start > 0.0 { interval.start } else { interval.stop }));
        }
        let p =
            if self.mode == SceneMode::Scene2D { ray.origin } else { ray.get_point(-ray.origin.x / ray.direction.x) };
        let cartographic = self.projection.unproject(Cartesian3::new(p.y, p.z, 0.0));
        if cartographic.latitude < -PI_OVER_TWO || cartographic.latitude > PI_OVER_TWO {
            return None;
        }
        if self.mode == SceneMode::ColumbusView && (cartographic.longitude < -PI || cartographic.longitude > PI) {
            return None;
        }
        Some(self.projection.ellipsoid().cartographic_to_cartesian(&cartographic))
    }

    pub fn distance_to_bounding_sphere(&mut self, sphere: &BoundingSphere) -> f64 {
        self.update_members();
        let delta = self.members.position_wc - sphere.center;
        let projection = self.members.direction_wc.multiply_by_scalar(delta.dot(self.members.direction_wc));
        (projection.magnitude() - sphere.radius).max(0.0)
    }
    pub fn get_pixel_size(&mut self, sphere: &BoundingSphere, width: f64, height: f64) -> f64 {
        let distance = self.distance_to_bounding_sphere(sphere);
        let dimensions = self.frustum.get_pixel_dimensions(width, height, distance, self.pixel_ratio);
        dimensions.x.max(dimensions.y)
    }

    pub fn compute_view_rectangle(&mut self, ellipsoid: &Ellipsoid) -> Result<Option<Rectangle>, CameraError> {
        self.update_members();
        let culling = self.frustum.compute_culling_volume(
            self.members.position_wc,
            self.members.direction_wc,
            self.members.up_wc,
        );
        let sphere = BoundingSphere::new(Cartesian3::ZERO, ellipsoid.maximum_radius);
        if culling.compute_visibility(&sphere) == Intersect::Outside {
            return Ok(None);
        }
        let q = ellipsoid.one_over_radii.multiply_components(self.members.position_wc);
        let q_magnitude = q.magnitude();
        let q_unit = q.normalize();
        let (east_unit, north_unit) = if q_unit.equals_epsilon(Cartesian3::UNIT_Z, EPSILON10) {
            (Cartesian3::UNIT_Y, Cartesian3::UNIT_Z)
        } else {
            let east = Cartesian3::UNIT_Z.cross(q_unit).normalize();
            (east, q_unit.cross(east).normalize())
        };
        let w = (q.magnitude_squared() - 1.0).sqrt();
        let center = q_unit.multiply_by_scalar(1.0 / q_magnitude);
        let east = east_unit.multiply_by_scalar(w / q_magnitude);
        let north = north_unit.multiply_by_scalar(w / q_magnitude);
        let horizon = [center + north - east, center - north - east, center - north + east, center + north + east];
        let windows = [
            Cartesian2::ZERO,
            Cartesian2::new(0., self.client_height),
            Cartesian2::new(self.client_width, self.client_height),
            Cartesian2::new(self.client_width, 0.),
        ];
        let mut cartographics = [Cartographic::default(); 4];
        let mut successes = 0;
        for i in 0..4 {
            let point = if let Some(p) = self.pick_ellipsoid(windows[i], ellipsoid) {
                successes += 1;
                p
            } else {
                if !w.is_finite() {
                    return Err(CameraError("normalized result is not a number"));
                }
                ellipsoid.radii.multiply_components(horizon[i])
            };
            cartographics[i] =
                ellipsoid.cartesian_to_cartographic(point).ok_or(CameraError("invalid horizon position"))?;
        }
        if successes < 2 {
            return Ok(Some(Rectangle::MAX_VALUE));
        }
        let mut result = Rectangle::from_cartographic_array(&cartographics);
        let mut distance = 0.0;
        let mut last = cartographics[3].longitude;
        for c in cartographics {
            let diff = (c.longitude - last).abs();
            distance += if diff > PI { TWO_PI - diff } else { diff };
            last = c.longitude;
        }
        if equals_epsilon(distance.abs(), TWO_PI, EPSILON9, EPSILON9) {
            result.west = -PI;
            result.east = PI;
            if cartographics[0].latitude >= 0.0 {
                result.north = PI_OVER_TWO;
            } else {
                result.south = -PI_OVER_TWO;
            }
        }
        Ok(Some(result))
    }

    /// Runs Cesium's camera-change detection with an explicit monotonic
    /// timestamp in milliseconds, avoiding a hidden platform clock.
    pub fn update_camera_changed(&mut self, timestamp_ms: f64) {
        let position = self.position_wc();
        if let Some(old) = self.old_position_wc {
            self.position_wc_delta_magnitude_last_frame = self.position_wc_delta_magnitude;
            self.position_wc_delta_magnitude = (position - old).magnitude();
            if self.position_wc_delta_magnitude > 0.0 {
                self.time_since_moved = 0.0;
                self.last_moved_timestamp = timestamp_ms;
            } else {
                self.time_since_moved = (timestamp_ms - self.last_moved_timestamp).max(0.0) / 1000.0;
            }
        }
        self.old_position_wc = Some(position);
        if self.changed.number_of_listeners() == 0 {
            return;
        }
        let threshold = self.percentage_changed;
        let heading = self.heading().unwrap_or(f64::NAN);
        let previous_heading = *self.changed_heading.get_or_insert(heading);
        let delta = (previous_heading - heading).abs() % TWO_PI;
        let heading_percentage = if delta > PI { TWO_PI - delta } else { delta } / PI;
        if heading_percentage > threshold {
            self.changed_heading = Some(heading);
        }
        let roll = self.roll().unwrap_or(f64::NAN);
        let previous_roll = *self.changed_roll.get_or_insert(roll);
        let delta = (previous_roll - roll).abs() % TWO_PI;
        let roll_percentage = if delta > PI { TWO_PI - delta } else { delta } / PI;
        if roll_percentage > threshold {
            self.changed_roll = Some(roll);
        }
        if heading_percentage > threshold || roll_percentage > threshold {
            self.changed.raise_event(&roll_percentage.max(heading_percentage));
        }
        if self.mode == SceneMode::Scene2D {
            let f = self.frustum.orthographic().expect("2D frustum");
            if let (Some(last), Some(last_p)) = (self.changed_frustum, self.changed_position) {
                let p = self.position;
                let (x0, x1, x2, x3) = (p.x + f.left, p.x + f.right, last_p.x + last.left, last_p.x + last.right);
                let (y0, y1, y2, y3) = (p.y + f.bottom, p.y + f.top, last_p.y + last.bottom, last_p.y + last.top);
                let (left, right, bottom, top) = (x0.max(x2), x1.min(x3), y0.max(y2), y1.min(y3));
                // Cesium compares bottom to y1 here, rather than to top.
                let area = if left >= right || bottom >= y1 {
                    1.0
                } else {
                    let reference = if x0 < x2 && x1 > x3 && y0 < y2 && y1 > y3 { f } else { last };
                    1.0 - ((right - left) * (top - bottom))
                        / ((reference.right - reference.left) * (reference.top - reference.bottom))
                };
                if area > threshold {
                    self.changed.raise_event(&area);
                    self.changed_position = Some(p);
                    self.changed_frustum = Some(f);
                }
            } else {
                self.changed_position = Some(self.position);
                self.changed_frustum = Some(f);
            }
            return;
        }
        self.update_members();
        if let (Some(last_direction), Some(last_position)) = (self.changed_direction, self.changed_position) {
            let angle = acos_clamped(self.members.direction_wc.dot(last_direction));
            let direction_percentage =
                if let CameraFrustum::Perspective(f) = self.frustum { angle / (f.fovy() * 0.5) } else { angle };
            let height_percentage = self.members.position_wc.distance(last_position)
                / self.members.cartographic.map_or(f64::NAN, |p| p.height);
            if direction_percentage > threshold || height_percentage > threshold {
                self.changed.raise_event(&direction_percentage.max(height_percentage));
                self.changed_position = Some(self.members.position_wc);
                self.changed_direction = Some(self.members.direction_wc);
            }
        } else {
            self.changed_position = Some(self.members.position_wc);
            self.changed_direction = Some(self.members.direction_wc);
        }
    }

    fn flight_orientation(&self, destination: Cartesian3, orientation: Option<CameraOrientation>) -> HeadingPitchRoll {
        match orientation {
            None => HeadingPitchRoll::new(0., -PI_OVER_TWO, 0.),
            Some(CameraOrientation::HeadingPitchRoll(hpr)) => hpr,
            Some(CameraOrientation::DirectionUp { mut direction, mut up }) => {
                if self.mode == SceneMode::Scene3D {
                    let inverse = Transforms::east_north_up_to_fixed_frame(destination, self.projection.ellipsoid())
                        .inverse_transformation();
                    direction = inverse.multiply_by_point_as_vector(direction);
                    up = inverse.multiply_by_point_as_vector(up);
                }
                HeadingPitchRoll::new(
                    Self::heading_for(direction, up),
                    Self::pitch_for(direction),
                    Self::roll_for(direction, up, direction.cross(up)),
                )
            }
        }
    }

    pub fn can_preload_flight(&self) -> bool {
        self.current_flight.is_some() && self.mode != SceneMode::Scene2D
    }
    pub fn flight_in_progress(&self) -> bool {
        self.current_flight.is_some()
    }
    pub fn scheduled_flight_count(&self) -> usize {
        self.scheduled_flights.len()
    }

    pub fn fly_to(&mut self, mut options: CameraFlightOptions) -> Result<(), CameraError> {
        let requested = options.view.destination.ok_or(CameraError("destination is required."))?;
        if self.mode == SceneMode::Morphing {
            return Ok(());
        }
        self.cancel_flight();
        let destination = match requested {
            CameraDestination::Cartesian(p) => p,
            CameraDestination::Rectangle(r) => self.get_rectangle_camera_coordinates(r).expect("settled mode"),
        };
        let orientation = self.flight_orientation(destination, options.view.orientation);
        if options.duration.is_some_and(|d| d <= 0.) {
            options.view.orientation = Some(CameraOrientation::HeadingPitchRoll(orientation));
            self.set_view(options.view)?;
            if let Some(mut complete) = options.complete {
                complete(self);
            }
            return Ok(());
        }
        self.next_flight_id += 1;
        let id = self.next_flight_id;
        let mut complete = options.complete;
        let path_options = CameraFlightPathOptions {
            destination,
            orientation,
            duration: options.duration,
            convert: !matches!(requested, CameraDestination::Rectangle(_)) && options.view.convert,
            end_transform: options.view.end_transform,
            maximum_height: options.maximum_height,
            pitch_adjust_height: options.pitch_adjust_height,
            fly_over_longitude: options.fly_over_longitude,
            fly_over_longitude_weight: options.fly_over_longitude_weight,
            easing_function: options.easing_function,
            cancel: options.cancel,
            complete: Some(Box::new(move |camera| {
                if camera.current_flight.as_ref().is_some_and(|f| f.id == id) {
                    camera.current_flight = None;
                }
                if let Some(callback) = &mut complete {
                    callback(camera);
                }
            })),
        };
        let tween = Rc::new(CameraFlightPath::create_tween(self, path_options)?);
        if tween.duration == 0. {
            tween.complete(self)?;
            return Ok(());
        }
        self.current_flight = Some(CurrentFlight { id, tween: Rc::clone(&tween), destination, orientation });
        self.scheduled_flights.push(ScheduledFlight { id, tween, started: None });
        if self.mode != SceneMode::Scene2D {
            let retain = self.preload_flight_camera.is_some();
            let mut preload = self.preload_flight_camera.take().unwrap_or_else(|| Box::new(self.clone_camera()));
            let result = preload.set_view(CameraView {
                destination: Some(CameraDestination::Cartesian(destination)),
                orientation: Some(CameraOrientation::HeadingPitchRoll(orientation)),
                ..CameraView::default()
            });
            if result.is_ok() {
                let p = preload.position_wc();
                let d = preload.direction_wc();
                let u = preload.up_wc();
                self.preload_flight_culling_volume = Some(preload.frustum.compute_culling_volume(p, d, u));
            }
            if retain {
                self.preload_flight_camera = Some(preload);
            }
            result?;
        }
        Ok(())
    }

    /// Drive the camera's flight tweens with an absolute monotonic time in
    /// seconds. A tween starts on its first update, as in TweenCollection.
    /// Completion callbacks may enqueue more flights during the same tick.
    pub fn update_flights(&mut self, time: f64) -> Result<(), CameraError> {
        let mut i = 0;
        let time_ms = time / 0.001;
        while i < self.scheduled_flights.len() {
            let mut entry = self.scheduled_flights.remove(i);
            let start = *entry.started.get_or_insert(time_ms);
            if time_ms < start {
                self.scheduled_flights.insert(i, entry);
                i += 1;
                continue;
            }
            let duration_ms = entry.tween.duration / 0.001;
            let elapsed = time_ms - start;
            let fraction = if elapsed >= duration_ms { 1. } else { elapsed / duration_ms };
            let eased = entry.tween.easing_function.as_ref().map_or(fraction, |e| e.evaluate(fraction));
            if let Err(error) = entry.tween.update(self, entry.tween.duration * eased) {
                self.scheduled_flights.insert(i, entry);
                return Err(error);
            }
            if elapsed >= duration_ms {
                entry.tween.complete(self)?;
            } else {
                self.scheduled_flights.insert(i, entry);
                i += 1;
            }
        }
        Ok(())
    }

    fn remove_flight(&mut self, id: u64) {
        if let Some(index) = self.scheduled_flights.iter().position(|f| f.id == id) {
            let entry = self.scheduled_flights.remove(index);
            entry.tween.cancel(self);
        }
    }
    pub fn cancel_flight(&mut self) {
        if let Some(flight) = self.current_flight.clone() {
            self.remove_flight(flight.id);
            self.current_flight = None;
        }
    }
    /// Cesium completes a flight by cancelling its queued tween, applying
    /// setView, then calling completion; the cancellation callback also runs.
    pub fn complete_flight(&mut self) -> Result<(), CameraError> {
        if let Some(flight) = self.current_flight.clone() {
            self.remove_flight(flight.id);
            self.set_view(CameraView {
                destination: Some(CameraDestination::Cartesian(flight.destination)),
                orientation: Some(CameraOrientation::HeadingPitchRoll(flight.orientation)),
                ..CameraView::default()
            })?;
            let current =
                self.current_flight.clone().ok_or(CameraError("flight was cleared by its cancellation callback"))?;
            current.tween.complete(self)?;
            self.current_flight = None;
        }
        Ok(())
    }
    pub fn fly_home(&mut self, duration: Option<f64>) -> Result<(), CameraError> {
        let mode = self.mode;
        if mode == SceneMode::Morphing {
            self.morph_completion_requested.raise_event(&());
            return Ok(());
        }
        let mut options = CameraFlightOptions { duration, ..CameraFlightOptions::default() };
        options.view.end_transform = Some(Matrix4::IDENTITY);
        options.view.destination = Some(match mode {
            SceneMode::Scene2D => CameraDestination::Rectangle(Self::default_view_rectangle()),
            SceneMode::Scene3D => {
                let p = self.get_rectangle_camera_coordinates(Self::default_view_rectangle()).expect("settled mode");
                let magnitude = p.magnitude();
                CameraDestination::Cartesian(
                    p.normalize().multiply_by_scalar(magnitude + magnitude * Self::DEFAULT_VIEW_FACTOR),
                )
            }
            SceneMode::ColumbusView => {
                let p = Cartesian3::new(0., -1., 1.)
                    .normalize()
                    .multiply_by_scalar(5. * self.projection.ellipsoid().maximum_radius);
                options.view.convert = false;
                options.view.orientation = Some(CameraOrientation::HeadingPitchRoll(HeadingPitchRoll::new(
                    0.,
                    -p.normalize().z.js_acos(),
                    0.,
                )));
                CameraDestination::Cartesian(p)
            }
            SceneMode::Morphing => unreachable!(),
        });
        self.fly_to(options)
    }

    pub fn fly_to_bounding_sphere(
        &mut self,
        sphere: &BoundingSphere,
        offset: Option<HeadingPitchRange>,
        mut options: CameraFlightOptions,
    ) -> Result<(), CameraError> {
        let flat = self.mode == SceneMode::Scene2D || self.mode == SceneMode::ColumbusView;
        self.set_transform(Matrix4::IDENTITY)?;
        let offset = self.bounding_sphere_offset(sphere, offset);
        let position = if flat {
            Cartesian3::UNIT_Z.multiply_by_scalar(offset.range)
        } else {
            Self::offset_from_heading_pitch_range(offset)
        };
        let transform = Transforms::east_north_up_to_fixed_frame(sphere.center, &self.ellipsoid);
        let position = transform.multiply_by_point(position);
        let orientation = if flat {
            None
        } else {
            let direction = (sphere.center - position).normalize();
            let mut up = transform.multiply_by_point_as_vector(Cartesian3::UNIT_Z);
            if 1. - direction.dot(up).abs() < EPSILON6 {
                let q = Quaternion::from_axis_angle(direction, offset.heading);
                up = Matrix3::from_quaternion(&q)
                    .multiply_by_vector(Cartesian3::from_cartesian4(transform.get_column(1)));
            }
            let right = direction.cross(up);
            up = right.cross(direction).normalize();
            Some(CameraOrientation::DirectionUp { direction, up })
        };
        options.view.destination = Some(CameraDestination::Cartesian(position));
        options.view.orientation = orientation;
        // flyToBoundingSphere does not forward the convert option.
        options.view.convert = true;
        self.fly_to(options)
    }

    pub fn create_correct_position_tween(&mut self, duration: f64) -> Option<CameraCorrectionTween> {
        if self.mode != SceneMode::ColumbusView {
            return None;
        }
        let normal = self.world_to_camera_coordinates_vector(Cartesian3::UNIT_X);
        let scalar = -normal.dot(self.position) / normal.dot(self.direction);
        let center = self.position + self.direction.multiply_by_scalar(scalar);
        let center = self.camera_to_world_coordinates_point(center);
        let position = self.camera_to_world_coordinates_point(self.position);
        let CameraFrustum::Perspective(frustum) = self.frustum else {
            return None;
        };
        let tan_phi = (frustum.fovy() * 0.5).js_tan();
        let tan_theta = frustum.aspect_ratio * tan_phi;
        let distance = (position - center).magnitude();
        let max_x = (tan_theta * distance - self.max_coord.x).max(self.max_coord.x);
        let max_y = (tan_phi * distance - self.max_coord.y).max(self.max_coord.y);
        if (position.z < -max_x || position.z > max_x || position.y < -max_y || position.y > max_y)
            && (center.y < -max_x || center.y > max_x || center.z < -max_y || center.z > max_y)
        {
            let mut stop = position;
            if center.y > max_x {
                stop.y -= center.y - max_x;
            } else if center.y < -max_x {
                stop.y += -max_x - center.y;
            }
            if center.z > max_y {
                stop.z -= center.z - max_y;
            } else if center.z < -max_y {
                stop.z += -max_y - center.z;
            }
            Some(CameraCorrectionTween {
                duration,
                easing_function: EasingFunction::ExponentialOut,
                start: position,
                stop,
            })
        } else {
            None
        }
    }

    /// Cesium's `Camera.clone`: copies pose, transform and frustum into a new
    /// camera with default controls, 3D mode, and no listeners/flight state.
    pub fn clone_camera(&self) -> Self {
        let mut result = Self::new(CameraOptions {
            drawing_buffer_width: self.drawing_buffer_width,
            drawing_buffer_height: self.drawing_buffer_height,
            client_width: self.client_width,
            client_height: self.client_height,
            pixel_ratio: self.pixel_ratio,
            map_projection: Rc::clone(&self.projection),
            ellipsoid: self.ellipsoid,
            map_mode_2d: self.map_mode_2d,
            minimum_zoom_distance: self.minimum_zoom_distance,
            maximum_zoom_distance: self.maximum_zoom_distance,
            surface: self.surface.clone(),
        });
        result.position = self.position;
        result.direction = self.direction;
        result.up = self.up;
        result.right = self.right;
        result.transform = self.transform;
        result.transform_changed = true;
        result.frustum = self.frustum;
        result
    }
}
