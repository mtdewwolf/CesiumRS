//! CesiumJS `EllipsoidRhumbLine`: a line of constant heading on the ellipsoid.

use super::cartographic::Cartographic;
use super::ellipsoid::Ellipsoid;
use super::js_math::JsMath;
use super::math::{
    equals_epsilon, negative_pi_to_pi, sign, EPSILON10, EPSILON12, EPSILON14, EPSILON8, PI, PI_OVER_TWO,
};

/// Meridional arc length from the equator to `latitude`.
fn calculate_m(ellipticity: f64, major: f64, latitude: f64) -> f64 {
    if ellipticity == 0.0 {
        return major * latitude;
    }
    let e2 = ellipticity * ellipticity;
    let e4 = e2 * e2;
    let e6 = e4 * e2;
    let e8 = e6 * e2;
    let e10 = e8 * e2;
    let e12 = e10 * e2;
    let phi = latitude;
    let sin2_phi = (2.0 * phi).js_sin();
    let sin4_phi = (4.0 * phi).js_sin();
    let sin6_phi = (6.0 * phi).js_sin();
    let sin8_phi = (8.0 * phi).js_sin();
    let sin10_phi = (10.0 * phi).js_sin();
    let sin12_phi = (12.0 * phi).js_sin();
    major
        * ((1.0
            - e2 / 4.0
            - (3.0 * e4) / 64.0
            - (5.0 * e6) / 256.0
            - (175.0 * e8) / 16384.0
            - (441.0 * e10) / 65536.0
            - (4851.0 * e12) / 1048576.0)
            * phi
            - ((3.0 * e2) / 8.0
                + (3.0 * e4) / 32.0
                + (45.0 * e6) / 1024.0
                + (105.0 * e8) / 4096.0
                + (2205.0 * e10) / 131072.0
                + (6237.0 * e12) / 524288.0)
                * sin2_phi
            + ((15.0 * e4) / 256.0
                + (45.0 * e6) / 1024.0
                + (525.0 * e8) / 16384.0
                + (1575.0 * e10) / 65536.0
                + (155925.0 * e12) / 8388608.0)
                * sin4_phi
            - ((35.0 * e6) / 3072.0 + (175.0 * e8) / 12288.0 + (3675.0 * e10) / 262144.0 + (13475.0 * e12) / 1048576.0)
                * sin6_phi
            + ((315.0 * e8) / 131072.0 + (2205.0 * e10) / 524288.0 + (43659.0 * e12) / 8388608.0) * sin8_phi
            - ((693.0 * e10) / 1310720.0 + (6237.0 * e12) / 5242880.0) * sin10_phi
            + ((1001.0 * e12) / 8388608.0) * sin12_phi)
}

/// Latitude whose meridional arc length is `m`.
fn calculate_inverse_m(m: f64, ellipticity: f64, major: f64) -> f64 {
    let d = m / major;
    if ellipticity == 0.0 {
        return d;
    }
    let d2 = d * d;
    let d3 = d2 * d;
    let d4 = d3 * d;
    let e = ellipticity;
    let e2 = e * e;
    let e4 = e2 * e2;
    let e6 = e4 * e2;
    let e8 = e6 * e2;
    let e10 = e8 * e2;
    let e12 = e10 * e2;
    let sin2_d = (2.0 * d).js_sin();
    let cos2_d = (2.0 * d).js_cos();
    let sin4_d = (4.0 * d).js_sin();
    let cos4_d = (4.0 * d).js_cos();
    let sin6_d = (6.0 * d).js_sin();
    let cos6_d = (6.0 * d).js_cos();
    let sin8_d = (8.0 * d).js_sin();
    let cos8_d = (8.0 * d).js_cos();
    let sin10_d = (10.0 * d).js_sin();
    let cos10_d = (10.0 * d).js_cos();
    let sin12_d = (12.0 * d).js_sin();
    d + (d * e2) / 4.0
        + (7.0 * d * e4) / 64.0
        + (15.0 * d * e6) / 256.0
        + (579.0 * d * e8) / 16384.0
        + (1515.0 * d * e10) / 65536.0
        + (16837.0 * d * e12) / 1048576.0
        + ((3.0 * d * e4) / 16.0 + (45.0 * d * e6) / 256.0
            - (d * (32.0 * d2 - 561.0) * e8) / 4096.0
            - (d * (232.0 * d2 - 1677.0) * e10) / 16384.0
            + (d * (399985.0 - 90560.0 * d2 + 512.0 * d4) * e12) / 5242880.0)
            * cos2_d
        + ((21.0 * d * e6) / 256.0 + (483.0 * d * e8) / 4096.0
            - (d * (224.0 * d2 - 1969.0) * e10) / 16384.0
            - (d * (33152.0 * d2 - 112599.0) * e12) / 1048576.0)
            * cos4_d
        + ((151.0 * d * e8) / 4096.0 + (4681.0 * d * e10) / 65536.0 + (1479.0 * d * e12) / 16384.0
            - (453.0 * d3 * e12) / 32768.0)
            * cos6_d
        + ((1097.0 * d * e10) / 65536.0 + (42783.0 * d * e12) / 1048576.0) * cos8_d
        + ((8011.0 * d * e12) / 1048576.0) * cos10_d
        + ((3.0 * e2) / 8.0 + (3.0 * e4) / 16.0 + (213.0 * e6) / 2048.0 - (3.0 * d2 * e6) / 64.0
            + (255.0 * e8) / 4096.0
            - (33.0 * d2 * e8) / 512.0
            + (20861.0 * e10) / 524288.0
            - (33.0 * d2 * e10) / 512.0
            + (d4 * e10) / 1024.0
            + (28273.0 * e12) / 1048576.0
            - (471.0 * d2 * e12) / 8192.0
            + (9.0 * d4 * e12) / 4096.0)
            * sin2_d
        + ((21.0 * e4) / 256.0 + (21.0 * e6) / 256.0 + (533.0 * e8) / 8192.0 - (21.0 * d2 * e8) / 512.0
            + (197.0 * e10) / 4096.0
            - (315.0 * d2 * e10) / 4096.0
            + (584039.0 * e12) / 16777216.0
            - (12517.0 * d2 * e12) / 131072.0
            + (7.0 * d4 * e12) / 2048.0)
            * sin4_d
        + ((151.0 * e6) / 6144.0 + (151.0 * e8) / 4096.0 + (5019.0 * e10) / 131072.0 - (453.0 * d2 * e10) / 16384.0
            + (26965.0 * e12) / 786432.0
            - (8607.0 * d2 * e12) / 131072.0)
            * sin6_d
        + ((1097.0 * e8) / 131072.0 + (1097.0 * e10) / 65536.0 + (225797.0 * e12) / 10485760.0
            - (1097.0 * d2 * e12) / 65536.0)
            * sin8_d
        + ((8011.0 * e10) / 2621440.0 + (8011.0 * e12) / 1048576.0) * sin10_d
        + ((293393.0 * e12) / 251658240.0) * sin12_d
}

