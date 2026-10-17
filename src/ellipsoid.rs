//! WGS84 ellipsoid. Ported from CesiumJS `Ellipsoid` and `scaleToGeodeticSurface`.

use crate::math::{equals_epsilon, sign, Vec3, EPSILON1, EPSILON12};

/// Quadratic surface `(x/a)² + (y/b)² + (z/c)² = 1`.
#[derive(Clone, Copy, Debug)]
pub struct Ellipsoid {
    pub radii: Vec3,
    pub radii_squared: Vec3,
    pub one_over_radii: Vec3,
    pub one_over_radii_squared: Vec3,
    pub minimum_radius: f64,
    pub maximum_radius: f64,
    pub center_tolerance_squared: f64,
}

impl Ellipsoid {
    pub const fn new(x: f64, y: f64, z: f64) -> Self {
        let radii_squared = Vec3::new(x * x, y * y, z * z);
        let one_over_radii = Vec3::new(
            if x == 0.0 { 0.0 } else { 1.0 / x },
            if y == 0.0 { 0.0 } else { 1.0 / y },
            if z == 0.0 { 0.0 } else { 1.0 / z },
        );
        let one_over_radii_squared = Vec3::new(
            if x == 0.0 { 0.0 } else { 1.0 / (x * x) },
            if y == 0.0 { 0.0 } else { 1.0 / (y * y) },
            if z == 0.0 { 0.0 } else { 1.0 / (z * z) },
        );
        Self {
            radii: Vec3::new(x, y, z),
            radii_squared,
            one_over_radii,
            one_over_radii_squared,
            minimum_radius: if y < x && y < z {
                y
            } else if z < x {
                z
            } else {
                x
            },
            maximum_radius: if y > x && y > z {
                y
            } else if z > x {
                z
            } else {
                x
            },
            center_tolerance_squared: EPSILON1,
        }
    }

    /// Cesium `Ellipsoid.WGS84`.
    pub const fn wgs84() -> Self {
        Self::new(6_378_137.0, 6_378_137.0, 6_356_752.314_245_179_3)
    }
}

pub fn geodetic_surface_normal(ellipsoid: &Ellipsoid, cartesian: Vec3) -> Vec3 {
    cartesian.mul_components(ellipsoid.one_over_radii_squared).normalize()
}

pub fn geodetic_surface_normal_cartographic(longitude: f64, latitude: f64) -> Vec3 {
    let cos_lat = latitude.cos();
    Vec3::new(cos_lat * longitude.cos(), cos_lat * longitude.sin(), latitude.sin())
}

/// Cesium `Ellipsoid.cartographicToCartesian`. Longitude and latitude are radians.
pub fn cartographic_to_cartesian(ellipsoid: &Ellipsoid, longitude: f64, latitude: f64, height: f64) -> Vec3 {
    let n = geodetic_surface_normal_cartographic(longitude, latitude);
    let k = ellipsoid.radii_squared.mul_components(n);
    let gamma = n.dot(k).sqrt();
    let surface = if gamma == 0.0 { Vec3::ZERO } else { k / gamma };
    surface + n * height
}

/// Cesium `scaleToGeodeticSurface`. `None` when the point is at the center.
pub fn scale_to_geodetic_surface(ellipsoid: &Ellipsoid, cartesian: Vec3) -> Option<Vec3> {
    let one_over = ellipsoid.one_over_radii;
    let one_over_sq = ellipsoid.one_over_radii_squared;

    let x2 = cartesian.x * cartesian.x * one_over.x * one_over.x;
    let y2 = cartesian.y * cartesian.y * one_over.y * one_over.y;
    let z2 = cartesian.z * cartesian.z * one_over.z * one_over.z;

    let squared_norm = x2 + y2 + z2;
    let ratio = (1.0 / squared_norm).sqrt();
    let intersection = cartesian * ratio;

    if squared_norm < ellipsoid.center_tolerance_squared {
        return if ratio.is_finite() { Some(intersection) } else { None };
    }

    let gradient = Vec3::new(
        intersection.x * one_over_sq.x * 2.0,
        intersection.y * one_over_sq.y * 2.0,
        intersection.z * one_over_sq.z * 2.0,
    );

    let mut lambda = ((1.0 - ratio) * cartesian.magnitude()) / (0.5 * gradient.magnitude());
    let mut correction = 0.0;
    let mut x_m = 1.0;
    let mut y_m = 1.0;
    let mut z_m = 1.0;

    for _ in 0..32 {
        lambda -= correction;
        x_m = 1.0 / (1.0 + lambda * one_over_sq.x);
        y_m = 1.0 / (1.0 + lambda * one_over_sq.y);
        z_m = 1.0 / (1.0 + lambda * one_over_sq.z);
        let x_m2 = x_m * x_m;
        let y_m2 = y_m * y_m;
        let z_m2 = z_m * z_m;
        let func = x2 * x_m2 + y2 * y_m2 + z2 * z_m2 - 1.0;
        let denominator =
            x2 * x_m2 * x_m * one_over_sq.x + y2 * y_m2 * y_m * one_over_sq.y + z2 * z_m2 * z_m * one_over_sq.z;
        let derivative = -2.0 * denominator;
        if derivative == 0.0 || !derivative.is_finite() {
            break;
        }
        correction = func / derivative;
        if func.abs() <= EPSILON12 {
            break;
        }
    }

    Some(Vec3::new(cartesian.x * x_m, cartesian.y * y_m, cartesian.z * z_m))
}

#[derive(Clone, Copy, Debug)]
pub struct Cartographic {
    pub longitude: f64,
    pub latitude: f64,
    pub height: f64,
}

/// Cesium `Ellipsoid.cartesianToCartographic`.
pub fn cartesian_to_cartographic(ellipsoid: &Ellipsoid, cartesian: Vec3) -> Option<Cartographic> {
    let p = scale_to_geodetic_surface(ellipsoid, cartesian)?;
    let n = geodetic_surface_normal(ellipsoid, p);
    let h = cartesian - p;
    let longitude = n.y.atan2(n.x);
    let latitude = n.z.clamp(-1.0, 1.0).asin();
    let height = sign(h.dot(cartesian)) * h.magnitude();
    if !longitude.is_finite() || !latitude.is_finite() || !height.is_finite() {
        return None;
    }
    Some(Cartographic { longitude, latitude, height })
}

pub fn vec3_near(a: Vec3, b: Vec3, eps: f64) -> bool {
    equals_epsilon(a.x, b.x, eps) && equals_epsilon(a.y, b.y, eps) && equals_epsilon(a.z, b.z, eps)
}
