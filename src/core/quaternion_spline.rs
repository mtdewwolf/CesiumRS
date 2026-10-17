//! CesiumJS `QuaternionSpline`, using its Float32-rounded fastSlerp.

use super::spline::{valid_times, Spline};
use super::Quaternion;

#[derive(Clone, Debug, PartialEq)]
pub struct QuaternionSpline {
    times: Vec<f64>,
    points: Vec<Quaternion>,
}

impl QuaternionSpline {
    pub fn new(times: Vec<f64>, points: Vec<Quaternion>) -> Option<Self> {
        if times.len() != points.len() || !valid_times(&times) {
            return None;
        }
        Some(Self { times, points })
    }

    pub fn points(&self) -> &[Quaternion] {
        &self.points
    }
}

impl Spline for QuaternionSpline {
    type Value = Quaternion;

    fn times(&self) -> &[f64] {
        &self.times
    }

    fn evaluate(&self, time: f64) -> Option<Quaternion> {
        let i = self.find_time_interval(time, None)?;
        let u = (time - self.times[i]) / (self.times[i + 1] - self.times[i]);
        Some(self.points[i].fast_slerp(&self.points[i + 1], u))
    }
}
