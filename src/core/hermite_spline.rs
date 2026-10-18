//! CesiumJS `HermiteSpline`: cubic Hermite interpolation with given in and
//! out tangents, plus natural and clamped cubic construction.

use super::cartesian3::Cartesian3;
use super::cartesian4::Cartesian4;
use super::linear_spline::LinearSpline;
use super::matrix4::Matrix4;
use super::quaternion::Quaternion;
use super::spline::{valid_times, Spline, SplinePoint};
use super::tridiagonal_system_solver;

/// Control points a Hermite spline can blend: numbers, Cartesian3 and
/// (component-wise) quaternions, as in Cesium.
pub trait HermitePoint: SplinePoint {
    fn scale(self, s: f64) -> Self;
    fn plus(self, other: Self) -> Self;
}

impl HermitePoint for f64 {
    fn scale(self, s: f64) -> Self {
        self * s
    }
    fn plus(self, other: Self) -> Self {
        self + other
    }
}

impl HermitePoint for Cartesian3 {
    fn scale(self, s: f64) -> Self {
        self.multiply_by_scalar(s)
    }
    fn plus(self, other: Self) -> Self {
        self.add(other)
    }
}

impl HermitePoint for Quaternion {
    fn scale(self, s: f64) -> Self {
        self.multiply_by_scalar(s)
    }
    fn plus(self, other: Self) -> Self {
        self.add(&other)
    }
}

/// Cesium's `HermiteSpline.hermiteCoefficientMatrix`.
pub const HERMITE_COEFFICIENT_MATRIX: Matrix4 =
    Matrix4::new(2.0, -3.0, 0.0, 1.0, -2.0, 3.0, 0.0, 0.0, 1.0, -2.0, 1.0, 0.0, 1.0, -1.0, 0.0, 0.0);

/// `[u³, u², u, 1]` times a coefficient matrix.
pub(crate) fn coefficients(matrix: &Matrix4, u: f64) -> Cartesian4 {
    let u2 = u * u;
    matrix.multiply_by_vector(Cartesian4::new(u2 * u, u2, u, 1.0))
}

#[derive(Clone, Debug, PartialEq)]
pub struct HermiteSpline<T: HermitePoint> {
    times: Vec<f64>,
    points: Vec<T>,
    in_tangents: Vec<T>,
    out_tangents: Vec<T>,
}

/// What [`HermiteSpline::create_natural_cubic`] and
/// [`HermiteSpline::create_clamped_cubic`] build: Cesium falls back to a
/// linear spline for two points.
#[derive(Clone, Debug, PartialEq)]
pub enum CubicSpline {
    Hermite(HermiteSpline<Cartesian3>),
    Linear(LinearSpline<Cartesian3>),
}

impl<T: HermitePoint> HermiteSpline<T> {
    /// `in_tangents` and `out_tangents` have one entry per segment
    /// (`points.len() - 1`). `None` if the lengths or times are invalid.
    pub fn new(times: Vec<f64>, points: Vec<T>, in_tangents: Vec<T>, out_tangents: Vec<T>) -> Option<Self> {
        if points.len() < 2
            || times.len() != points.len()
            || in_tangents.len() != out_tangents.len()
            || in_tangents.len() != points.len() - 1
            || !valid_times(&times)
        {
            return None;
        }
        Some(Self { times, points, in_tangents, out_tangents })
    }

    /// A C¹ spline from one tangent per point.
    pub fn create_c1(times: Vec<f64>, points: Vec<T>, tangents: Vec<T>) -> Option<Self> {
        if points.len() < 2 || times.len() != points.len() || times.len() != tangents.len() {
            return None;
        }
        let out_tangents = tangents[..tangents.len() - 1].to_vec();
        let in_tangents = tangents[1..].to_vec();
        Self::new(times, points, in_tangents, out_tangents)
    }

    pub fn points(&self) -> &[T] {
        &self.points
    }

    pub fn in_tangents(&self) -> &[T] {
        &self.in_tangents
    }

    pub fn out_tangents(&self) -> &[T] {
        &self.out_tangents
    }
}

fn split_tangents(tangents: Vec<Cartesian3>) -> (Vec<Cartesian3>, Vec<Cartesian3>) {
    let out_tangents = tangents[..tangents.len() - 1].to_vec();
    let in_tangents = tangents[1..].to_vec();
    (in_tangents, out_tangents)
}

