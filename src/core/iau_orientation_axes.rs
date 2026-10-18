//! CesiumJS `IauOrientationAxes`: the rotation from ICRF to a body's fixed
//! frame, from IAU orientation parameters.

use super::cartesian3::Cartesian3;
use super::iau2000_orientation;
use super::iau_orientation_parameters::IauOrientationParameters;
use super::js_math::JsMath;
use super::julian_date::JulianDate;
use super::math::{zero_to_two_pi, PI_OVER_TWO};
use super::matrix3::Matrix3;
use super::quaternion::Quaternion;

#[derive(Clone, Copy, Debug)]
pub struct IauOrientationAxes {
    compute_function: fn(&JulianDate) -> IauOrientationParameters,
}

impl Default for IauOrientationAxes {
    /// The Moon, as in Cesium.
    fn default() -> Self {
        Self::new(iau2000_orientation::compute_moon)
    }
}

fn compute_rotation_matrix(alpha: f64, delta: f64) -> Matrix3 {
    let x_axis = Cartesian3::new((alpha + PI_OVER_TWO).js_cos(), (alpha + PI_OVER_TWO).js_sin(), 0.0);
    let cos_dec = delta.js_cos();
    let z_axis = Cartesian3::new(cos_dec * alpha.js_cos(), cos_dec * alpha.js_sin(), delta.js_sin());
    let y_axis = z_axis.cross(x_axis);
    Matrix3::from_column_major_array([
        x_axis.x, y_axis.x, z_axis.x, x_axis.y, y_axis.y, z_axis.y, x_axis.z, y_axis.z, z_axis.z,
    ])
}

impl IauOrientationAxes {
    pub fn new(compute_function: fn(&JulianDate) -> IauOrientationParameters) -> Self {
        Self { compute_function }
    }

    /// The rotation from ICRF to the body-fixed frame at a TAI date.
    pub fn evaluate(&self, date: &JulianDate) -> Matrix3 {
        let alpha_delta_w = (self.compute_function)(date);
        let prec_mtx = compute_rotation_matrix(alpha_delta_w.right_ascension, alpha_delta_w.declination);
        let rot = zero_to_two_pi(alpha_delta_w.rotation);
        let quat = Quaternion::from_axis_angle(Cartesian3::UNIT_Z, rot);
        let rot_mtx = Matrix3::from_quaternion(&quat.conjugate());
        rot_mtx.multiply(&prec_mtx)
    }
}
