//! CesiumJS `Ellipsoid`: the quadric `(x/a)² + (y/b)² + (z/c)² = 1`.
//!
//! Cesium's mutable `Ellipsoid.default` is not ported: Rust APIs take the
//! ellipsoid explicitly, usually [`Ellipsoid::WGS84`].

use super::cartesian2::Cartesian2;
use super::cartesian3::Cartesian3;
use super::cartographic::Cartographic;
use super::math::{equals_epsilon, sign, EPSILON1, EPSILON14, EPSILON15, TWO_PI};
use super::rectangle::Rectangle;
use super::scale_to_geodetic_surface::scale_to_geodetic_surface;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ellipsoid {
    pub radii: Cartesian3,
    pub radii_squared: Cartesian3,
    pub radii_to_the_fourth: Cartesian3,
    pub one_over_radii: Cartesian3,
    pub one_over_radii_squared: Cartesian3,
    pub minimum_radius: f64,
    pub maximum_radius: f64,
    pub center_tolerance_squared: f64,
    /// `radii.x² / radii.z²`; NaN when `radii.z` is zero.
    pub squared_x_over_squared_z: f64,
}

const fn recip(v: f64) -> f64 {
    if v == 0.0 {
        0.0
    } else {
        1.0 / v
    }
}

const fn min3(x: f64, y: f64, z: f64) -> f64 {
    let m = if x < y { x } else { y };
    if m < z {
        m
    } else {
        z
    }
}

const fn max3(x: f64, y: f64, z: f64) -> f64 {
    let m = if x > y { x } else { y };
    if m > z {
        m
    } else {
        z
    }
}

impl Ellipsoid {
    pub const WGS84: Self = Self::new(6378137.0, 6378137.0, 6356752.3142451793);
    pub const UNIT_SPHERE: Self = Self::new(1.0, 1.0, 1.0);
    pub const MOON: Self = Self::new(
        super::math::LUNAR_RADIUS,
        super::math::LUNAR_RADIUS,
        super::math::LUNAR_RADIUS,
    );
    pub const MARS: Self = Self::new(3396190.0, 3396190.0, 3376200.0);
    pub const PACKED_LENGTH: usize = 3;

    pub const fn new(x: f64, y: f64, z: f64) -> Self {
        Self {
            radii: Cartesian3::new(x, y, z),
            radii_squared: Cartesian3::new(x * x, y * y, z * z),
            radii_to_the_fourth: Cartesian3::new(x * x * x * x, y * y * y * y, z * z * z * z),
            one_over_radii: Cartesian3::new(recip(x), recip(y), recip(z)),
            one_over_radii_squared: Cartesian3::new(
                if x == 0.0 { 0.0 } else { 1.0 / (x * x) },
                if y == 0.0 { 0.0 } else { 1.0 / (y * y) },
                if z == 0.0 { 0.0 } else { 1.0 / (z * z) },
            ),
            minimum_radius: min3(x, y, z),
            maximum_radius: max3(x, y, z),
            center_tolerance_squared: EPSILON1,
            squared_x_over_squared_z: if z * z != 0.0 { (x * x) / (z * z) } else { f64::NAN },
        }
    }

    pub const fn from_cartesian3(radii: Cartesian3) -> Self {
        Self::new(radii.x, radii.y, radii.z)
    }

    pub fn pack(&self, array: &mut [f64], starting_index: usize) {
        self.radii.pack(array, starting_index);
    }

    pub fn unpack(array: &[f64], starting_index: usize) -> Self {
        Self::from_cartesian3(Cartesian3::unpack(array, starting_index))
    }

    /// Direction from the center, Cesium `geocentricSurfaceNormal`.
    pub fn geocentric_surface_normal(&self, cartesian: Cartesian3) -> Cartesian3 {
        cartesian.normalize()
    }

    pub fn geodetic_surface_normal_cartographic(&self, cartographic: &Cartographic) -> Cartesian3 {
        let cos_latitude = cartographic.latitude.cos();
        Cartesian3::new(
            cos_latitude * cartographic.longitude.cos(),
            cos_latitude * cartographic.longitude.sin(),
            cartographic.latitude.sin(),
        )
        .normalize()
    }

    /// `None` at the center, where the normal is undefined.
    pub fn geodetic_surface_normal(&self, cartesian: Cartesian3) -> Option<Cartesian3> {
        if cartesian.equals_epsilon(Cartesian3::ZERO, EPSILON14) {
            return None;
        }
        Some(cartesian.multiply_components(self.one_over_radii_squared).normalize())
    }

    pub fn cartographic_to_cartesian(&self, cartographic: &Cartographic) -> Cartesian3 {
        let n = self.geodetic_surface_normal_cartographic(cartographic);
        let k = self.radii_squared.multiply_components(n);
        let gamma = n.dot(k).sqrt();
        k.divide_by_scalar(gamma).add(n.multiply_by_scalar(cartographic.height))
    }

    pub fn cartographic_array_to_cartesian_array(&self, cartographics: &[Cartographic]) -> Vec<Cartesian3> {
        cartographics.iter().map(|c| self.cartographic_to_cartesian(c)).collect()
    }

