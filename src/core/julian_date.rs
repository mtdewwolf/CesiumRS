//! CesiumJS `JulianDate`: a whole day number and seconds into the day, in TAI.
//!
//! One deliberate difference from Cesium: times in ISO 8601 strings without
//! an offset are read as UTC (Cesium uses the machine's time zone).
//!
//! The leap-second table is process-wide, like Cesium's mutable
//! `JulianDate.leapSeconds`: it starts as [`DEFAULT_LEAP_SECONDS`] and
//! [`JulianDate::add_leap_second`] extends it (as Earth orientation data does).

use super::binary_search::binary_search;
use super::gregorian_date::GregorianDate;
use super::is_leap_year::is_leap_year;
use super::leap_second::LeapSecond;
use super::math::{js_number_to_string, js_to_fixed, to_int32};
use super::time_constants::{SECONDS_PER_DAY, SECONDS_PER_HOUR, SECONDS_PER_MILLISECOND, SECONDS_PER_MINUTE};
use super::time_standard::TimeStandard;
use std::sync::{LazyLock, RwLock};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct JulianDate {
    /// Whole days. A float because Cesium's `addDays` accepts fractions.
    pub day_number: f64,
    /// Seconds into the day, in `[0, 86400)`.
    pub seconds_of_day: f64,
}

/// Cesium's `new JulianDate()`: day 0 at noon UTC, which is 10 s later in TAI.
impl Default for JulianDate {
    fn default() -> Self {
        Self::new(0.0, 0.0, TimeStandard::Utc)
    }
}

/// An ISO 8601 string Cesium's debug build would reject.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Iso8601Error(pub String);

impl std::fmt::Display for Iso8601Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Invalid ISO 8601 date: {}", self.0)
    }
}

impl std::error::Error for Iso8601Error {}

const fn tai(day_number: f64, seconds_of_day: f64) -> JulianDate {
    JulianDate { day_number, seconds_of_day }
}

const fn leap(day: f64, seconds: f64, offset: f64) -> LeapSecond {
    LeapSecond { julian_date: tai(day, seconds), offset }
}

/// TAI − UTC from 1972 on, as in Cesium (`JulianDate.leapSeconds`).
pub const DEFAULT_LEAP_SECONDS: [LeapSecond; 28] = [
    leap(2441317.0, 43210.0, 10.0), // January 1, 1972
    leap(2441499.0, 43211.0, 11.0), // July 1, 1972
    leap(2441683.0, 43212.0, 12.0), // January 1, 1973
    leap(2442048.0, 43213.0, 13.0), // January 1, 1974
    leap(2442413.0, 43214.0, 14.0), // January 1, 1975
    leap(2442778.0, 43215.0, 15.0), // January 1, 1976
    leap(2443144.0, 43216.0, 16.0), // January 1, 1977
    leap(2443509.0, 43217.0, 17.0), // January 1, 1978
    leap(2443874.0, 43218.0, 18.0), // January 1, 1979
    leap(2444239.0, 43219.0, 19.0), // January 1, 1980
    leap(2444786.0, 43220.0, 20.0), // July 1, 1981
    leap(2445151.0, 43221.0, 21.0), // July 1, 1982
    leap(2445516.0, 43222.0, 22.0), // July 1, 1983
    leap(2446247.0, 43223.0, 23.0), // July 1, 1985
    leap(2447161.0, 43224.0, 24.0), // January 1, 1988
    leap(2447892.0, 43225.0, 25.0), // January 1, 1990
    leap(2448257.0, 43226.0, 26.0), // January 1, 1991
    leap(2448804.0, 43227.0, 27.0), // July 1, 1992
    leap(2449169.0, 43228.0, 28.0), // July 1, 1993
    leap(2449534.0, 43229.0, 29.0), // July 1, 1994
    leap(2450083.0, 43230.0, 30.0), // January 1, 1996
    leap(2450630.0, 43231.0, 31.0), // July 1, 1997
    leap(2451179.0, 43232.0, 32.0), // January 1, 1999
    leap(2453736.0, 43233.0, 33.0), // January 1, 2006
    leap(2454832.0, 43234.0, 34.0), // January 1, 2009
    leap(2456109.0, 43235.0, 35.0), // July 1, 2012
    leap(2457204.0, 43236.0, 36.0), // July 1, 2015
    leap(2457754.0, 43237.0, 37.0), // January 1, 2017
];