impl HermiteSpline<Cartesian3> {
    /// A C² spline with zero second derivative at both ends.
    pub fn create_natural_cubic(times: Vec<f64>, points: Vec<Cartesian3>) -> Option<CubicSpline> {
        if points.len() < 2 || times.len() != points.len() {
            return None;
        }
        if points.len() < 3 {
            return LinearSpline::new(times, points).map(CubicSpline::Linear);
        }
        let (in_tangents, out_tangents) = split_tangents(generate_natural(&points)?);
        Self::new(times, points, in_tangents, out_tangents).map(CubicSpline::Hermite)
    }

    /// A C² spline with the given first and last tangents.
    pub fn create_clamped_cubic(
        times: Vec<f64>,
        points: Vec<Cartesian3>,
        first_tangent: Cartesian3,
        last_tangent: Cartesian3,
    ) -> Option<CubicSpline> {
        if points.len() < 2 || times.len() != points.len() {
            return None;
        }
        if points.len() < 3 {
            return LinearSpline::new(times, points).map(CubicSpline::Linear);
        }
        let (in_tangents, out_tangents) = split_tangents(generate_clamped(&points, first_tangent, last_tangent)?);
        Self::new(times, points, in_tangents, out_tangents).map(CubicSpline::Hermite)
    }
}

/// Tangents with the first and last fixed. The coefficients, including the
/// second-to-last row, are Cesium's.
fn generate_clamped(
    points: &[Cartesian3],
    first_tangent: Cartesian3,
    last_tangent: Cartesian3,
) -> Option<Vec<Cartesian3>> {
    let n = points.len();
    let mut l = vec![0.0; n - 1];
    let mut u = vec![0.0; n - 1];
    let mut d = vec![0.0; n];
    let mut r = vec![Cartesian3::ZERO; n];
    l[0] = 1.0;
    d[0] = 1.0;
    u[0] = 0.0;
    r[0] = first_tangent;
    let mut i = 1;
    while i + 1 < l.len() {
        l[i] = 1.0;
        u[i] = 1.0;
        d[i] = 4.0;
        r[i] = points[i + 1].subtract(points[i - 1]).multiply_by_scalar(3.0);
        i += 1;
    }
    l[i] = 0.0;
    u[i] = 1.0;
    d[i] = 4.0;
    r[i] = points[i + 1].subtract(points[i - 1]).multiply_by_scalar(3.0);
    d[i + 1] = 1.0;
    r[i + 1] = last_tangent;
    tridiagonal_system_solver::solve(&l, &d, &u, &r)
}

fn generate_natural(points: &[Cartesian3]) -> Option<Vec<Cartesian3>> {
    let n = points.len();
    let mut l = vec![0.0; n - 1];
    let mut u = vec![0.0; n - 1];
    let mut d = vec![0.0; n];
    let mut r = vec![Cartesian3::ZERO; n];
    l[0] = 1.0;
    u[0] = 1.0;
    d[0] = 2.0;
    r[0] = points[1].subtract(points[0]).multiply_by_scalar(3.0);
    let mut i = 1;
    while i < l.len() {
        l[i] = 1.0;
        u[i] = 1.0;
        d[i] = 4.0;
        r[i] = points[i + 1].subtract(points[i - 1]).multiply_by_scalar(3.0);
        i += 1;
    }
    d[i] = 2.0;
    r[i] = points[i].subtract(points[i - 1]).multiply_by_scalar(3.0);
    tridiagonal_system_solver::solve(&l, &d, &u, &r)
}

impl<T: HermitePoint> Spline for HermiteSpline<T> {
    type Value = T;

    fn times(&self) -> &[f64] {
        &self.times
    }

    fn evaluate(&self, time: f64) -> Option<T> {
        let i = self.find_time_interval(time, None)?;
        let times_delta = self.times[i + 1] - self.times[i];
        let u = (time - self.times[i]) / times_delta;
        let mut coefs = coefficients(&HERMITE_COEFFICIENT_MATRIX, u);
        coefs.z *= times_delta;
        coefs.w *= times_delta;
        Some(
            self.points[i]
                .scale(coefs.x)
                .plus(self.points[i + 1].scale(coefs.y))
                .plus(self.out_tangents[i].scale(coefs.z))
                .plus(self.in_tangents[i].scale(coefs.w)),
        )
    }
}

impl Spline for CubicSpline {
    type Value = Cartesian3;

    fn times(&self) -> &[f64] {
        match self {
            Self::Hermite(s) => s.times(),
            Self::Linear(s) => s.times(),
        }
    }

    fn evaluate(&self, time: f64) -> Option<Cartesian3> {
        match self {
            Self::Hermite(s) => s.evaluate(time),
            Self::Linear(s) => s.evaluate(time),
        }
    }
}