/// Isometric latitude.
fn calculate_sigma(ellipticity: f64, latitude: f64) -> f64 {
    if ellipticity == 0.0 {
        return (0.5 * (PI_OVER_TWO + latitude)).js_tan().js_ln();
    }
    let e_sin_l = ellipticity * latitude.js_sin();
    (0.5 * (PI_OVER_TWO + latitude)).js_tan().js_ln() - (ellipticity / 2.0) * ((1.0 + e_sin_l) / (1.0 - e_sin_l)).js_ln()
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EllipsoidRhumbLine {
    pub ellipsoid: Ellipsoid,
    pub start: Cartographic,
    pub end: Cartographic,
    /// Radians clockwise from north.
    pub heading: f64,
    /// Length in meters.
    pub surface_distance: f64,
    ellipticity: f64,
    ellipticity_squared: f64,
}

fn interpolate(start: &Cartographic, heading: f64, distance: f64, major: f64, ellipticity: f64) -> Cartographic {
    if distance == 0.0 {
        return *start;
    }
    let ellipticity_squared = ellipticity * ellipticity;
    let (longitude, latitude);
    if (PI_OVER_TWO - heading.abs()).abs() > EPSILON8 {
        let m1 = calculate_m(ellipticity, major, start.latitude);
        let delta_m = distance * heading.js_cos();
        let m2 = m1 + delta_m;
        latitude = calculate_inverse_m(m2, ellipticity, major);
        if heading.abs() < EPSILON10 {
            longitude = negative_pi_to_pi(start.longitude);
        } else {
            let sigma1 = calculate_sigma(ellipticity, start.latitude);
            let sigma2 = calculate_sigma(ellipticity, latitude);
            let delta_longitude = heading.js_tan() * (sigma2 - sigma1);
            longitude = negative_pi_to_pi(start.longitude + delta_longitude);
        }
    } else {
        latitude = start.latitude;
        let local_rad = if ellipticity == 0.0 {
            major * start.latitude.js_cos()
        } else {
            let sin_phi = start.latitude.js_sin();
            (major * start.latitude.js_cos()) / (1.0 - ellipticity_squared * sin_phi * sin_phi).sqrt()
        };
        let delta_longitude = distance / local_rad;
        longitude = if heading > 0.0 {
            negative_pi_to_pi(start.longitude + delta_longitude)
        } else {
            negative_pi_to_pi(start.longitude - delta_longitude)
        };
    }
    Cartographic::new(longitude, latitude, 0.0)
}

impl EllipsoidRhumbLine {
    pub fn new(start: &Cartographic, end: &Cartographic, ellipsoid: &Ellipsoid) -> Self {
        let major = ellipsoid.maximum_radius;
        let minor = ellipsoid.minimum_radius;
        let major_squared = major * major;
        let minor_squared = minor * minor;
        let ellipticity_squared = (major_squared - minor_squared) / major_squared;
        let ellipticity = ellipticity_squared.sqrt();
        let start = Cartographic::new(start.longitude, start.latitude, 0.0);
        let end = Cartographic::new(end.longitude, end.latitude, 0.0);
        let sigma1 = calculate_sigma(ellipticity, start.latitude);
        let sigma2 = calculate_sigma(ellipticity, end.latitude);
        let heading = negative_pi_to_pi(end.longitude - start.longitude).js_atan2(sigma2 - sigma1);

        let delta_longitude = end.longitude - start.longitude;
        let distance = if equals_epsilon(heading.abs(), PI_OVER_TWO, EPSILON8, EPSILON8) {
            if major == minor {
                major * start.latitude.js_cos() * negative_pi_to_pi(delta_longitude)
            } else {
                let sin_phi = start.latitude.js_sin();
                (major * start.latitude.js_cos() * negative_pi_to_pi(delta_longitude))
                    / (1.0 - ellipticity_squared * sin_phi * sin_phi).sqrt()
            }
        } else {
            let m1 = calculate_m(ellipticity, major, start.latitude);
            let m2 = calculate_m(ellipticity, major, end.latitude);
            (m2 - m1) / heading.js_cos()
        };
        Self {
            ellipsoid: *ellipsoid,
            start,
            end,
            heading,
            surface_distance: distance.abs(),
            ellipticity,
            ellipticity_squared,
        }
    }

    pub fn from_start_heading_distance(
        start: &Cartographic,
        heading: f64,
        distance: f64,
        ellipsoid: &Ellipsoid,
    ) -> Self {
        let major = ellipsoid.maximum_radius;
        let minor = ellipsoid.minimum_radius;
        let ellipticity = ((major * major - minor * minor) / (major * major)).sqrt();
        let heading = negative_pi_to_pi(heading);
        let end = interpolate(start, heading, distance, major, ellipticity);
        Self::new(start, &end, ellipsoid)
    }

    pub fn set_end_points(&mut self, start: &Cartographic, end: &Cartographic) {
        *self = Self::new(start, end, &self.ellipsoid);
    }

    pub fn interpolate_using_fraction(&self, fraction: f64) -> Cartographic {
        self.interpolate_using_surface_distance(fraction * self.surface_distance)
    }

    pub fn interpolate_using_surface_distance(&self, distance: f64) -> Cartographic {
        interpolate(&self.start, self.heading, distance, self.ellipsoid.maximum_radius, self.ellipticity)
    }

    /// Where the line crosses a meridian; `None` if it runs along that meridian.
    pub fn find_intersection_with_longitude(&self, intersection_longitude: f64) -> Option<Cartographic> {
        let ellipticity = self.ellipticity;
        let heading = self.heading;
        let abs_heading = heading.abs();
        let start = self.start;
        let mut lon = negative_pi_to_pi(intersection_longitude);
        if equals_epsilon(lon.abs(), PI, EPSILON14, EPSILON14) {
            lon = sign(start.longitude) * PI;
        }
        if (PI_OVER_TWO - abs_heading).abs() <= EPSILON8 {
            return Some(Cartographic::new(lon, start.latitude, 0.0));
        } else if equals_epsilon((PI_OVER_TWO - abs_heading).abs(), PI_OVER_TWO, EPSILON8, EPSILON8) {
            if equals_epsilon(lon, start.longitude, EPSILON12, EPSILON12) {
                return None;
            }
            return Some(Cartographic::new(lon, PI_OVER_TWO * sign(PI_OVER_TWO - heading), 0.0));
        }
        let phi1 = start.latitude;
        let e_sin_phi1 = ellipticity * phi1.js_sin();
        let left_component = (0.5 * (PI_OVER_TWO + phi1)).js_tan() * ((lon - start.longitude) / heading.js_tan()).js_exp();
        let denominator = (1.0 + e_sin_phi1) / (1.0 - e_sin_phi1);
        let mut new_phi = start.latitude;
        loop {
            let phi = new_phi;
            let e_sin_phi = ellipticity * phi.js_sin();
            let numerator = (1.0 + e_sin_phi) / (1.0 - e_sin_phi);
            new_phi = 2.0 * (left_component * (numerator / denominator).powf(ellipticity / 2.0)).js_atan() - PI_OVER_TWO;
            if equals_epsilon(new_phi, phi, EPSILON12, EPSILON12) {
                break;
            }
        }
        Some(Cartographic::new(lon, new_phi, 0.0))
    }

    /// Where the line crosses a parallel; `None` for an east-west line.
    pub fn find_intersection_with_latitude(&self, intersection_latitude: f64) -> Option<Cartographic> {
        if equals_epsilon(self.heading.abs(), PI_OVER_TWO, EPSILON8, EPSILON8) {
            return None;
        }
        let sigma1 = calculate_sigma(self.ellipticity, self.start.latitude);
        let sigma2 = calculate_sigma(self.ellipticity, intersection_latitude);
        let delta_longitude = self.heading.js_tan() * (sigma2 - sigma1);
        let longitude = negative_pi_to_pi(self.start.longitude + delta_longitude);
        Some(Cartographic::new(longitude, intersection_latitude, 0.0))
    }

    /// Squared eccentricity of the ellipsoid.
    pub fn ellipticity_squared(&self) -> f64 {
        self.ellipticity_squared
    }
}
