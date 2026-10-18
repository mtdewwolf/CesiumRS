//! CesiumJS `Stereographic`: a point projected stereographically onto the
//! plane tangent to a half-unit sphere at the north or south pole.

use super::cartesian2::Cartesian2;
use super::cartesian3::Cartesian3;
use super::cartographic::Cartographic;
use super::ellipsoid::Ellipsoid;
use super::ellipsoid_tangent_plane::EllipsoidTangentPlane;
use super::intersection_tests::ray_plane;
use super::js_math::JsMath;
use super::math::{sign_not_zero, PI_OVER_TWO, TWO_PI};
use super::ray::Ray;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Stereographic {
    pub position: Cartesian2,
    pub tangent_plane: EllipsoidTangentPlane,
}

impl Stereographic {
    pub const HALF_UNIT_SPHERE: Ellipsoid = Ellipsoid::new(0.5, 0.5, 0.5);
    pub const NORTH_POLE: Cartesian3 = Cartesian3::new(0.0, 0.0, 0.5);
    pub const SOUTH_POLE: Cartesian3 = Cartesian3::new(0.0, 0.0, -0.5);

    pub fn north_pole_tangent_plane() -> EllipsoidTangentPlane {
        EllipsoidTangentPlane::new(Self::NORTH_POLE, &Self::HALF_UNIT_SPHERE).expect("the pole is on the sphere")
    }

    pub fn south_pole_tangent_plane() -> EllipsoidTangentPlane {
        EllipsoidTangentPlane::new(Self::SOUTH_POLE, &Self::HALF_UNIT_SPHERE).expect("the pole is on the sphere")
    }

    /// Defaults to the origin of the north-pole plane.
    pub fn new(position: Option<Cartesian2>, tangent_plane: Option<EllipsoidTangentPlane>) -> Self {
        Self {
            position: position.unwrap_or_default(),
            tangent_plane: tangent_plane.unwrap_or_else(Self::north_pole_tangent_plane),
        }
    }

    pub fn ellipsoid(&self) -> &Ellipsoid {
        &self.tangent_plane.ellipsoid
    }

    pub fn x(&self) -> f64 {
        self.position.x
    }

    pub fn y(&self) -> f64 {
        self.position.y
    }

    pub fn conformal_latitude(&self) -> f64 {
        let r = self.position.magnitude();
        let d = 2.0 * self.ellipsoid().maximum_radius;
        let sign = self.tangent_plane.plane.normal.z;
        sign * (PI_OVER_TWO - 2.0 * r.js_atan2(d))
    }

    pub fn longitude(&self) -> f64 {
        let mut longitude = PI_OVER_TWO + self.y().js_atan2(self.x());
        if longitude > std::f64::consts::PI {
            longitude -= TWO_PI;
        }
        longitude
    }

    /// Geodetic latitude on `ellipsoid` (Cesium defaults to WGS84).
    pub fn get_latitude(&self, ellipsoid: &Ellipsoid) -> Option<f64> {
        let cartographic = Cartographic::new(self.longitude(), self.conformal_latitude(), 0.0);
        let cartesian = self.ellipsoid().cartographic_to_cartesian(&cartographic);
        ellipsoid.cartesian_to_cartographic(cartesian).map(|c| c.latitude)
    }

    /// Projects from the opposite pole onto the tangent plane of the pole in
    /// `cartesian`'s hemisphere. `None` if the ray misses the plane.
    pub fn from_cartesian(cartesian: Cartesian3) -> Option<Self> {
        let sign = sign_not_zero(cartesian.z);
        let (tangent_plane, origin) = if sign < 0.0 {
            (Self::south_pole_tangent_plane(), Self::NORTH_POLE)
        } else {
            (Self::north_pole_tangent_plane(), Self::SOUTH_POLE)
        };
        let ray_origin = tangent_plane.ellipsoid.scale_to_geocentric_surface(cartesian);
        let ray = Ray { origin: ray_origin, direction: ray_origin.subtract(origin).normalize() };
        let intersection_point = ray_plane(&ray, &tangent_plane.plane)?;
        let v = intersection_point.subtract(origin);
        let x = tangent_plane.x_axis.dot(v);
        let y = sign * tangent_plane.y_axis.dot(v);
        Some(Self { position: Cartesian2::new(x, y), tangent_plane })
    }

    pub fn from_cartesian_array(cartesians: &[Cartesian3]) -> Option<Vec<Self>> {
        cartesians.iter().map(|c| Self::from_cartesian(*c)).collect()
    }
}
