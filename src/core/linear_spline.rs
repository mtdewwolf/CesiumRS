//! CesiumJS `LinearSpline` for scalar or Cartesian3 control points.
//!
//! ```
//! use cesium_rs::core::{LinearSpline, Spline};
//!
//! let spline = LinearSpline::new(vec![0.0, 2.0], vec![10.0, 20.0]).unwrap();
//! assert_eq!(spline.evaluate(1.0), Some(15.0));
//! assert_eq!(spline.evaluate(3.0), None);
//! assert_eq!(spline.evaluate(spline.clamp_time(3.0)), Some(20.0));
//! ```

use super::spline::{valid_times, LinearPoint, Spline};

#[derive(Clone, Debug, PartialEq)]
pub struct LinearSpline<T: LinearPoint> {
    times: Vec<f64>,
    points: Vec<T>,
}

impl<T: LinearPoint> LinearSpline<T> {
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

impl<T: LinearPoint> Spline for LinearSpline<T> {
    type Value = T;

    fn times(&self) -> &[f64] {
        &self.times
    }

    fn evaluate(&self, time: f64) -> Option<T> {
        let i = self.find_time_interval(time, None)?;
        let u = (time - self.times[i]) / (self.times[i + 1] - self.times[i]);
        Some(T::interpolate(self.points[i], self.points[i + 1], u))
    }
}
