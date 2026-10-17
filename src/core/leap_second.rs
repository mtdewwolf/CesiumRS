//! CesiumJS `LeapSecond`: a TAI date and the TAI − UTC offset from then on.

use super::julian_date::JulianDate;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct LeapSecond {
    pub julian_date: JulianDate,
    pub offset: f64,
}
