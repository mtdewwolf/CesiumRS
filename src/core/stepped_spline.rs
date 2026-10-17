//! CesiumJS `SteppedSpline`.

use super::spline::{valid_times, Spline, SplinePoint};

#[derive(Clone, Debug, PartialEq)]
pub struct SteppedSpline<T: SplinePoint> {
    times: Vec<f64>,
    points: Vec<T>,
}

impl<T: SplinePoint> SteppedSpline<T> {
    pub fn new(times: Vec<f64>, points: Vec<T>) -> Option<Self> {
        if times.len() != points.len() || !valid_times(&times) {
            return None;
        }
        Some(Self { times, points })
    }

    pub fn points(&self) -> &[T] {
        &self.points
    }
}

impl<T: SplinePoint> Spline for SteppedSpline<T> {
    type Value = T;

    fn times(&self) -> &[f64] {
        &self.times
    }

    /// At the final time Cesium returns the penultimate control point.
    fn evaluate(&self, time: f64) -> Option<T> {
        let i = self.find_time_interval(time, None)?;
        Some(self.points[i])
    }
}
