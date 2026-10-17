//! East-north-up frames and Cesium's heading/pitch/range camera.

use crate::ellipsoid::{cartographic_to_cartesian, geodetic_surface_normal, Ellipsoid};
use crate::math::{
    clamp, equals_epsilon, mat3_from_quaternion, mat3_mul_vec, mat4_mul, mat4_mul_dir,
    mat4_mul_point, sign, zero_to_two_pi, Mat4, Quat, Vec3, EPSILON10, EPSILON14, PI_OVER_TWO,
};

/// Cesium `Transforms.eastNorthUpToFixedFrame` axes: east, north, up.
pub fn enu_axes(origin: Vec3, ellipsoid: &Ellipsoid) -> (Vec3, Vec3, Vec3) {
    let at_center = equals_epsilon(origin.x, 0.0, EPSILON14)
        && equals_epsilon(origin.y, 0.0, EPSILON14)
        && equals_epsilon(origin.z, 0.0, EPSILON14);
    if at_center {
        return (Vec3::UNIT_Y, Vec3::new(-1.0, 0.0, 0.0), Vec3::UNIT_Z);
    }
    if equals_epsilon(origin.x, 0.0, EPSILON14) && equals_epsilon(origin.y, 0.0, EPSILON14) {
        let s = sign(origin.z);
        return (
            Vec3::UNIT_Y,
            Vec3::new(-s, 0.0, 0.0),
            Vec3::new(0.0, 0.0, s),
        );
    }
    let up = geodetic_surface_normal(ellipsoid, origin);
    let east = Vec3::new(-origin.y, origin.x, 0.0).normalize();
    let north = up.cross(east);
    (east, north, up)
}

pub fn enu_to_fixed(origin: Vec3, ellipsoid: &Ellipsoid) -> Mat4 {
    let (east, north, up) = enu_axes(origin, ellipsoid);
    let mut m = [0.0; 16];
    m[0] = east.x;
    m[1] = east.y;
    m[2] = east.z;
    m[4] = north.x;
    m[5] = north.y;
    m[6] = north.z;
    m[8] = up.x;
    m[9] = up.y;
    m[10] = up.z;
    m[12] = origin.x;
    m[13] = origin.y;
    m[14] = origin.z;
    m[15] = 1.0;
    m
}

/// Cesium `offsetFromHeadingPitchRange`. Offset is in the local ENU frame.
/// Pitch `-π/2` looks straight down; heading `0` is north.
pub fn offset_from_heading_pitch_range(heading: f64, pitch: f64, range: f64) -> Vec3 {
    let pitch = clamp(pitch, -PI_OVER_TWO, PI_OVER_TWO);
    let heading = zero_to_two_pi(heading) - PI_OVER_TWO;
    let pitch_quat = Quat::from_axis_angle(Vec3::UNIT_Y, -pitch);
    let heading_quat = Quat::from_axis_angle(Vec3::UNIT_Z, -heading);
    let rot = mat3_from_quaternion(heading_quat.mul(pitch_quat));
    -mat3_mul_vec(&rot, Vec3::UNIT_X) * range
}

/// Cesium `Matrix4.computePerspectiveFieldOfView`.
pub fn perspective_fov(fov_y: f64, aspect: f64, near: f64, far: f64) -> Mat4 {
    let column1_row1 = 1.0 / (fov_y * 0.5).tan();
    let column0_row0 = column1_row1 / aspect;
    let column2_row2 = (far + near) / (near - far);
    let column3_row2 = (2.0 * far * near) / (near - far);
    let mut m = [0.0; 16];
    m[0] = column0_row0;
    m[5] = column1_row1;
    m[10] = column2_row2;
    m[11] = -1.0;
    m[14] = column3_row2;
    m
}

/// Cesium `Matrix4.computeView`. `direction` is the look vector.
pub fn compute_view(position: Vec3, direction: Vec3, up: Vec3, right: Vec3) -> Mat4 {
    let mut m = [0.0; 16];
    m[0] = right.x;
    m[1] = up.x;
    m[2] = -direction.x;
    m[4] = right.y;
    m[5] = up.y;
    m[6] = -direction.y;
    m[8] = right.z;
    m[9] = up.z;
    m[10] = -direction.z;
    m[12] = -right.dot(position);
    m[13] = -up.dot(position);
    m[14] = direction.dot(position);
    m[15] = 1.0;
    m
}

#[derive(Clone, Copy, Debug)]
pub struct CameraFrame {
    pub view: Mat4,
    pub projection: Mat4,
    pub view_projection: Mat4,
    pub eye: Vec3,
    pub direction: Vec3,
    pub up: Vec3,
    pub right: Vec3,
    pub focus: Vec3,
    pub fov_y: f64,
    pub aspect: f64,
}

/// Cesium `Camera.lookAt` with a `HeadingPitchRange` on the WGS84 ENU frame.
pub fn look_at(
    ellipsoid: &Ellipsoid,
    focus_lon: f64,
    focus_lat: f64,
    focus_height: f64,
    heading: f64,
    pitch: f64,
    range: f64,
    fov_y: f64,
    aspect: f64,
    near: f64,
    far: f64,
) -> CameraFrame {
    let focus = cartographic_to_cartesian(ellipsoid, focus_lon, focus_lat, focus_height);
    let transform = enu_to_fixed(focus, ellipsoid);
    let offset = offset_from_heading_pitch_range(heading, pitch, range);
    let eye = mat4_mul_point(&transform, offset);

    let mut direction = -offset;
    let mag = direction.magnitude();
    direction = if mag > 0.0 {
        direction / mag
    } else {
        Vec3::new(0.0, 0.0, -1.0)
    };
    let mut right = direction.cross(Vec3::UNIT_Z);
    if right.magnitude_squared() < EPSILON10 {
        right = Vec3::UNIT_X;
    }
    right = right.normalize();
    let local_up = right.cross(direction).normalize();

    let direction_w = mat4_mul_dir(&transform, direction).normalize();
    let right_w = mat4_mul_dir(&transform, right).normalize();
    let up_w = mat4_mul_dir(&transform, local_up).normalize();

    let view = compute_view(eye, direction_w, up_w, right_w);
    let projection = perspective_fov(fov_y, aspect.max(1e-6), near, far);
    CameraFrame {
        view_projection: mat4_mul(&projection, &view),
        view,
        projection,
        eye,
        direction: direction_w,
        up: up_w,
        right: right_w,
        focus,
        fov_y,
        aspect,
    }
}

/// Pick ray through an NDC point. `+x` is right, `+y` is up.
pub fn ray_from_ndc(frame: &CameraFrame, ndc_x: f64, ndc_y: f64) -> (Vec3, Vec3) {
    let tan_half = (frame.fov_y * 0.5).tan();
    let x = ndc_x * tan_half * frame.aspect;
    let y = ndc_y * tan_half;
    let direction = (frame.direction + frame.right * x + frame.up * y).normalize();
    (frame.eye, direction)
}