const DAYS_IN_MONTH: [f64; 12] = [31.0, 28.0, 31.0, 30.0, 31.0, 30.0, 31.0, 31.0, 30.0, 31.0, 30.0, 31.0];
const DAYS_IN_LEAP_FEBRUARY: f64 = 29.0;

static LEAP_SECONDS: LazyLock<RwLock<Vec<LeapSecond>>> = LazyLock::new(|| RwLock::new(DEFAULT_LEAP_SECONDS.to_vec()));

/// Runs `f` with the current leap-second table.
fn with_leap_seconds<R>(f: impl FnOnce(&[LeapSecond]) -> R) -> R {
    let table = LEAP_SECONDS.read().unwrap_or_else(|e| e.into_inner());
    f(&table)
}

fn leap_index(table: &[LeapSecond], date: &JulianDate) -> isize {
    binary_search(table, date, |l, d| JulianDate::compare(&l.julian_date, d))
}

fn set_components(mut whole_days: f64, mut seconds_of_day: f64) -> JulianDate {
    let extra_days = f64::from(to_int32(seconds_of_day / SECONDS_PER_DAY));
    whole_days += extra_days;
    seconds_of_day -= SECONDS_PER_DAY * extra_days;
    if seconds_of_day < 0.0 {
        whole_days -= 1.0;
        seconds_of_day += SECONDS_PER_DAY;
    }
    JulianDate { day_number: whole_days, seconds_of_day }
}

fn convert_utc_to_tai(date: JulianDate) -> JulianDate {
    with_leap_seconds(|table| convert_utc_to_tai_with(table, date))
}

fn convert_utc_to_tai_with(table: &[LeapSecond], date: JulianDate) -> JulianDate {
    let mut index = leap_index(table, &date);
    if index < 0 {
        index = !index;
    }
    let mut index = (index as usize).min(table.len() - 1);
    let mut offset = table[index].offset;
    if index > 0 {
        let difference = JulianDate::seconds_difference(&table[index].julian_date, &date);
        if difference > offset {
            index -= 1;
            offset = table[index].offset;
        }
    }
    date.add_seconds(offset)
}

/// `None` during a leap second, which UTC dates cannot hold.
fn convert_tai_to_utc(date: &JulianDate) -> Option<JulianDate> {
    with_leap_seconds(|table| convert_tai_to_utc_with(table, date))
}

fn convert_tai_to_utc_with(table: &[LeapSecond], date: &JulianDate) -> Option<JulianDate> {
    let mut index = leap_index(table, date);
    if index < 0 {
        index = !index;
    }
    let index = index as usize;
    if index == 0 {
        return Some(date.add_seconds(-table[0].offset));
    }
    if index >= table.len() {
        return Some(date.add_seconds(-table[index - 1].offset));
    }
    let difference = JulianDate::seconds_difference(&table[index].julian_date, date);
    if difference == 0.0 {
        return Some(date.add_seconds(-table[index].offset));
    }
    if difference <= 1.0 {
        return None;
    }
    Some(date.add_seconds(-table[index - 1].offset))
}

/// Day number and seconds of day (from noon) for UTC calendar fields.
#[allow(clippy::too_many_arguments)]
fn compute_julian_date_components(
    year: f64,
    month: f64,
    day: f64,
    hour: f64,
    minute: f64,
    second: f64,
    millisecond: f64,
) -> (f64, f64) {
    let i = |v: f64| f64::from(to_int32(v));
    let a = i((month - 14.0) / 12.0);
    let b = year + 4800.0 + a;
    let mut day_number = i((1461.0 * b) / 4.0) + i((367.0 * (month - 2.0 - 12.0 * a)) / 12.0)
        - i((3.0 * i((b + 100.0) / 100.0)) / 4.0)
        + day
        - 32075.0;
    let mut hour = hour - 12.0;
    if hour < 0.0 {
        hour += 24.0;
    }
    let seconds_of_day =
        second + (hour * SECONDS_PER_HOUR + minute * SECONDS_PER_MINUTE + millisecond * SECONDS_PER_MILLISECOND);
    if seconds_of_day >= 43200.0 {
        day_number -= 1.0;
    }
    (day_number, seconds_of_day)
}

// ---- JavaScript Date arithmetic, used where Cesium goes through `Date` ----

