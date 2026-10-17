//! Vincenty ellipsoid geodesic. Ported from CesiumJS `EllipsoidGeodesic`.

use crate::ellipsoid::{cartographic_to_cartesian, Ellipsoid};
use crate::math::{Vec3, EPSILON12, PI};

#[derive(Clone, Copy, Debug)]
struct Constants {
    a: f64,
    b: f64,
    f: f64,
    cosine_heading: f64,
    sine_heading: f64,
    cosine_u: f64,
    sine_u: f64,
    sigma: f64,
    sine_alpha: f64,
    cosine_squared_alpha: f64,
    cosine_alpha: f64,
    u2_over_4: f64,
    u4_over_16: f64,
    u6_over_64: f64,
    u8_over_256: f64,
    distance_ratio: f64,
}

#[derive(Clone, Copy, Debug)]
pub struct Geodesic {
    pub distance: f64,
    pub start_heading: f64,
    pub end_heading: f64,
    start_lon: f64,
    start_dir: Vec3,
    end_dir: Vec3,
    spherical: bool,
    constants: Constants,
}

fn compute_c(f: f64, cosine_squared_alpha: f64) -> f64 {
    f * cosine_squared_alpha * (4.0 + f * (4.0 - 3.0 * cosine_squared_alpha)) / 16.0
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
                    + c * cosine_sigma
                        * (2.0 * cosine_twice_sigma_midpoint * cosine_twice_sigma_midpoint - 1.0)))
}

fn slerp(a: Vec3, b: Vec3, t: f64) -> Vec3 {
    let a = a.normalize();
    let b = b.normalize();
    let d = a.dot(b).clamp(-1.0, 1.0);
    if d > 0.9995 {
        return a.lerp(b, t).normalize();
    }
    let theta = d.acos();
    let s = theta.sin();
    if s.abs() < 1e-12 {
        return a;
    }
    a * (((1.0 - t) * theta).sin() / s) + b * ((t * theta).sin() / s)
}

impl Geodesic {
    pub fn new(
        ellipsoid: &Ellipsoid,
        start_lon: f64,
        start_lat: f64,
        end_lon: f64,
        end_lat: f64,
    ) -> Self {
        let start = cartographic_to_cartesian(ellipsoid, start_lon, start_lat, 0.0);
        let end = cartographic_to_cartesian(ellipsoid, end_lon, end_lat, 0.0);
        let start_dir = start.normalize();
        let end_dir = end.normalize();
        let angle = start_dir.dot(end_dir).clamp(-1.0, 1.0).acos();

        if angle < 1e-12 || (angle - PI).abs() < 0.0125 {
            let distance = angle * ellipsoid.maximum_radius;
            return Self {
                distance,
                start_heading: 0.0,
                end_heading: 0.0,
                start_lon,
                start_dir,
                end_dir,
                spherical: true,
                constants: Constants {
                    a: ellipsoid.maximum_radius,
                    b: ellipsoid.minimum_radius,
                    f: 0.0,
                    cosine_heading: 1.0,
                    sine_heading: 0.0,
                    cosine_u: 1.0,
                    sine_u: 0.0,
                    sigma: 0.0,
                    sine_alpha: 0.0,
                    cosine_squared_alpha: 1.0,
                    cosine_alpha: 1.0,
                    u2_over_4: 0.0,
                    u4_over_16: 0.0,
                    u6_over_64: 0.0,
                    u8_over_256: 0.0,
                    distance_ratio: 0.0,
                },
            };
        }

        let (distance, start_heading, end_heading, u_squared) = vincenty_inverse(
            ellipsoid.maximum_radius,
            ellipsoid.minimum_radius,
            start_lon,
            start_lat,
            end_lon,
            end_lat,
        );
        let constants = set_constants(
            ellipsoid.maximum_radius,
            ellipsoid.minimum_radius,
            start_lat,
            start_heading,
            u_squared,
        );
        Self {
            distance,
            start_heading,
            end_heading,
            start_lon,
            start_dir,
            end_dir,
            spherical: false,
            constants,
        }
    }

    pub fn interpolate(&self, fraction: f64) -> (f64, f64) {
        self.interpolate_distance(self.distance * fraction)
    }

