//! CesiumJS `MorphWeightSpline`, with weights in time-major order.

use super::spline::{valid_times, Spline};

#[derive(Clone, Debug, PartialEq)]
pub struct MorphWeightSpline {
    times: Vec<f64>,
    weights: Vec<f64>,
    count: usize,
}

impl MorphWeightSpline {
    pub fn new(times: Vec<f64>, weights: Vec<f64>) -> Option<Self> {
        if !valid_times(&times) || weights.len() < 3 || !weights.len().is_multiple_of(times.len()) {
            return None;
        }
        let count = weights.len() / times.len();
        Some(Self { times, weights, count })
    }

    pub fn weights(&self) -> &[f64] {
        &self.weights
    }

    pub fn target_count(&self) -> usize {
        self.count
    }
}

impl Spline for MorphWeightSpline {
    type Value = Vec<f64>;

    fn times(&self) -> &[f64] {
        &self.times
    }

    fn evaluate(&self, time: f64) -> Option<Vec<f64>> {
        let i = self.find_time_interval(time, None)?;
        let u = (time - self.times[i]) / (self.times[i + 1] - self.times[i]);
        Some(
            (0..self.count)
                .map(|j| {
                    let index = i * self.count + j;
                    self.weights[index] * (1.0 - u) + self.weights[index + self.count] * u
                })
                .collect(),
        )
    }
}