/// Days from 1970-01-01 to a proleptic Gregorian date.
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = if y >= 0 { y } else { y - 399 } / 400;
    let yoe = y - era * 400;
    let mp = (m + 9) % 12;
    let doy = (153 * mp + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}

/// `(year, month 1-12, day)` for days since 1970-01-01.
fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

/// `Date.UTC(year, month0, day, ...)` in milliseconds, including its quirk
/// of reading years 0-99 as 1900-1999.
fn js_date_utc(year: f64, month0: f64, day: f64, hour: f64, minute: f64, second: f64, ms: f64) -> f64 {
    let mut y = year.trunc();
    if (0.0..=99.0).contains(&y) {
        y += 1900.0;
    }
    let m = month0.trunc();
    let ym = y + (m / 12.0).floor();
    let mn = m.rem_euclid(12.0);
    let days = days_from_civil(ym as i64, mn as i64 + 1, 1) as f64 + day.trunc() - 1.0;
    let time = hour.trunc() * 3_600_000.0 + minute.trunc() * 60_000.0 + second.trunc() * 1000.0 + ms.trunc();
    days * 86_400_000.0 + time
}

/// UTC fields of a `Date` timestamp: year, month (1-12), day, hour, minute,
/// second, millisecond, and weekday (0 = Sunday).
fn js_date_fields(ms: f64) -> [f64; 8] {
    let days = (ms / 86_400_000.0).floor();
    let in_day = ms - days * 86_400_000.0;
    let (y, m, d) = civil_from_days(days as i64);
    let weekday = (days + 4.0).rem_euclid(7.0);
    [
        y as f64,
        m as f64,
        d as f64,
        (in_day / 3_600_000.0).floor(),
        ((in_day % 3_600_000.0) / 60_000.0).floor(),
        ((in_day % 60_000.0) / 1000.0).floor(),
        in_day % 1000.0,
        weekday,
    ]
}

// ---- ISO 8601 matching (Cesium's regular expressions, by hand) ----

fn digits(s: &str, n: usize) -> Option<(f64, &str)> {
    if s.len() >= n && s.as_bytes()[..n].iter().all(u8::is_ascii_digit) {
        Some((s[..n].parse().ok()?, &s[n..]))
    } else {
        None
    }
}

fn opt_char(s: &str, c: char) -> &str {
    s.strip_prefix(c).unwrap_or(s)
}

/// `^(\d{4})-?(\d{2})-?(\d{2})$`
fn match_calendar_date(s: &str) -> Option<(f64, f64, f64)> {
    let (y, r) = digits(s, 4)?;
    let (m, r) = digits(opt_char(r, '-'), 2)?;
    let (d, r) = digits(opt_char(r, '-'), 2)?;
    r.is_empty().then_some((y, m, d))
}

/// `^(\d{4})-(\d{2})$`
fn match_calendar_month(s: &str) -> Option<(f64, f64)> {
    let (y, r) = digits(s, 4)?;
    let (m, r) = digits(r.strip_prefix('-')?, 2)?;
    r.is_empty().then_some((y, m))
}

/// `^(\d{4})$`
fn match_calendar_year(s: &str) -> Option<f64> {
    let (y, r) = digits(s, 4)?;
    r.is_empty().then_some(y)
}

/// `^(\d{4})-?(\d{3})$`
fn match_ordinal_date(s: &str) -> Option<(f64, f64)> {
    let (y, r) = digits(s, 4)?;
    let (d, r) = digits(opt_char(r, '-'), 3)?;
    r.is_empty().then_some((y, d))
}

/// `^(\d{4})-?W(\d{2})-?(\d{1})?$`
fn match_week_date(s: &str) -> Option<(f64, f64, Option<f64>)> {
    let (y, r) = digits(s, 4)?;
    let r = opt_char(r, '-').strip_prefix('W')?;
    let (w, r) = digits(r, 2)?;
    let r = opt_char(r, '-');
    if r.is_empty() {
        return Some((y, w, None));
    }
    let (d, r) = digits(r, 1)?;
    r.is_empty().then_some((y, w, Some(d)))
}