    /// `None` at the center.
    pub fn cartesian_to_cartographic(&self, cartesian: Cartesian3) -> Option<Cartographic> {
        let p = self.scale_to_geodetic_surface(cartesian)?;
        let n = self.geodetic_surface_normal(p)?;
        let h = cartesian.subtract(p);
        Some(Cartographic::new(
            n.y.atan2(n.x),
            n.z.asin(),
            sign(h.dot(cartesian)) * h.magnitude(),
        ))
    }

    pub fn cartesian_array_to_cartographic_array(&self, cartesians: &[Cartesian3]) -> Vec<Option<Cartographic>> {
        cartesians.iter().map(|c| self.cartesian_to_cartographic(*c)).collect()
    }

    /// Closest surface point along the geodetic normal; `None` at the center.
    pub fn scale_to_geodetic_surface(&self, cartesian: Cartesian3) -> Option<Cartesian3> {
        scale_to_geodetic_surface(
            cartesian,
            self.one_over_radii,
            self.one_over_radii_squared,
            self.center_tolerance_squared,
        )
    }

    /// Surface point along the ray from the center.
    pub fn scale_to_geocentric_surface(&self, cartesian: Cartesian3) -> Cartesian3 {
        let o = self.one_over_radii_squared;
        let beta = 1.0
            / (cartesian.x * cartesian.x * o.x + cartesian.y * cartesian.y * o.y + cartesian.z * cartesian.z * o.z)
                .sqrt();
        cartesian.multiply_by_scalar(beta)
    }

    pub fn transform_position_to_scaled_space(&self, position: Cartesian3) -> Cartesian3 {
        position.multiply_components(self.one_over_radii)
    }

    pub fn transform_position_from_scaled_space(&self, position: Cartesian3) -> Cartesian3 {
        position.multiply_components(self.radii)
    }

    pub fn equals(&self, right: &Self) -> bool {
        self.radii == right.radii
    }

    /// Where the surface normal at `position` meets the Z axis, or `None`
    /// within `buffer` of the poles. Panics unless `radii.x == radii.y`.
    pub fn get_surface_normal_intersection_with_z_axis(&self, position: Cartesian3, buffer: f64) -> Option<Cartesian3> {
        assert!(
            equals_epsilon(self.radii.x, self.radii.y, EPSILON15, EPSILON15),
            "Ellipsoid must be an ellipsoid of revolution (radii.x == radii.y)"
        );
        let z = position.z * (1.0 - self.squared_x_over_squared_z);
        if z.abs() >= self.radii.z - buffer {
            return None;
        }
        Some(Cartesian3::new(0.0, 0.0, z))
    }

    /// Principal curvatures `(1 / prime-vertical radius, 1 / meridional radius)`.
    pub fn get_local_curvature(&self, surface_position: Cartesian3) -> Cartesian2 {
        let endpoint = self
            .get_surface_normal_intersection_with_z_axis(surface_position, 0.0)
            .unwrap_or(Cartesian3::new(f64::NAN, f64::NAN, f64::NAN));
        let prime_vertical_radius = surface_position.distance(endpoint);
        let radius_ratio = (self.minimum_radius * prime_vertical_radius) / self.maximum_radius.powi(2);
        let meridional_radius = prime_vertical_radius * radius_ratio.powi(2);
        Cartesian2::new(1.0 / prime_vertical_radius, 1.0 / meridional_radius)
    }

    /// Area of a cartographic rectangle on the surface, by Gauss-Legendre quadrature.
    pub fn surface_area(&self, rectangle: &Rectangle) -> f64 {
        let min_longitude = rectangle.west;
        let mut max_longitude = rectangle.east;
        let min_latitude = rectangle.south;
        let max_latitude = rectangle.north;
        while max_longitude < min_longitude {
            max_longitude += TWO_PI;
        }
        let a2 = self.radii_squared.x;
        let b2 = self.radii_squared.y;
        let c2 = self.radii_squared.z;
        let a2b2 = a2 * b2;
        gauss_legendre_quadrature(min_latitude, max_latitude, |lat| {
            let sin_phi = lat.cos();
            let cos_phi = lat.sin();
            lat.cos()
                * gauss_legendre_quadrature(min_longitude, max_longitude, |lon| {
                    let cos_theta = lon.cos();
                    let sin_theta = lon.sin();
                    (a2b2 * cos_phi * cos_phi
                        + c2 * (b2 * cos_theta * cos_theta + a2 * sin_theta * sin_theta) * sin_phi * sin_phi)
                        .sqrt()
                })
        })
    }
}

const ABSCISSAS: [f64; 5] = [
    0.14887433898163,
    0.43339539412925,
    0.67940956829902,
    0.86506336668898,
    0.97390652851717,
];
const WEIGHTS: [f64; 5] = [
    0.29552422471475,
    0.26926671930999,
    0.21908636251598,
    0.14945134915058,
    0.066671344308684,
];

fn gauss_legendre_quadrature(a: f64, b: f64, f: impl Fn(f64) -> f64) -> f64 {
    let x_mean = 0.5 * (b + a);
    let x_range = 0.5 * (b - a);
    let mut sum = 0.0;
    for i in 0..5 {
        let dx = x_range * ABSCISSAS[i];
        sum += WEIGHTS[i] * (f(x_mean + dx) + f(x_mean - dx));
    }
    sum * x_range
}

impl std::fmt::Display for Ellipsoid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.radii.fmt(f)
    }
}
