//! CesiumJS `GregorianDate`: calendar fields of a UTC time.

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct GregorianDate {
    pub year: i32,
    pub month: i32,
    pub day: i32,
    pub hour: i32,
    pub minute: i32,
    /// `60` during a leap second.
    pub second: i32,
    pub millisecond: f64,
    pub is_leap_second: bool,
}

const DAYS_IN_YEAR: [i32; 12] = [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31];

impl Default for GregorianDate {
    fn default() -> Self {
        Self { year: 1, month: 1, day: 1, hour: 0, minute: 0, second: 0, millisecond: 0.0, is_leap_second: false }
    }
}

impl GregorianDate {
    /// `None` for out-of-range fields or a day past the end of the month,
    /// where Cesium throws.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        year: i32,
        month: i32,
        day: i32,
        hour: i32,
        minute: i32,
        second: i32,
        millisecond: f64,
        is_leap_second: bool,
    ) -> Option<Self> {
        let max_second = if is_leap_second { 60 } else { 59 };
        if !(1..=9999).contains(&year)
            || !(1..=12).contains(&month)
            || !(1..=31).contains(&day)
            || !(0..=23).contains(&hour)
            || !(0..=59).contains(&minute)
            || !(0..=max_second).contains(&second)
            || !(0.0..1000.0).contains(&millisecond)
        {
            return None;
        }
        let leap = super::is_leap_year::is_leap_year(f64::from(year));
        let days = if month == 2 && leap { 29 } else { DAYS_IN_YEAR[month as usize - 1] };
        if day > days {
            return None;
        }
        Some(Self { year, month, day, hour, minute, second, millisecond, is_leap_second })
    }
}
