//! CesiumJS `EllipsoidGeodesic`: the shortest path between two points on an
//! ellipsoid of revolution, by Vincenty's formulae.

use super::cartesian3::Cartesian3;
use super::cartographic::Cartographic;
use super::ellipsoid::Ellipsoid;
use super::js_math::JsMath;
use super::math::EPSILON12;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
struct Constants {
    a: f64,
    b: f64,
    f: f64,
    cosine_heading: f64,
    sine_heading: f64,
    tan_u: f64,
    cosine_u: f64,
    sine_u: f64,
    sigma: f64,
    sine_alpha: f64,
    sine_squared_alpha: f64,
    cosine_squared_alpha: f64,
    cosine_alpha: f64,
    u2_over_4: f64,
    u4_over_16: f64,
    u6_over_64: f64,
    u8_over_256: f64,
    a0: f64,
    a1: f64,
    a2: f64,
    a3: f64,
    distance_ratio: f64,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EllipsoidGeodesic {
    pub ellipsoid: Ellipsoid,
    pub start: Cartographic,
    pub end: Cartographic,
    /// Radians clockwise from north at the start.
    pub start_heading: f64,
    /// Radians clockwise from north at the end.
    pub end_heading: f64,
    /// Length in meters.
    pub surface_distance: f64,
    u_squared: f64,
    constants: Constants,
}

fn compute_c(f: f64, cosine_squared_alpha: f64) -> f64 {
    (f * cosine_squared_alpha * (4.0 + f * (4.0 - 3.0 * cosine_squared_alpha))) / 16.0
}

fn compute_delta_lambda(
    f: f64,
    sine_alpha: f64,
    cosine_squared_alpha: f64,
    sigma: f64,
    sine_sigma: f64,
    cosine_sigma: f64,
    cosine_twice_sigma_midpoint: f64,
) -> f64 {
    let c = compute_c(f, cosine_squared_alpha);
    (1.0 - c)
        * f
        * sine_alpha
        * (sigma
            + c * sine_sigma
                * (cosine_twice_sigma_midpoint
                    + c * cosine_sigma * (2.0 * cosine_twice_sigma_midpoint * cosine_twice_sigma_midpoint - 1.0)))
}

/// Iterations before giving up. Cesium has no cap and spins forever on
/// near-antipodal input, which [`EllipsoidGeodesic::new`] rejects up front.
const MAX_ITERATIONS: usize = 10_000;

/// Returns `(distance, start_heading, end_heading, u_squared)`.
fn vincenty_inverse(major: f64, minor: f64, lon1: f64, lat1: f64, lon2: f64, lat2: f64) -> (f64, f64, f64, f64) {
    let eff = (major - minor) / major;
    let l = lon2 - lon1;
    let u1 = ((1.0 - eff) * lat1.js_tan()).js_atan();
    let u2 = ((1.0 - eff) * lat2.js_tan()).js_atan();
    let cosine_u1 = u1.js_cos();
    let sine_u1 = u1.js_sin();
    let cosine_u2 = u2.js_cos();
    let sine_u2 = u2.js_sin();
    let cc = cosine_u1 * cosine_u2;
    let cs = cosine_u1 * sine_u2;
    let ss = sine_u1 * sine_u2;
    let sc = sine_u1 * cosine_u2;

    let mut lambda = l;
    let mut lambda_dot;
    let mut cosine_lambda;
    let mut sine_lambda;
    let mut sigma;
    let mut cosine_sigma;
    let mut sine_sigma;
    let mut cosine_squared_alpha;
    let mut cosine_twice_sigma_midpoint;
    let mut iterations = 0;
    loop {
        cosine_lambda = lambda.js_cos();
        sine_lambda = lambda.js_sin();
        let temp = cs - sc * cosine_lambda;
        sine_sigma = (cosine_u2 * cosine_u2 * sine_lambda * sine_lambda + temp * temp).sqrt();
        cosine_sigma = ss + cc * cosine_lambda;
        sigma = sine_sigma.js_atan2(cosine_sigma);
        let sine_alpha;
        if sine_sigma == 0.0 {
            sine_alpha = 0.0;
            cosine_squared_alpha = 1.0;
        } else {
            sine_alpha = (cc * sine_lambda) / sine_sigma;
            cosine_squared_alpha = 1.0 - sine_alpha * sine_alpha;
        }
        lambda_dot = lambda;
        cosine_twice_sigma_midpoint = cosine_sigma - (2.0 * ss) / cosine_squared_alpha;
        if !cosine_twice_sigma_midpoint.is_finite() {
            cosine_twice_sigma_midpoint = 0.0;
        }
        lambda = l + compute_delta_lambda(
            eff,
            sine_alpha,
            cosine_squared_alpha,
            sigma,
            sine_sigma,
            cosine_sigma,
            cosine_twice_sigma_midpoint,
        );
        iterations += 1;
        let delta = (lambda - lambda_dot).abs();
        // JS `while (|Δ| > ε)` also stops on NaN.
        if delta <= EPSILON12 || delta.is_nan() || iterations >= MAX_ITERATIONS {
            break;
        }
    }

    let u_squared = (cosine_squared_alpha * (major * major - minor * minor)) / (minor * minor);
    let a = 1.0 + (u_squared * (4096.0 + u_squared * (u_squared * (320.0 - 175.0 * u_squared) - 768.0))) / 16384.0;
    let b = (u_squared * (256.0 + u_squared * (u_squared * (74.0 - 47.0 * u_squared) - 128.0))) / 1024.0;
    let cos2 = cosine_twice_sigma_midpoint * cosine_twice_sigma_midpoint;
    let delta_sigma = b
        * sine_sigma
        * (cosine_twice_sigma_midpoint
            + (b * (cosine_sigma * (2.0 * cos2 - 1.0)
                - (b * cosine_twice_sigma_midpoint * (4.0 * sine_sigma * sine_sigma - 3.0) * (4.0 * cos2 - 3.0))
                    / 6.0))
                / 4.0);
    let distance = minor * a * (sigma - delta_sigma);
    // Cesium uses the sine and cosine from the last iteration, not the final lambda.
    let start_heading = (cosine_u2 * sine_lambda).js_atan2(cs - sc * cosine_lambda);
    let end_heading = (cosine_u1 * sine_lambda).js_atan2(cs * cosine_lambda - sc);
    (distance, start_heading, end_heading, u_squared)
}

fn set_constants(ellipsoid: &Ellipsoid, start_latitude: f64, start_heading: f64, u_squared: f64) -> Constants {
    let a = ellipsoid.maximum_radius;
    let b = ellipsoid.minimum_radius;
    let f = (a - b) / a;
    let cosine_heading = start_heading.js_cos();
    let sine_heading = start_heading.js_sin();
    let tan_u = (1.0 - f) * start_latitude.js_tan();
    let cosine_u = 1.0 / (1.0 + tan_u * tan_u).sqrt();
    let sine_u = cosine_u * tan_u;
    let sigma = tan_u.js_atan2(cosine_heading);
    let sine_alpha = cosine_u * sine_heading;
    let sine_squared_alpha = sine_alpha * sine_alpha;
    let cosine_squared_alpha = 1.0 - sine_squared_alpha;
    let cosine_alpha = cosine_squared_alpha.sqrt();
    let u2_over_4 = u_squared / 4.0;
    let u4_over_16 = u2_over_4 * u2_over_4;
    let u6_over_64 = u4_over_16 * u2_over_4;
    let u8_over_256 = u4_over_16 * u4_over_16;
    let a0 = 1.0 + u2_over_4 - (3.0 * u4_over_16) / 4.0 + (5.0 * u6_over_64) / 4.0 - (175.0 * u8_over_256) / 64.0;
    let a1 = 1.0 - u2_over_4 + (15.0 * u4_over_16) / 8.0 - (35.0 * u6_over_64) / 8.0;
    let a2 = 1.0 - 3.0 * u2_over_4 + (35.0 * u4_over_16) / 4.0;
    let a3 = 1.0 - 5.0 * u2_over_4;
    let distance_ratio = a0 * sigma
        - (a1 * (2.0 * sigma).js_sin() * u2_over_4) / 2.0
        - (a2 * (4.0 * sigma).js_sin() * u4_over_16) / 16.0
        - (a3 * (6.0 * sigma).js_sin() * u6_over_64) / 48.0
        - ((8.0 * sigma).js_sin() * 5.0 * u8_over_256) / 512.0;
    Constants {
        a,
        b,
        f,
        cosine_heading,
        sine_heading,
        tan_u,
        cosine_u,
        sine_u,
        sigma,
        sine_alpha,
        sine_squared_alpha,
        cosine_squared_alpha,
        cosine_alpha,
        u2_over_4,
        u4_over_16,
        u6_over_64,
        u8_over_256,
        a0,
        a1,
        a2,
        a3,
        distance_ratio,
    }
}

impl EllipsoidGeodesic {
    /// `None` within 0.0125 rad of antipodal, Cesium's (debug-build) limit,
    /// where Vincenty's iteration may not converge.
    pub fn new(start: &Cartographic, end: &Cartographic, ellipsoid: &Ellipsoid) -> Option<Self> {
        let first = ellipsoid.cartographic_to_cartesian(start).normalize();
        let last = ellipsoid.cartographic_to_cartesian(end).normalize();
        if (Cartesian3::angle_between(&first, last).abs() - std::f64::consts::PI).abs() < 0.0125 {
            return None;
        }
        let (distance, start_heading, end_heading, u_squared) = vincenty_inverse(
            ellipsoid.maximum_radius,
            ellipsoid.minimum_radius,
            start.longitude,
            start.latitude,
            end.longitude,
            end.latitude,
        );
        Some(Self {
            ellipsoid: *ellipsoid,
            start: Cartographic::new(start.longitude, start.latitude, 0.0),
            end: Cartographic::new(end.longitude, end.latitude, 0.0),
            start_heading,
            end_heading,
            surface_distance: distance,
            u_squared,
            constants: set_constants(ellipsoid, start.latitude, start_heading, u_squared),
        })
    }

