//! CesiumJS `ConstantSpline`.

use super::spline::{Spline, SplinePoint};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ConstantSpline<T: SplinePoint> {
    value: T,
}

impl<T: SplinePoint> ConstantSpline<T> {
    pub fn new(value: T) -> Self {
        Self { value }
    }

    pub fn value(&self) -> T {
        self.value
    }
}

impl<T: SplinePoint> Spline for ConstantSpline<T> {
    type Value = T;

    fn times(&self) -> &[f64] {
        &[]
    }

    fn evaluate(&self, _time: f64) -> Option<T> {
        Some(self.value)
    }

    fn find_time_interval(&self, _time: f64, _start_index: Option<usize>) -> Option<usize> {
        None
    }

    fn wrap_time(&self, _time: f64) -> f64 {
        0.0
    }

    fn clamp_time(&self, _time: f64) -> f64 {
        0.0
    }
}
