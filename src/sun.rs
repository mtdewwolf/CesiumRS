//! Approximate solar direction in ECEF. A compact stand-in for Cesium's Simon 1994 ephemeris.

use crate::math::Vec3;

/// Unit vector from the geocenter toward the sun, in Earth-fixed coordinates.
/// `jd` is the Julian date (UTC).
pub fn sun_direction_ecef(jd: f64) -> Vec3 {
    let n = jd - 2_451_545.0;
    let mean_longitude = (280.460 + 0.985_647_4 * n).rem_euclid(360.0);
    let mean_anomaly = (357.528 + 0.985_600_3 * n).rem_euclid(360.0).to_radians();
    let lambda = (mean_longitude + 1.915 * mean_anomaly.sin() + 0.020 * (2.0 * mean_anomaly).sin()).to_radians();
    let obliquity = (23.439 - 0.000_000_4 * n).to_radians();

    let x = lambda.cos();
    let y = obliquity.cos() * lambda.sin();
    let z = obliquity.sin() * lambda.sin();

    let gmst = (280.460_618_37 + 360.985_647_366_29 * n).rem_euclid(360.0).to_radians();
    let (c, s) = (gmst.cos(), gmst.sin());
    Vec3::new(c * x + s * y, -s * x + c * y, z).normalize()
}
