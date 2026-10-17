//! CesiumJS `Spline`, expressed as a trait with shared time operations.
//!
//! Runtime point-type dispatch becomes sealed Rust traits. Concrete splines
//! own their control data and return `None` for an invalid domain or time.

use super::{math, Cartesian3, Quaternion};

/// A time-parameterized curve. Import this trait to evaluate concrete splines
/// and use their common interval, wrapping, and clamping methods.
pub trait Spline {
    type Value;

    /// Immutable control-point times. A constant spline returns an empty slice.
    fn times(&self) -> &[f64];
    fn evaluate(&self, time: f64) -> Option<Self::Value>;

    fn find_time_interval(&self, time: f64, start_index: Option<usize>) -> Option<usize> {
        find_time_interval(self.times(), time, start_index)
    }

    fn wrap_time(&self, time: f64) -> f64 {
        wrap_time(self.times(), time)
    }

    fn clamp_time(&self, time: f64) -> f64 {
        let times = self.times();
        if times.is_empty() {
            return f64::NAN;
        }
        math::clamp(time, times[0], times[times.len() - 1])
    }
}

/// Finds the interval containing `time` in a strictly increasing time array.
/// The final time belongs to the final interval, whose index is `len - 2`.
/// A supplied hint must also identify an interval (not the final point).
pub fn find_time_interval(times: &[f64], time: f64, start_index: Option<usize>) -> Option<usize> {
    if times.len() < 2 || !time.is_finite() || time < times[0] || time > times[times.len() - 1] {
        return None;
    }
    let start = start_index.unwrap_or(0);
    if start >= times.len() - 1 {
        return None;
    }
    if time >= times[start] {
        if time < times[start + 1] {
            return Some(start);
        }
        if start + 2 < times.len() && time < times[start + 2] {
            return Some(start + 1);
        }
    } else if start > 0 && time >= times[start - 1] {
        return Some(start - 1);
    }
    Some(times.partition_point(|&t| t <= time).checked_sub(1)?.min(times.len() - 2))
}

/// Matches Cesium's endpoint-sensitive wrapping, rather than a modulo: an
/// exact period below the start wraps to the end, and above the end to start.
pub fn wrap_time(times: &[f64], mut time: f64) -> f64 {
    if times.len() < 2 {
        return f64::NAN;
    }
    let start = times[0];
    let end = times[times.len() - 1];
    let stretch = end - start;
    if time < start {
        let periods = ((start - time) / stretch).floor() + 1.0;
        time += periods * stretch;
    }
    if time > end {
        let periods = ((time - end) / stretch).floor() + 1.0;
        time -= periods * stretch;
    }
    time
}

pub(crate) fn valid_times(times: &[f64]) -> bool {
    times.len() >= 2 && times.iter().all(|v| v.is_finite()) && times.windows(2).all(|pair| pair[0] < pair[1])
}

mod sealed {
    pub trait Sealed {}
    impl Sealed for f64 {}
    impl Sealed for super::Cartesian3 {}
    impl Sealed for super::Quaternion {}
}

/// Cesium's supported scalar, Cartesian3, and quaternion control-point types.
pub trait SplinePoint: sealed::Sealed + Copy {}
impl SplinePoint for f64 {}
impl SplinePoint for Cartesian3 {}
impl SplinePoint for Quaternion {}

/// The two point types supported by a linear spline. Quaternion animation
/// uses `QuaternionSpline` instead.
pub trait LinearPoint: SplinePoint {
    fn interpolate(start: Self, end: Self, u: f64) -> Self;
}

impl LinearPoint for f64 {
    fn interpolate(start: Self, end: Self, u: f64) -> Self {
        (1.0 - u) * start + u * end
    }
}

impl LinearPoint for Cartesian3 {
    fn interpolate(start: Self, end: Self, u: f64) -> Self {
        start.lerp(end, u)
    }
}