    pub fn interpolate_distance(&self, distance: f64) -> (f64, f64) {
        if self.spherical || self.distance.abs() < 1e-6 {
            let t = if self.distance.abs() < 1e-6 {
                0.0
            } else {
                distance / self.distance
            };
            let dir = slerp(self.start_dir, self.end_dir, t.clamp(0.0, 1.0));
            let longitude = dir.y.atan2(dir.x);
            let latitude = (dir.z).clamp(-1.0, 1.0).asin();
            return (longitude, latitude);
        }

        let c = &self.constants;
        let s = c.distance_ratio + distance / c.b;
        let cosine2s = (2.0 * s).cos();
        let cosine4s = (4.0 * s).cos();
        let cosine6s = (6.0 * s).cos();
        let sine2s = (2.0 * s).sin();
        let sine4s = (4.0 * s).sin();
        let sine6s = (6.0 * s).sin();
        let sine8s = (8.0 * s).sin();
        let s2 = s * s;
        let s3 = s * s2;
        let u8 = c.u8_over_256;
        let u2 = c.u2_over_4;
        let u6 = c.u6_over_64;
        let u4 = c.u4_over_16;

        let mut sigma = (2.0 * s3 * u8 * cosine2s) / 3.0
            + s * (1.0 - u2
                + (7.0 * u4) / 4.0
                + (15.0 * u6) / -4.0
                + (579.0 * u8) / 64.0
                - (u4 - (15.0 * u6) / 4.0 + (187.0 * u8) / 16.0) * cosine2s
                - ((5.0 * u6) / 4.0 - (115.0 * u8) / 16.0) * cosine4s
                - (29.0 * u8 * cosine6s) / 16.0)
            + (u2 / 2.0 - u4 + (71.0 * u6) / 32.0 - (85.0 * u8) / 16.0) * sine2s
            + ((5.0 * u4) / 16.0 - (5.0 * u6) / 4.0 + (383.0 * u8) / 96.0) * sine4s
            - s2 * ((u6 - (11.0 * u8) / 2.0) * sine2s + (5.0 * u8 * sine4s) / 2.0)
            + ((29.0 * u6) / 96.0 - (29.0 * u8) / 16.0) * sine6s
            + (539.0 * u8 * sine8s) / 1536.0;

        let theta = (sigma.sin() * c.cosine_alpha).clamp(-1.0, 1.0).asin();
        let latitude = ((c.a / c.b) * theta.tan()).atan();
        sigma -= c.sigma;

        let cosine_twice = (2.0 * c.sigma + sigma).cos();
        let sine_sigma = sigma.sin();
        let cosine_sigma = sigma.cos();
        let cc = c.cosine_u * cosine_sigma;
        let ss = c.sine_u * sine_sigma;
        let lambda = (sine_sigma * c.sine_heading).atan2(cc - ss * c.cosine_heading);
        let l = lambda
            - compute_delta_lambda(
                c.f,
                c.sine_alpha,
                c.cosine_squared_alpha,
                sigma,
                sine_sigma,
                cosine_sigma,
                cosine_twice,
            );
        (self.start_lon + l, latitude)
    }
}

