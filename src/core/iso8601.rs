//! CesiumJS `Iso8601`: the earliest and latest representable times.

use super::julian_date::JulianDate;
use super::time_interval::TimeInterval;

/// `0000-01-01T00:00:00Z`.
pub fn minimum_value() -> JulianDate {
    JulianDate::from_iso8601("0000-01-01T00:00:00Z").expect("valid constant")
}

/// `9999-12-31T24:00:00Z`.
pub fn maximum_value() -> JulianDate {
    JulianDate::from_iso8601("9999-12-31T24:00:00Z").expect("valid constant")
}

/// The interval between [`minimum_value`] and [`maximum_value`], both included.
pub fn maximum_interval<T>() -> TimeInterval<T> {
    TimeInterval::new(minimum_value(), maximum_value(), true, true, None)
}
