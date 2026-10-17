//! CesiumJS `isLeapYear`.

/// Gregorian leap year rule.
pub fn is_leap_year(year: f64) -> bool {
    (year % 4.0 == 0.0 && year % 100.0 != 0.0) || year % 400.0 == 0.0
}
