//! Viewer camera: Cesium's heading/pitch/range `Camera.lookAt` behaviour,
//! built on `core` (`Transforms`, `Matrix4`, `Quaternion`, and
//! `IntersectionTests`). Cesium's `Camera` itself lives in Scene, which is
//! not ported yet; this is the thin viewer layer over the ported Core math.

use crate::core::intersection_tests::ray_ellipsoid;
use crate::core::math::{zero_to_two_pi, EPSILON10};
use crate::core::{Cartesian3, Cartographic, Ellipsoid, Interval, Matrix3, Matrix4, Quaternion, Ray, Transforms};
use std::f64::consts::FRAC_PI_2;

/// Cesium `Camera.offsetFromHeadingPitchRange` (Scene): the offset in the
/// local east-north-up frame. Pitch `-π/2` looks straight down; heading `0`
/// is north. Core has no equivalent because its `HeadingPitchRange` is only
/// the data type.
pub fn offset_from_heading_pitch_range(heading: f64, pitch: f64, range: f64) -> Cartesian3 {
    let pitch = pitch.clamp(-FRAC_PI_2, FRAC_PI_2);
    let heading = zero_to_two_pi(heading) - FRAC_PI_2;
    let pitch_quat = Quaternion::from_axis_angle(Cartesian3::UNIT_Y, -pitch);
    let heading_quat = Quaternion::from_axis_angle(Cartesian3::UNIT_Z, -heading);
    let rot = Matrix3::from_quaternion(&heading_quat.multiply(&pitch_quat));
    rot.multiply_by_vector(Cartesian3::UNIT_X).multiply_by_scalar(-range)
}

#[derive(Clone, Copy, Debug)]
pub struct CameraFrame {
    pub view: Matrix4,
    pub projection: Matrix4,
    pub view_projection: Matrix4,
    pub eye: Cartesian3,
    pub direction: Cartesian3,
    pub up: Cartesian3,
    pub right: Cartesian3,
    pub focus: Cartesian3,
    pub fov_y: f64,
    pub aspect: f64,
}

/// Cesium `Camera.lookAt` with a `HeadingPitchRange`, on the ellipsoid's
/// east-north-up frame at the focus point.
#[allow(clippy::too_many_arguments)]
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
    let focus = ellipsoid.cartographic_to_cartesian(&Cartographic::new(focus_lon, focus_lat, focus_height));
    let transform = Transforms::east_north_up_to_fixed_frame(focus, ellipsoid);
    let offset = offset_from_heading_pitch_range(heading, pitch, range);
    let eye = transform.multiply_by_point(offset);

    let mut direction = offset.negate();
    let mag = direction.magnitude();
    direction = if mag > 0.0 { direction.divide_by_scalar(mag) } else { Cartesian3::new(0.0, 0.0, -1.0) };
    let mut right = direction.cross(Cartesian3::UNIT_Z);
    if right.magnitude_squared() < EPSILON10 {
        right = Cartesian3::UNIT_X;
    }
    right = right.normalize();
    let local_up = right.cross(direction).normalize();

    let direction_w = transform.multiply_by_point_as_vector(direction).normalize();
    let right_w = transform.multiply_by_point_as_vector(right).normalize();
    let up_w = transform.multiply_by_point_as_vector(local_up).normalize();

    let view = Matrix4::compute_view(eye, direction_w, up_w, right_w);
    let projection = Matrix4::compute_perspective_field_of_view(fov_y, aspect.max(1e-6), near, far);
    CameraFrame {
        view_projection: projection.multiply(&view),
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
pub fn ray_from_ndc(frame: &CameraFrame, ndc_x: f64, ndc_y: f64) -> (Cartesian3, Cartesian3) {
    let tan_half = (frame.fov_y * 0.5).tan();
    let x = ndc_x * tan_half * frame.aspect;
    let y = ndc_y * tan_half;
    let direction =
        frame.direction.add(frame.right.multiply_by_scalar(x)).add(frame.up.multiply_by_scalar(y)).normalize();
    (frame.eye, direction)
}

#[derive(Clone, Copy, Debug)]
pub struct Pick {
    /// Distance along the ray, in units of the direction vector passed in.
    pub t: f64,
    pub longitude: f64,
    pub latitude: f64,
    pub height: f64,
    pub position: Cartesian3,
}

/// First non-negative hit of a ray with the ellipsoid, as a cartographic
/// point, via Core's `IntersectionTests.rayEllipsoid`.
pub fn pick_ray(origin: Cartesian3, direction: Cartesian3, ellipsoid: &Ellipsoid) -> Option<Pick> {
    // `Ray::new` normalizes the direction; Cesium's rayEllipsoid does not
    // require a unit vector and `t` is measured in units of the given
    // vector, so construct the ray without normalizing.
    let ray = Ray { origin, direction };
    let interval: Interval = ray_ellipsoid(&ray, ellipsoid)?;
    let t = if interval.start >= 0.0 {
        interval.start
    } else if interval.stop >= 0.0 {
        interval.stop
    } else {
        return None;
    };
    let position = ray.get_point(t);
    let c = ellipsoid.cartesian_to_cartographic(position)?;
    Some(Pick { t, longitude: c.longitude, latitude: c.latitude, height: c.height, position })
}