#[allow(unused_assignments)]
fn vincenty_inverse(
    major: f64,
    minor: f64,
    first_longitude: f64,
    first_latitude: f64,
    second_longitude: f64,
    second_latitude: f64,
) -> (f64, f64, f64, f64) {
    let eff = (major - minor) / major;
    let l = second_longitude - first_longitude;
    let u1 = ((1.0 - eff) * first_latitude.tan()).atan();
    let u2 = ((1.0 - eff) * second_latitude.tan()).atan();
    let cosine_u1 = u1.cos();
    let sine_u1 = u1.sin();
    let cosine_u2 = u2.cos();
    let sine_u2 = u2.sin();
    let cc = cosine_u1 * cosine_u2;
    let cs = cosine_u1 * sine_u2;
    let ss = sine_u1 * sine_u2;
    let sc = sine_u1 * cosine_u2;

    let mut lambda = l;
    let mut sigma = 0.0;
    let mut cosine_sigma = 1.0;
    let mut sine_sigma = 0.0;
    let mut cosine_squared_alpha = 1.0;
    let mut cosine_twice_sigma_midpoint = 0.0;
    let mut sine_alpha = 0.0;

    for _ in 0..100 {
        let cosine_lambda = lambda.cos();
        let sine_lambda = lambda.sin();
        let temp = cs - sc * cosine_lambda;
        sine_sigma = (cosine_u2 * cosine_u2 * sine_lambda * sine_lambda + temp * temp).sqrt();
        cosine_sigma = ss + cc * cosine_lambda;
        sigma = sine_sigma.atan2(cosine_sigma);
        if sine_sigma == 0.0 {
            sine_alpha = 0.0;
            cosine_squared_alpha = 1.0;
        } else {
            sine_alpha = (cc * sine_lambda) / sine_sigma;
            cosine_squared_alpha = 1.0 - sine_alpha * sine_alpha;
        }
        let lambda_dot = lambda;
        cosine_twice_sigma_midpoint = if cosine_squared_alpha.abs() < 1e-15 {
            0.0
        } else {
            cosine_sigma - (2.0 * ss) / cosine_squared_alpha
        };
        if !cosine_twice_sigma_midpoint.is_finite() {
            cosine_twice_sigma_midpoint = 0.0;
        }
        lambda = l
            + compute_delta_lambda(
                eff,
                sine_alpha,
                cosine_squared_alpha,
                sigma,
                sine_sigma,
                cosine_sigma,
                cosine_twice_sigma_midpoint,
            );
        if (lambda - lambda_dot).abs() <= EPSILON12 {
            break;
        }
    }

    let u_squared = (cosine_squared_alpha * (major * major - minor * minor)) / (minor * minor);
    let a = 1.0
        + (u_squared * (4096.0 + u_squared * (u_squared * (320.0 - 175.0 * u_squared) - 768.0)))
            / 16384.0;
    let b = (u_squared * (256.0 + u_squared * (u_squared * (74.0 - 47.0 * u_squared) - 128.0)))
        / 1024.0;
    let cosine_squared_twice = cosine_twice_sigma_midpoint * cosine_twice_sigma_midpoint;
    let delta_sigma = b
        * sine_sigma
        * (cosine_twice_sigma_midpoint
            + (b
                * (cosine_sigma * (2.0 * cosine_squared_twice - 1.0)
                    - (b
                        * cosine_twice_sigma_midpoint
                        * (4.0 * sine_sigma * sine_sigma - 3.0)
                        * (4.0 * cosine_squared_twice - 3.0))
                        / 6.0))
                / 4.0);
    let distance = minor * a * (sigma - delta_sigma);
    let start_heading = (cosine_u2 * lambda.sin()).atan2(cs - sc * lambda.cos());
    let end_heading = (cosine_u1 * lambda.sin()).atan2(cs * lambda.cos() - sc);
    (distance, start_heading, end_heading, u_squared)
}

fn set_constants(a: f64, b: f64, start_lat: f64, start_heading: f64, u_squared: f64) -> Constants {
    let f = (a - b) / a;
    let cosine_heading = start_heading.cos();
    let sine_heading = start_heading.sin();
    let tan_u = (1.0 - f) * start_lat.tan();
    let cosine_u = 1.0 / (1.0 + tan_u * tan_u).sqrt();
    let sine_u = cosine_u * tan_u;
    let sigma = tan_u.atan2(cosine_heading);
    let sine_alpha = cosine_u * sine_heading;
    let sine_squared_alpha = sine_alpha * sine_alpha;
    let cosine_squared_alpha = 1.0 - sine_squared_alpha;
    let cosine_alpha = cosine_squared_alpha.max(0.0).sqrt();
    let u2_over_4 = u_squared / 4.0;
    let u4_over_16 = u2_over_4 * u2_over_4;
    let u6_over_64 = u4_over_16 * u2_over_4;
    let u8_over_256 = u4_over_16 * u4_over_16;
    let a0 = 1.0 + u2_over_4 - (3.0 * u4_over_16) / 4.0 + (5.0 * u6_over_64) / 4.0
        - (175.0 * u8_over_256) / 64.0;
    let a1 = 1.0 - u2_over_4 + (15.0 * u4_over_16) / 8.0 - (35.0 * u6_over_64) / 8.0;
    let a2 = 1.0 - 3.0 * u2_over_4 + (35.0 * u4_over_16) / 4.0;
    let a3 = 1.0 - 5.0 * u2_over_4;
    let distance_ratio = a0 * sigma
        - (a1 * (2.0 * sigma).sin() * u2_over_4) / 2.0
        - (a2 * (4.0 * sigma).sin() * u4_over_16) / 16.0
        - (a3 * (6.0 * sigma).sin() * u6_over_64) / 48.0
        - ((8.0 * sigma).sin() * 5.0 * u8_over_256) / 512.0;
    Constants {
        a,
        b,
        f,
        cosine_heading,
        sine_heading,
        cosine_u,
        sine_u,
        sigma,
        sine_alpha,
        cosine_squared_alpha,
        cosine_alpha,
        u2_over_4,
        u4_over_16,
        u6_over_64,
        u8_over_256,
        distance_ratio,
    }
}