/// Captures of `([Z+\-])?(\d{2})?:?(\d{2})?$` over the whole of `s`, trying
/// each optional group greedily first like a backtracking regex.
fn match_utc_offset(s: &str) -> Option<(Option<char>, Option<f64>, Option<f64>)> {
    let sign_options: Vec<(Option<char>, &str)> = match s.chars().next() {
        Some(c @ ('Z' | '+' | '-')) => vec![(Some(c), &s[1..]), (None, s)],
        _ => vec![(None, s)],
    };
    for (sign, r) in sign_options {
        let mut hour_options = Vec::new();
        if let Some((h, r2)) = digits(r, 2) {
            hour_options.push((Some(h), r2));
        }
        hour_options.push((None, r));
        for (hours, r) in hour_options {
            let mut colon_options = Vec::new();
            if let Some(r2) = r.strip_prefix(':') {
                colon_options.push(r2);
            }
            colon_options.push(r);
            for r in colon_options {
                if let Some((m, r2)) = digits(r, 2) {
                    if r2.is_empty() {
                        return Some((sign, hours, Some(m)));
                    }
                }
                if r.is_empty() {
                    return Some((sign, hours, None));
                }
            }
        }
    }
    None
}

type TimeMatch = (Vec<f64>, Option<f64>, (Option<char>, Option<f64>, Option<f64>));

/// `^(\d{2})` then `n - 1` more `:?(\d{2})`, an optional `(\.\d+)`, and the offset.
fn match_time(s: &str, fields: usize) -> Option<TimeMatch> {
    let (first, mut r) = digits(s, 2)?;
    let mut values = vec![first];
    for _ in 1..fields {
        let (v, r2) = digits(opt_char(r, ':'), 2)?;
        values.push(v);
        r = r2;
    }
    // `(\.\d+)?` is greedy but may give back digits for the offset.
    let mut frac_options: Vec<(Option<f64>, &str)> = Vec::new();
    if let Some(rest) = r.strip_prefix('.') {
        let n = rest.bytes().take_while(u8::is_ascii_digit).count();
        for k in (1..=n).rev() {
            let v: f64 = format!("0.{}", &rest[..k]).parse().ok()?;
            frac_options.push((Some(v), &rest[k..]));
        }
    }
    frac_options.push((None, r));
    for (frac, r) in frac_options {
        if let Some(offset) = match_utc_offset(r) {
            return Some((values, frac, offset));
        }
    }
    None
}

impl JulianDate {
    /// `julian_day_number` may carry a fraction of a day.
    pub fn new(julian_day_number: f64, seconds_of_day: f64, time_standard: TimeStandard) -> Self {
        let whole_days = f64::from(to_int32(julian_day_number));
        let seconds_of_day = seconds_of_day + (julian_day_number - whole_days) * SECONDS_PER_DAY;
        let date = set_components(whole_days, seconds_of_day);
        if time_standard == TimeStandard::Utc {
            convert_utc_to_tai(date)
        } else {
            date
        }
    }

    pub fn from_gregorian_date(date: &GregorianDate) -> Self {
        let (d, s) = compute_julian_date_components(
            f64::from(date.year),
            f64::from(date.month),
            f64::from(date.day),
            f64::from(date.hour),
            f64::from(date.minute),
            f64::from(date.second),
            date.millisecond,
        );
        Self::new(d, s, TimeStandard::Utc)
    }

    /// [`Self::from_gregorian_date`] on raw fields, which may be fractional
    /// (Cesium's `GregorianDate` does not truncate).
    pub(crate) fn from_gregorian_fields(f: [f64; 7]) -> Self {
        let (d, s) = compute_julian_date_components(f[0], f[1], f[2], f[3], f[4], f[5], f[6]);
        Self::new(d, s, TimeStandard::Utc)
    }

    /// From a JavaScript-style timestamp: milliseconds since 1970-01-01 UTC.
    /// Fractional milliseconds are dropped, as `Date` does.
    pub fn from_date(unix_milliseconds: f64) -> Self {
        let f = js_date_fields(unix_milliseconds.trunc());
        let (d, s) = compute_julian_date_components(f[0], f[1], f[2], f[3], f[4], f[5], f[6]);
        Self::new(d, s, TimeStandard::Utc)
    }