    /// `false` (and unchanged) for near-antipodal points.
    pub fn set_end_points(&mut self, start: &Cartographic, end: &Cartographic) -> bool {
        match Self::new(start, end, &self.ellipsoid) {
            Some(g) => {
                *self = g;
                true
            }
            None => false,
        }
    }

    pub fn interpolate_using_fraction(&self, fraction: f64) -> Cartographic {
        self.interpolate_using_surface_distance(self.surface_distance * fraction)
    }

    pub fn interpolate_using_surface_distance(&self, distance: f64) -> Cartographic {
        let c = &self.constants;
        let s = c.distance_ratio + distance / c.b;
        let cosine2_s = (2.0 * s).js_cos();
        let cosine4_s = (4.0 * s).js_cos();
        let cosine6_s = (6.0 * s).js_cos();
        let sine2_s = (2.0 * s).js_sin();
        let sine4_s = (4.0 * s).js_sin();
        let sine6_s = (6.0 * s).js_sin();
        let sine8_s = (8.0 * s).js_sin();
        let s2 = s * s;
        let s3 = s * s2;
        let u8 = c.u8_over_256;
        let u2 = c.u2_over_4;
        let u6 = c.u6_over_64;
        let u4 = c.u4_over_16;
        let mut sigma = (2.0 * s3 * u8 * cosine2_s) / 3.0
            + s * (1.0 - u2 + (7.0 * u4) / 4.0 - (15.0 * u6) / 4.0 + (579.0 * u8) / 64.0
                - (u4 - (15.0 * u6) / 4.0 + (187.0 * u8) / 16.0) * cosine2_s
                - ((5.0 * u6) / 4.0 - (115.0 * u8) / 16.0) * cosine4_s
                - (29.0 * u8 * cosine6_s) / 16.0)
            + (u2 / 2.0 - u4 + (71.0 * u6) / 32.0 - (85.0 * u8) / 16.0) * sine2_s
            + ((5.0 * u4) / 16.0 - (5.0 * u6) / 4.0 + (383.0 * u8) / 96.0) * sine4_s
            - s2 * ((u6 - (11.0 * u8) / 2.0) * sine2_s + (5.0 * u8 * sine4_s) / 2.0)
            + ((29.0 * u6) / 96.0 - (29.0 * u8) / 16.0) * sine6_s
            + (539.0 * u8 * sine8_s) / 1536.0;
        let theta = (sigma.js_sin() * c.cosine_alpha).js_asin();
        let latitude = ((c.a / c.b) * theta.js_tan()).js_atan();
        sigma -= c.sigma;
        let cosine_twice_sigma_midpoint = (2.0 * c.sigma + sigma).js_cos();
        let sine_sigma = sigma.js_sin();
        let cosine_sigma = sigma.js_cos();
        let cc = c.cosine_u * cosine_sigma;
        let ss = c.sine_u * sine_sigma;
        let lambda = (sine_sigma * c.sine_heading).js_atan2(cc - ss * c.cosine_heading);
        let l = lambda
            - compute_delta_lambda(
                c.f,
                c.sine_alpha,
                c.cosine_squared_alpha,
                sigma,
                sine_sigma,
                cosine_sigma,
                cosine_twice_sigma_midpoint,
            );
        Cartographic::new(self.start.longitude + l, latitude, 0.0)
    }
}
