//! CesiumJS `Plane` in Hessian normal form: `normal · p + distance = 0`.

use super::cartesian3::Cartesian3;
use super::cartesian4::Cartesian4;
use super::math::{equals_epsilon, EPSILON6};
use super::matrix4::Matrix4;
use super::runtime_error::RuntimeError;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Plane {
    /// Unit normal.
    pub normal: Cartesian3,
    /// Signed distance from the origin to the plane, along `-normal`.
    pub distance: f64,
}

fn debug_check_normal(normal: Cartesian3) {
    debug_assert!(
        equals_epsilon(normal.magnitude(), 1.0, EPSILON6, EPSILON6),
        "normal must be normalized."
    );
}

impl Plane {
    pub const ORIGIN_XY_PLANE: Self = Self { normal: Cartesian3::UNIT_Z, distance: 0.0 };
    pub const ORIGIN_YZ_PLANE: Self = Self { normal: Cartesian3::UNIT_X, distance: 0.0 };
    pub const ORIGIN_ZX_PLANE: Self = Self { normal: Cartesian3::UNIT_Y, distance: 0.0 };

    pub fn new(normal: Cartesian3, distance: f64) -> Self {
        debug_check_normal(normal);
        Self { normal, distance }
    }

    pub fn from_point_normal(point: Cartesian3, normal: Cartesian3) -> Self {
        Self::new(normal, -normal.dot(point))
    }

    /// `(x, y, z)` is the normal and `w` the distance.
    pub fn from_cartesian4(coefficients: Cartesian4) -> Self {
        Self::new(Cartesian3::from_cartesian4(coefficients), coefficients.w)
    }

    /// Signed distance, positive on the side the normal points to.
    pub fn get_point_distance(&self, point: Cartesian3) -> f64 {
        self.normal.dot(point) + self.distance
    }

    pub fn project_point_onto_plane(&self, point: Cartesian3) -> Cartesian3 {
        let d = self.get_point_distance(point);
        point.subtract(self.normal.multiply_by_scalar(d))
    }

    /// Transforms the plane by the inverse transpose of `transform`.
    pub fn transform(&self, transform: &Matrix4) -> Result<Self, RuntimeError> {
        let inverse_transpose = transform.inverse_transpose()?;
        let p = inverse_transpose.multiply_by_vector(Cartesian4::new(
            self.normal.x,
            self.normal.y,
            self.normal.z,
            self.distance,
        ));
        let n = Cartesian3::from_cartesian4(p);
        Ok(Self::from_cartesian4(p.divide_by_scalar(n.magnitude())))
    }

    pub fn equals(&self, right: &Self) -> bool {
        self.distance == right.distance && self.normal == right.normal
    }
}