    pub fn now() -> Self {
        let ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_millis() as f64)
            .unwrap_or(0.0);
        Self::from_date(ms)
    }

    /// Parses calendar, ordinal, and week dates with optional times and offsets.
    pub fn from_iso8601(iso8601: &str) -> Result<Self, Iso8601Error> {
        let err = || Iso8601Error(iso8601.to_string());
        let normalized = iso8601.replacen(',', ".", 1);
        let mut tokens = normalized.split('T');
        let date = tokens.next().ok_or_else(err)?;
        let time = tokens.next();

        let (mut year, mut month, mut day);
        let (mut hour, mut minute, mut second, mut millisecond) = (0.0, 0.0, 0.0, 0.0);

        if let Some((y, m, d)) = match_calendar_date(date) {
            let dash_count = date.matches('-').count();
            if dash_count > 0 && dash_count != 2 {
                return Err(err());
            }
            (year, month, day) = (y, m, d);
        } else if let Some((y, m)) = match_calendar_month(date) {
            (year, month, day) = (y, m, 1.0);
        } else if let Some(y) = match_calendar_year(date) {
            (year, month, day) = (y, 1.0, 1.0);
        } else {
            let day_of_year;
            if let Some((y, d)) = match_ordinal_date(date) {
                year = y;
                day_of_year = d;
                let leap = is_leap_year(year);
                if day_of_year < 1.0 || (leap && day_of_year > 366.0) || (!leap && day_of_year > 365.0) {
                    return Err(err());
                }
            } else if let Some((y, week, dow)) = match_week_date(date) {
                year = y;
                let dash_count = date.matches('-').count();
                if dash_count > 0 && ((dow.is_none() && dash_count != 1) || (dow.is_some() && dash_count != 2)) {
                    return Err(err());
                }
                let day_of_week = dow.unwrap_or(0.0);
                let january4 = js_date_fields(js_date_utc(year, 0.0, 4.0, 0.0, 0.0, 0.0, 0.0));
                day_of_year = week * 7.0 + day_of_week - january4[7] - 3.0;
            } else {
                return Err(err());
            }
            // `new Date(Date.UTC(year, 0, 1)).setUTCDate(dayOfYear)`; Cesium keeps `year`.
            let f = js_date_fields(js_date_utc(year, 0.0, day_of_year, 0.0, 0.0, 0.0, 0.0));
            month = f[1];
            day = f[2];
        }

        let in_leap_year = is_leap_year(year);
        if !(1.0..=12.0).contains(&month)
            || day < 1.0
            || ((month != 2.0 || !in_leap_year) && day > DAYS_IN_MONTH[month as usize - 1])
            || (in_leap_year && month == 2.0 && day > DAYS_IN_LEAP_FEBRUARY)
        {
            return Err(err());
        }

        if let Some(time) = time {
            let (offset, offset_hours, offset_minutes);
            if let Some((v, frac, off)) = match_time(time, 3) {
                let colons = time.matches(':').count();
                if colons > 0 && colons != 2 && colons != 3 {
                    return Err(err());
                }
                hour = v[0];
                minute = v[1];
                second = v[2];
                millisecond = frac.unwrap_or(0.0) * 1000.0;
                (offset, offset_hours, offset_minutes) = off;
            } else if let Some((v, frac, off)) = match_time(time, 2) {
                if time.matches(':').count() > 2 {
                    return Err(err());
                }
                hour = v[0];
                minute = v[1];
                second = frac.unwrap_or(0.0) * 60.0;
                (offset, offset_hours, offset_minutes) = off;
            } else if let Some((v, frac, off)) = match_time(time, 1) {
                hour = v[0];
                minute = frac.unwrap_or(0.0) * 60.0;
                (offset, offset_hours, offset_minutes) = off;
            } else {
                return Err(err());
            }
            if minute >= 60.0
                || second >= 61.0
                || hour > 24.0
                || (hour == 24.0 && (minute > 0.0 || second > 0.0 || millisecond > 0.0))
            {
                return Err(err());
            }
            // `+undefined` is NaN in JS.
            let offset_hours = offset_hours.unwrap_or(f64::NAN);
            let offset_minutes = offset_minutes.unwrap_or(0.0);
            match offset {
                Some('+') => {
                    hour -= offset_hours;
                    minute -= offset_minutes;
                }
                Some('-') => {
                    hour += offset_hours;
                    minute += offset_minutes;
                }
                // `Z`, or no offset: read as UTC here (Cesium: local time).
                _ => {}
            }
        }

        let is_leap_second = second == 60.0;
        if is_leap_second {
            second -= 1.0;
        }
        let month_length = |month: f64| -> f64 {
            if in_leap_year && month == 2.0 {
                DAYS_IN_LEAP_FEBRUARY
            } else if (1.0..=12.0).contains(&month) && month.fract() == 0.0 {
                DAYS_IN_MONTH[month as usize - 1]
            } else {
                f64::NAN
            }
        };
        while minute >= 60.0 {
            minute -= 60.0;
            hour += 1.0;
        }
        while hour >= 24.0 {
            hour -= 24.0;
            day += 1.0;
        }
        let mut tmp = month_length(month);
        while day > tmp {
            day -= tmp;
            month += 1.0;
            if month > 12.0 {
                month -= 12.0;
                year += 1.0;
            }
            tmp = month_length(month);
        }
        while minute < 0.0 {
            minute += 60.0;
            hour -= 1.0;
        }
        while hour < 0.0 {
            hour += 24.0;
            day -= 1.0;
        }
        while day < 1.0 {
            month -= 1.0;
            if month < 1.0 {
                month += 12.0;
                year -= 1.0;
            }
            day += month_length(month);
        }
        let (d, s) = compute_julian_date_components(year, month, day, hour, minute, second, millisecond);
        let mut result = Self::new(d, s, TimeStandard::Utc);
        if is_leap_second {
            result = result.add_seconds(1.0);
        }
        Ok(result)
    }

    pub fn to_gregorian_date(&self) -> GregorianDate {
        let mut is_leap_second = false;
        let this_utc = match convert_tai_to_utc(self) {
            Some(d) => d,
            None => {
                is_leap_second = true;
                convert_tai_to_utc(&self.add_seconds(-1.0)).expect("one second before a leap second is valid UTC")
            }
        };
        let mut julian_day_number = this_utc.day_number;
        let seconds_of_day = this_utc.seconds_of_day;
        if seconds_of_day >= 43200.0 {
            julian_day_number += 1.0;
        }
        let i = |v: f64| f64::from(to_int32(v));
        let mut l = i(julian_day_number + 68569.0);
        let n = i((4.0 * l) / 146097.0);
        l = i(l - i((146097.0 * n + 3.0) / 4.0));
        let ii = i((4000.0 * (l + 1.0)) / 1461001.0);
        l = i(l - i((1461.0 * ii) / 4.0) + 31.0);
        let j = i((80.0 * l) / 2447.0);
        let day = i(l - i((2447.0 * j) / 80.0));
        l = i(j / 11.0);
        let month = i(j + 2.0 - 12.0 * l);
        let year = i(100.0 * (n - 49.0) + ii + l);
        let mut hour = i(seconds_of_day / SECONDS_PER_HOUR);
        let mut remaining = seconds_of_day - hour * SECONDS_PER_HOUR;
        let minute = i(remaining / SECONDS_PER_MINUTE);
        remaining -= minute * SECONDS_PER_MINUTE;
        let mut second = i(remaining);
        let millisecond = (remaining - second) / SECONDS_PER_MILLISECOND;
        hour += 12.0;
        if hour > 23.0 {
            hour -= 24.0;
        }
        if is_leap_second {
            second += 1.0;
        }
        GregorianDate {
            year: year as i32,
            month: month as i32,
            day: day as i32,
            hour: hour as i32,
            minute: minute as i32,
            second: second as i32,
            millisecond,
            is_leap_second,
        }
    }

    /// JavaScript-style timestamp (milliseconds since 1970-01-01 UTC), like
    /// `JulianDate.toDate(...).getTime()`.
    pub fn to_date(&self) -> f64 {
        let g = self.to_gregorian_date();
        let second = if g.is_leap_second { g.second - 1 } else { g.second };
        js_date_utc(
            f64::from(g.year),
            f64::from(g.month - 1),
            f64::from(g.day),
            f64::from(g.hour),
            f64::from(g.minute),
            f64::from(second),
            g.millisecond,
        )
    }

    /// `YYYY-MM-DDThh:mm:ss[.fff]Z`. Without `precision`, as many fraction
    /// digits as needed; with it, exactly that many.
    pub fn to_iso8601(&self, precision: Option<usize>) -> String {
        let g = self.to_gregorian_date();
        let (mut year, mut month, mut day, mut hour) = (g.year, g.month, g.day, g.hour);
        let (minute, second, millisecond) = (g.minute, g.second, g.millisecond);
        if year == 10000 && month == 1 && day == 1 && hour == 0 && minute == 0 && second == 0 && millisecond == 0.0 {
            year = 9999;
            month = 12;
            day = 31;
            hour = 24;
        }
        let base = format!("{year:04}-{month:02}-{day:02}T{hour:02}:{minute:02}:{second:02}");
        match precision {
            None if millisecond != 0.0 => {
                let hundreds = millisecond * 0.01;
                let ms = if hundreds < 1e-6 {
                    js_to_fixed(hundreds, 20).replacen('.', "", 1).trim_end_matches('0').to_string()
                } else {
                    js_number_to_string(hundreds).replacen('.', "", 1)
                };
                format!("{base}.{ms}Z")
            }
            None | Some(0) => format!("{base}Z"),
            Some(p) => {
                let ms: String = js_to_fixed(millisecond * 0.01, p).replacen('.', "", 1).chars().take(p).collect();
                format!("{base}.{ms}Z")
            }
        }
    }

    /// Positive if `left` is later, in days or (on the same day) seconds.
    pub fn compare(left: &Self, right: &Self) -> f64 {
        let day_difference = left.day_number - right.day_number;
        if day_difference != 0.0 {
            return day_difference;
        }
        left.seconds_of_day - right.seconds_of_day
    }

    pub fn equals(&self, right: &Self) -> bool {
        self == right
    }

    pub fn equals_epsilon(&self, right: &Self, epsilon: f64) -> bool {
        Self::seconds_difference(self, right).abs() <= epsilon
    }

    pub fn total_days(&self) -> f64 {
        self.day_number + self.seconds_of_day / SECONDS_PER_DAY
    }

    pub fn seconds_difference(left: &Self, right: &Self) -> f64 {
        let day_difference = (left.day_number - right.day_number) * SECONDS_PER_DAY;
        day_difference + (left.seconds_of_day - right.seconds_of_day)
    }

    pub fn days_difference(left: &Self, right: &Self) -> f64 {
        let day_difference = left.day_number - right.day_number;
        let second_difference = (left.seconds_of_day - right.seconds_of_day) / SECONDS_PER_DAY;
        day_difference + second_difference
    }

    /// TAI − UTC at this date, in seconds.
    pub fn compute_tai_minus_utc(&self) -> f64 {
        with_leap_seconds(|table| {
            let mut index = leap_index(table, self);
            if index < 0 {
                index = !index;
                index -= 1;
                if index < 0 {
                    index = 0;
                }
            }
            table[index as usize].offset
        })
    }

    /// A copy of the current leap-second table (Cesium's `JulianDate.leapSeconds`).
    pub fn leap_seconds() -> Vec<LeapSecond> {
        with_leap_seconds(|table| table.to_vec())
    }

    /// Inserts a leap second in date order unless one already exists at that
    /// date. Returns whether it was added.
    pub fn add_leap_second(leap_second: LeapSecond) -> bool {
        let mut table = LEAP_SECONDS.write().unwrap_or_else(|e| e.into_inner());
        let index = leap_index(&table, &leap_second.julian_date);
        if index >= 0 {
            return false;
        }
        table.insert((!index) as usize, leap_second);
        true
    }

    /// Restores [`DEFAULT_LEAP_SECONDS`].
    pub fn reset_leap_seconds() {
        *LEAP_SECONDS.write().unwrap_or_else(|e| e.into_inner()) = DEFAULT_LEAP_SECONDS.to_vec();
    }

    pub fn add_seconds(&self, seconds: f64) -> Self {
        set_components(self.day_number, self.seconds_of_day + seconds)
    }

    pub fn add_minutes(&self, minutes: f64) -> Self {
        set_components(self.day_number, self.seconds_of_day + minutes * SECONDS_PER_MINUTE)
    }

    pub fn add_hours(&self, hours: f64) -> Self {
        set_components(self.day_number, self.seconds_of_day + hours * SECONDS_PER_HOUR)
    }

    pub fn add_days(&self, days: f64) -> Self {
        set_components(self.day_number + days, self.seconds_of_day)
    }

    pub fn less_than(left: &Self, right: &Self) -> bool {
        Self::compare(left, right) < 0.0
    }

    pub fn less_than_or_equals(left: &Self, right: &Self) -> bool {
        Self::compare(left, right) <= 0.0
    }

    pub fn greater_than(left: &Self, right: &Self) -> bool {
        Self::compare(left, right) > 0.0
    }

    pub fn greater_than_or_equals(left: &Self, right: &Self) -> bool {
        Self::compare(left, right) >= 0.0
    }
}

impl std::fmt::Display for JulianDate {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.to_iso8601(None))
    }
}
