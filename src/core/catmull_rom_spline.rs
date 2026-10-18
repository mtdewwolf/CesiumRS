//! CesiumJS `CatmullRomSpline`: a C¹ cubic through Cartesian3 points, with
//! tangents from neighboring points.

use super::cartesian3::Cartesian3;
use super::hermite_spline::{coefficients, HERMITE_COEFFICIENT_MATRIX};
use super::matrix4::Matrix4;
use super::spline::{valid_times, Spline};

/// Cesium's `CatmullRomSpline.catmullRomCoefficientMatrix`.
pub const CATMULL_ROM_COEFFICIENT_MATRIX: Matrix4 =
    Matrix4::new(-0.5, 1.0, -0.5, 0.0, 1.5, -2.5, 0.0, 1.0, -1.5, 2.0, 0.5, 0.0, 0.5, -0.5, 0.0, 0.0);

#[derive(Clone, Debug, PartialEq)]
pub struct CatmullRomSpline {
    times: Vec<f64>,
    points: Vec<Cartesian3>,
    first_tangent: Option<Cartesian3>,
    last_tangent: Option<Cartesian3>,
}

impl CatmullRomSpline {
    /// Missing end tangents are derived from the first and last three points.
    /// With two points the spline is a line and has no tangents.
    pub fn new(
        times: Vec<f64>,
        points: Vec<Cartesian3>,
        first_tangent: Option<Cartesian3>,
        last_tangent: Option<Cartesian3>,
    ) -> Option<Self> {
        if points.len() < 2 || times.len() != points.len() || !valid_times(&times) {
            return None;
        }
        let (mut first_tangent, mut last_tangent) = (first_tangent, last_tangent);
        if points.len() > 2 {
            first_tangent = first_tangent.or_else(|| {
                Some(points[1].multiply_by_scalar(2.0).subtract(points[2]).subtract(points[0]).multiply_by_scalar(0.5))
            });
            last_tangent = last_tangent.or_else(|| {
                let n = points.len() - 1;
                Some(
                    points[n]
                        .subtract(points[n - 1].multiply_by_scalar(2.0))
                        .add(points[n - 2])
                        .multiply_by_scalar(0.5),
                )
            });
        }
        Some(Self { times, points, first_tangent, last_tangent })
    }

    pub fn points(&self) -> &[Cartesian3] {
        &self.points
    }

    pub fn first_tangent(&self) -> Option<Cartesian3> {
        self.first_tangent
    }

    pub fn last_tangent(&self) -> Option<Cartesian3> {
        self.last_tangent
    }
}

impl Spline for CatmullRomSpline {
    type Value = Cartesian3;

    fn times(&self) -> &[f64] {
        &self.times
    }

    /// With two points, Cesium interpolates linearly without a range check,
    /// so times outside the spline extrapolate.
    fn evaluate(&self, time: f64) -> Option<Cartesian3> {
        let (points, times) = (&self.points, &self.times);
        if points.len() < 3 {
            let t0 = times[0];
            let inv_span = 1.0 / (times[1] - t0);
            return Some(points[0].lerp(points[1], (time - t0) * inv_span));
        }
        let i = self.find_time_interval(time, None)?;
        let u = (time - times[i]) / (times[i + 1] - times[i]);
        let (p0, p1, p2, p3, coefs);
        if i == 0 {
            p0 = points[0];
            p1 = points[1];
            p2 = self.first_tangent?;
            p3 = points[2].subtract(p0).multiply_by_scalar(0.5);
            coefs = coefficients(&HERMITE_COEFFICIENT_MATRIX, u);
        } else if i == points.len() - 2 {
            p0 = points[i];
            p1 = points[i + 1];
            p3 = self.last_tangent?;
            p2 = p1.subtract(points[i - 1]).multiply_by_scalar(0.5);
            coefs = coefficients(&HERMITE_COEFFICIENT_MATRIX, u);
        } else {
            p0 = points[i - 1];
            p1 = points[i];
            p2 = points[i + 1];
            p3 = points[i + 2];
            coefs = coefficients(&CATMULL_ROM_COEFFICIENT_MATRIX, u);
        }
        Some(
            p0.multiply_by_scalar(coefs.x)
                .add(p1.multiply_by_scalar(coefs.y))
                .add(p2.multiply_by_scalar(coefs.z))
                .add(p3.multiply_by_scalar(coefs.w)),
        )
    }
}
