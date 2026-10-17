//! CesiumJS `TimeIntervalCollection`: a sorted, non-overlapping set of time
//! intervals, merged or split as intervals are added and removed.
//!
//! Cesium compares interval data with `===` unless given a comparer; the
//! `*_with` methods take one, and the plain methods use `PartialEq`.

use super::binary_search::binary_search;
use super::event::Event;
use super::is_leap_year::is_leap_year;
use super::iso8601;
use super::julian_date::{Iso8601Error, JulianDate};
use super::time_interval::{DataMerge, TimeInterval};

pub struct TimeIntervalCollection<T> {
    intervals: Vec<TimeInterval<T>>,
    /// Raised whenever the intervals change.
    pub changed_event: Event<()>,
}

impl<T> Default for TimeIntervalCollection<T> {
    fn default() -> Self {
        Self { intervals: Vec::new(), changed_event: Event::new() }
    }
}

impl<T: std::fmt::Debug> std::fmt::Debug for TimeIntervalCollection<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_list().entries(&self.intervals).finish()
    }
}

/// Options for building a collection from a list of dates.
#[derive(Clone, Copy, Debug)]
pub struct DateArrayOptions {
    pub is_start_included: bool,
    pub is_stop_included: bool,
    /// Add an interval from [`iso8601::minimum_value`] to the first date.
    pub leading_interval: bool,
    /// Add an interval from the last date to [`iso8601::maximum_value`].
    pub trailing_interval: bool,
}

impl Default for DateArrayOptions {
    fn default() -> Self {
        Self { is_start_included: true, is_stop_included: true, leading_interval: false, trailing_interval: false }
    }
}

fn compare_start(left: &TimeInterval<impl Sized>, right: &JulianDate) -> f64 {
    JulianDate::compare(&left.start, right)
}

impl<T> TimeIntervalCollection<T> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn len(&self) -> usize {
        self.intervals.len()
    }

    pub fn is_empty(&self) -> bool {
        self.intervals.is_empty()
    }

    pub fn intervals(&self) -> &[TimeInterval<T>] {
        &self.intervals
    }

    pub fn get(&self, index: usize) -> Option<&TimeInterval<T>> {
        self.intervals.get(index)
    }

    pub fn start(&self) -> Option<JulianDate> {
        self.intervals.first().map(|i| i.start)
    }

    pub fn is_start_included(&self) -> bool {
        self.intervals.first().is_some_and(|i| i.is_start_included)
    }

    pub fn stop(&self) -> Option<JulianDate> {
        self.intervals.last().map(|i| i.stop)
    }

    pub fn is_stop_included(&self) -> bool {
        self.intervals.last().is_some_and(|i| i.is_stop_included)
    }

    pub fn remove_all(&mut self) {
        if !self.intervals.is_empty() {
            self.intervals.clear();
            self.changed_event.raise_event(&());
        }
    }

    /// Index of the interval containing `date`, or the complement of where
    /// one would be inserted.
    pub fn index_of(&self, date: &JulianDate) -> isize {
        let intervals = &self.intervals;
        let index = binary_search(intervals, date, compare_start);
        if index >= 0 {
            let i = index as usize;
            if intervals[i].is_start_included {
                return index;
            }
            if i > 0 && intervals[i - 1].stop == *date && intervals[i - 1].is_stop_included {
                return index - 1;
            }
            return !index;
        }
        let index = !index;
        let i = index as usize;
        if i > 0 && i - 1 < intervals.len() && intervals[i - 1].contains(date) {
            return index - 1;
        }
        !index
    }

    pub fn contains(&self, date: &JulianDate) -> bool {
        self.index_of(date) >= 0
    }

    pub fn find_interval_containing_date(&self, date: &JulianDate) -> Option<&TimeInterval<T>> {
        let index = self.index_of(date);
        (index >= 0).then(|| &self.intervals[index as usize])
    }

    pub fn find_data_for_interval_containing_date(&self, date: &JulianDate) -> Option<&T> {
        self.find_interval_containing_date(date).and_then(|i| i.data.as_ref())
    }

    /// First interval matching every given criterion.
    pub fn find_interval(
        &self,
        start: Option<&JulianDate>,
        stop: Option<&JulianDate>,
        is_start_included: Option<bool>,
        is_stop_included: Option<bool>,
    ) -> Option<&TimeInterval<T>> {
        self.intervals.iter().find(|i| {
            start.is_none_or(|s| i.start == *s)
                && stop.is_none_or(|s| i.stop == *s)
                && is_start_included.is_none_or(|v| i.is_start_included == v)
                && is_stop_included.is_none_or(|v| i.is_stop_included == v)
        })
    }
}

impl<T: Clone> TimeIntervalCollection<T> {
    /// Adds an interval. Where it overlaps an existing interval with equal
    /// data (by `same`), they merge; otherwise the new interval wins.
    pub fn add_interval_with(&mut self, interval: TimeInterval<T>, same: impl Fn(&Option<T>, &Option<T>) -> bool) {
        if interval.is_empty() {
            return;
        }
        let mut interval = interval;
        let intervals = &mut self.intervals;
        if intervals.is_empty() || JulianDate::greater_than(&interval.start, &intervals[intervals.len() - 1].stop) {
            intervals.push(interval);
            self.changed_event.raise_event(&());
            return;
        }

        let found = binary_search(intervals, &interval.start, compare_start);
        let mut index: usize;
        if found < 0 {
            index = (!found) as usize;
        } else {
            index = found as usize;
            if index > 0
                && interval.is_start_included
                && intervals[index - 1].is_start_included
                && intervals[index - 1].start == interval.start
            {
                index -= 1;
            } else if index < intervals.len()
                && !interval.is_start_included
                && intervals[index].is_start_included
                && intervals[index].start == interval.start
            {
                index += 1;
            }
        }

        if index > 0 {
            let comparison = JulianDate::compare(&intervals[index - 1].stop, &interval.start);
            if comparison > 0.0
                || (comparison == 0.0 && (intervals[index - 1].is_stop_included || interval.is_start_included))
            {
                if same(&intervals[index - 1].data, &interval.data) {
                    let prev = &intervals[index - 1];
                    interval = if JulianDate::greater_than(&interval.stop, &prev.stop) {
                        TimeInterval::new(
                            prev.start,
                            interval.stop,
                            prev.is_start_included,
                            interval.is_stop_included,
                            interval.data,
                        )
                    } else {
                        TimeInterval::new(
                            prev.start,
                            prev.stop,
                            prev.is_start_included,
                            prev.is_stop_included || (interval.stop == prev.stop && interval.is_stop_included),
                            interval.data,
                        )
                    };
                    intervals.remove(index - 1);
                    index -= 1;
                } else {
                    let comparison = JulianDate::compare(&intervals[index - 1].stop, &interval.stop);
                    if comparison > 0.0
                        || (comparison == 0.0 && intervals[index - 1].is_stop_included && !interval.is_stop_included)
                    {
                        let prev = &intervals[index - 1];
                        let tail = TimeInterval::new(
                            interval.stop,
                            prev.stop,
                            !interval.is_stop_included,
                            prev.is_stop_included,
                            prev.data.clone(),
                        );
                        intervals.insert(index, tail);
                    }
                    let prev = &intervals[index - 1];
                    intervals[index - 1] = TimeInterval::new(
                        prev.start,
                        interval.start,
                        prev.is_start_included,
                        !interval.is_start_included,
                        prev.data.clone(),
                    );
                }
            }
        }

        while index < intervals.len() {
            let comparison = JulianDate::compare(&interval.stop, &intervals[index].start);
            if comparison > 0.0
                || (comparison == 0.0 && (interval.is_stop_included || intervals[index].is_start_included))
            {
                if same(&intervals[index].data, &interval.data) {
                    let next = &intervals[index];
                    let later = JulianDate::greater_than(&next.stop, &interval.stop);
                    interval = TimeInterval::new(
                        interval.start,
                        if later { next.stop } else { interval.stop },
                        interval.is_start_included,
                        if later { next.is_stop_included } else { interval.is_stop_included },
                        interval.data,
                    );
                    intervals.remove(index);
                } else {
                    let next = &intervals[index];
                    intervals[index] = TimeInterval::new(
                        interval.stop,
                        next.stop,
                        !interval.is_stop_included,
                        next.is_stop_included,
                        next.data.clone(),
                    );
                    if intervals[index].is_empty() {
                        intervals.remove(index);
                    } else {
                        break;
                    }
                }
            } else {
                break;
            }
        }
        intervals.insert(index, interval);
        self.changed_event.raise_event(&());
    }

    /// Removes the time covered by `interval`, splitting intervals as needed.
    /// `same` decides whether the intervals on either side of a removed
    /// point can merge back. Returns `true` if anything changed.
    pub fn remove_interval_with(
        &mut self,
        interval: &TimeInterval<T>,
        same: impl Fn(&Option<T>, &Option<T>) -> bool,
    ) -> bool {
        if interval.is_empty() {
            return false;
        }
        let intervals = &mut self.intervals;
        let found = binary_search(intervals, &interval.start, compare_start);
        let mut index = if found < 0 { (!found) as usize } else { found as usize };
        let mut result = false;

        if index > 0
            && (JulianDate::greater_than(&intervals[index - 1].stop, &interval.start)
                || (intervals[index - 1].stop == interval.start
                    && intervals[index - 1].is_stop_included
                    && interval.is_start_included))
        {
            result = true;
            let prev = &intervals[index - 1];
            if JulianDate::greater_than(&prev.stop, &interval.stop)
                || (prev.is_stop_included && !interval.is_stop_included && prev.stop == interval.stop)
            {
                let tail = TimeInterval::new(
                    interval.stop,
                    prev.stop,
                    !interval.is_stop_included,
                    prev.is_stop_included,
                    prev.data.clone(),
                );
                intervals.insert(index, tail);
            }
            let prev = &intervals[index - 1];
            intervals[index - 1] = TimeInterval::new(
                prev.start,
                interval.start,
                prev.is_start_included,
                !interval.is_start_included,
                prev.data.clone(),
            );
        }

        if index < intervals.len()
            && !interval.is_start_included
            && intervals[index].is_start_included
            && interval.start == intervals[index].start
        {
            result = true;
            let next = &intervals[index];
            let point = TimeInterval::new(next.start, next.start, true, true, next.data.clone());
            intervals.insert(index, point);
            index += 1;
        }

        while index < intervals.len() && JulianDate::greater_than(&interval.stop, &intervals[index].stop) {
            result = true;
            intervals.remove(index);
        }

        if index < intervals.len() && interval.stop == intervals[index].stop {
            result = true;
            if !interval.is_stop_included && intervals[index].is_stop_included {
                if index + 1 < intervals.len()
                    && intervals[index + 1].start == interval.stop
                    && same(&intervals[index].data, &intervals[index + 1].data)
                {
                    intervals.remove(index);
                    let next = &intervals[index];
                    intervals[index] =
                        TimeInterval::new(next.start, next.stop, true, next.is_stop_included, next.data.clone());
                } else {
                    let cur = &intervals[index];
                    intervals[index] = TimeInterval::new(interval.stop, interval.stop, true, true, cur.data.clone());
                }
            } else {
                intervals.remove(index);
            }
        }

        if index < intervals.len()
            && (JulianDate::greater_than(&interval.stop, &intervals[index].start)
                || (interval.stop == intervals[index].start
                    && interval.is_stop_included
                    && intervals[index].is_start_included))
        {
            result = true;
            let next = &intervals[index];
            intervals[index] = TimeInterval::new(
                interval.stop,
                next.stop,
                !interval.is_stop_included,
                next.is_stop_included,
                next.data.clone(),
            );
        }

        if result {
            self.changed_event.raise_event(&());
        }
        result
    }

    /// Overlap of two collections. Intervals pair up when `same` accepts
    /// their data or a `merge` function is given.
    pub fn intersect_with(
        &self,
        other: &Self,
        same: impl Fn(&Option<T>, &Option<T>) -> bool + Copy,
        merge: Option<DataMerge<'_, T>>,
    ) -> Self {
        let mut result = Self::new();
        let (mut left, mut right) = (0, 0);
        let intervals = &self.intervals;
        let other_intervals = &other.intervals;
        while left < intervals.len() && right < other_intervals.len() {
            let l = &intervals[left];
            let r = &other_intervals[right];
            if JulianDate::less_than(&l.stop, &r.start) {
                left += 1;
            } else if JulianDate::less_than(&r.stop, &l.start) {
                right += 1;
            } else {
                if merge.is_some() || same(&l.data, &r.data) {
                    let intersection = l.intersect(r, merge);
                    if !intersection.is_empty() {
                        result.add_interval_with(intersection, same);
                    }
                }
                if JulianDate::less_than(&l.stop, &r.stop)
                    || (l.stop == r.stop && !l.is_stop_included && r.is_stop_included)
                {
                    left += 1;
                } else {
                    right += 1;
                }
            }
        }
        result
    }

    /// Intervals between consecutive dates; `data` gives each interval's
    /// data from the interval and its index (Cesium uses the index).
    pub fn from_julian_date_array(
        julian_dates: &[JulianDate],
        options: DateArrayOptions,
        mut data: impl FnMut(&TimeInterval<T>, usize) -> Option<T>,
        same: impl Fn(&Option<T>, &Option<T>) -> bool + Copy,
    ) -> Self {
        let mut result = Self::new();
        let length = julian_dates.len();
        let mut start_index = 0;
        let mut add = |result: &mut Self, mut interval: TimeInterval<T>| {
            interval.data = data(&interval, result.len());
            result.add_interval_with(interval, same);
        };
        if options.leading_interval {
            start_index += 1;
            let interval =
                TimeInterval::new(iso8601::minimum_value(), julian_dates[0], true, !options.is_start_included, None);
            add(&mut result, interval);
        }
        for i in 0..length.saturating_sub(1) {
            let interval = TimeInterval::new(
                julian_dates[i],
                julian_dates[i + 1],
                if result.len() == start_index { options.is_start_included } else { true },
                if i == length - 2 { options.is_stop_included } else { false },
                None,
            );
            add(&mut result, interval);
        }
        if options.trailing_interval {
            let interval = TimeInterval::new(
                julian_dates[length - 1],
                iso8601::maximum_value(),
                !options.is_stop_included,
                true,
                None,
            );
            add(&mut result, interval);
        }
        result
    }

    /// `start/stop[/duration]` in ISO 8601: one interval, or one per
    /// repetition of the duration.
    pub fn from_iso8601(
        iso8601: &str,
        options: DateArrayOptions,
        data: impl FnMut(&TimeInterval<T>, usize) -> Option<T>,
        same: impl Fn(&Option<T>, &Option<T>) -> bool + Copy,
    ) -> Result<Self, Iso8601Error> {
        let dates: Vec<&str> = iso8601.split('/').collect();
        let start = JulianDate::from_iso8601(dates[0])?;
        let stop = JulianDate::from_iso8601(dates.get(1).copied().unwrap_or(""))?;
        let mut julian_dates = Vec::new();
        match parse_duration(dates.get(2).copied())? {
            None => {
                julian_dates.push(start);
                julian_dates.push(stop);
            }
            Some(duration) => {
                let mut date = start;
                julian_dates.push(date);
                while JulianDate::compare(&date, &stop) < 0.0 {
                    date = add_to_date(&date, &duration);
                    if JulianDate::compare(&stop, &date) <= 0.0 {
                        date = stop;
                    }
                    julian_dates.push(date);
                }
            }
        }
        Ok(Self::from_julian_date_array(&julian_dates, options, data, same))
    }

    pub fn from_iso8601_date_array(
        iso8601_dates: &[&str],
        options: DateArrayOptions,
        data: impl FnMut(&TimeInterval<T>, usize) -> Option<T>,
        same: impl Fn(&Option<T>, &Option<T>) -> bool + Copy,
    ) -> Result<Self, Iso8601Error> {
        let dates = iso8601_dates.iter().map(|d| JulianDate::from_iso8601(d)).collect::<Result<Vec<_>, _>>()?;
        Ok(Self::from_julian_date_array(&dates, options, data, same))
    }

    /// Dates from `epoch` plus each duration (or the previous date plus each,
    /// with `relative_to_previous`). Zero durations after the first are skipped.
    pub fn from_iso8601_duration_array(
        epoch: &JulianDate,
        iso8601_durations: &[&str],
        relative_to_previous: bool,
        options: DateArrayOptions,
        data: impl FnMut(&TimeInterval<T>, usize) -> Option<T>,
        same: impl Fn(&Option<T>, &Option<T>) -> bool + Copy,
    ) -> Result<Self, Iso8601Error> {
        let mut julian_dates: Vec<JulianDate> = Vec::new();
        let mut previous: Option<JulianDate> = None;
        for (i, d) in iso8601_durations.iter().enumerate() {
            let parsed = parse_duration_parts(Some(d))?;
            if parsed.nonzero || i == 0 {
                let base = match (relative_to_previous, previous) {
                    (true, Some(p)) => p,
                    _ => *epoch,
                };
                let date = add_to_date(&base, &parsed.duration);
                julian_dates.push(date);
                previous = Some(date);
            }
        }
        Ok(Self::from_julian_date_array(&julian_dates, options, data, same))
    }
}

impl<T: Clone + PartialEq> TimeIntervalCollection<T> {
    pub fn from_intervals(intervals: impl IntoIterator<Item = TimeInterval<T>>) -> Self {
        let mut c = Self::new();
        for i in intervals {
            c.add_interval(i);
        }
        c
    }

    pub fn add_interval(&mut self, interval: TimeInterval<T>) {
        self.add_interval_with(interval, |a, b| a == b);
    }

    pub fn remove_interval(&mut self, interval: &TimeInterval<T>) -> bool {
        self.remove_interval_with(interval, |a, b| a == b)
    }

    pub fn intersect(&self, other: &Self) -> Self {
        self.intersect_with(other, |a, b| a == b, None)
    }

    pub fn equals(&self, right: &Self) -> bool {
        self.intervals.len() == right.intervals.len()
            && self.intervals.iter().zip(&right.intervals).all(|(a, b)| a.equals(b))
    }
}

/// An ISO 8601 duration's fields. Fractions are kept, as in Cesium.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Duration {
    pub year: f64,
    pub month: f64,
    pub day: f64,
    pub hour: f64,
    pub minute: f64,
    pub second: f64,
    pub millisecond: f64,
}

struct ParsedDuration {
    duration: Duration,
    nonzero: bool,
}

fn number(s: &str) -> f64 {
    let t = s.replacen(',', ".", 1);
    if t.is_empty() {
        return 0.0;
    }
    t.parse::<f64>().unwrap_or(f64::NAN)
}

/// Cesium's `parseDuration`: `P…` durations, or a time used as an offset.
fn parse_duration_parts(iso8601: Option<&str>) -> Result<ParsedDuration, Iso8601Error> {
    let mut d = Duration::default();
    let Some(s) = iso8601.filter(|s| !s.is_empty()) else {
        return Ok(ParsedDuration { duration: d, nonzero: false });
    };
    if let Some(rest) = s.strip_prefix('P') {
        // /P(?:([\d.,]+)Y)?(?:([\d.,]+)M)?(?:([\d.,]+)W)?(?:([\d.,]+)D)?(?:T(?:([\d.,]+)H)?(?:([\d.,]+)M)?(?:([\d.,]+)S)?)?/
        // Unanchored at the end: parse what matches and ignore the rest.
        let mut r = rest;
        let take = |r: &mut &str, unit: char| -> Option<f64> {
            let n = r.bytes().take_while(|b| b.is_ascii_digit() || *b == b'.' || *b == b',').count();
            if n > 0 && r[n..].starts_with(unit) {
                let v = number(&r[..n]);
                *r = &r[n + unit.len_utf8()..];
                Some(v)
            } else {
                None
            }
        };
        if let Some(v) = take(&mut r, 'Y') {
            d.year = v;
        }
        if let Some(v) = take(&mut r, 'M') {
            d.month = v;
        }
        if let Some(v) = take(&mut r, 'W') {
            d.day = v * 7.0;
        }
        if let Some(v) = take(&mut r, 'D') {
            d.day += v;
        }
        if let Some(mut t) = r.strip_prefix('T') {
            if let Some(v) = take(&mut t, 'H') {
                d.hour = v;
            }
            if let Some(v) = take(&mut t, 'M') {
                d.minute = v;
            }
            if let Some(v) = take(&mut t, 'S') {
                d.second = v.floor();
                d.millisecond = (v % 1.0) * 1000.0;
            }
        }
    } else {
        let s = if s.ends_with('Z') { s.to_string() } else { format!("{s}Z") };
        let g = JulianDate::from_iso8601(&s)?.to_gregorian_date();
        d = Duration {
            year: f64::from(g.year),
            month: f64::from(g.month),
            day: f64::from(g.day),
            hour: f64::from(g.hour),
            minute: f64::from(g.minute),
            second: f64::from(g.second),
            millisecond: g.millisecond,
        };
    }
    let truthy = |v: f64| v != 0.0 && !v.is_nan();
    let nonzero = truthy(d.year)
        || truthy(d.month)
        || truthy(d.day)
        || truthy(d.hour)
        || truthy(d.minute)
        || truthy(d.second)
        || truthy(d.millisecond);
    Ok(ParsedDuration { duration: d, nonzero })
}

fn parse_duration(iso8601: Option<&str>) -> Result<Option<Duration>, Iso8601Error> {
    let p = parse_duration_parts(iso8601)?;
    Ok(p.nonzero.then_some(p.duration))
}

/// Calendar addition with carries, as Cesium's `addToDate`.
fn add_to_date(date: &JulianDate, duration: &Duration) -> JulianDate {
    let g = date.to_gregorian_date();
    let mut millisecond = g.millisecond + duration.millisecond;
    let mut second = f64::from(g.second) + duration.second;
    let mut minute = f64::from(g.minute) + duration.minute;
    let mut hour = f64::from(g.hour) + duration.hour;
    let mut day = f64::from(g.day) + duration.day;
    let mut month = f64::from(g.month) + duration.month;
    let mut year = f64::from(g.year) + duration.year;
    if millisecond >= 1000.0 {
        second += (millisecond / 1000.0).floor();
        millisecond %= 1000.0;
    }
    if second >= 60.0 {
        minute += (second / 60.0).floor();
        second %= 60.0;
    }
    if minute >= 60.0 {
        hour += (minute / 60.0).floor();
        minute %= 60.0;
    }
    if hour >= 24.0 {
        day += (hour / 24.0).floor();
        hour %= 24.0;
    }
    let month_length = |month: f64, year: f64| -> f64 {
        const LENGTHS: [f64; 13] = [0.0, 31.0, 28.0, 31.0, 30.0, 31.0, 30.0, 31.0, 31.0, 30.0, 31.0, 30.0, 31.0];
        if month == 2.0 {
            if is_leap_year(year) {
                29.0
            } else {
                28.0
            }
        } else if (0.0..=12.0).contains(&month) && month.fract() == 0.0 {
            LENGTHS[month as usize]
        } else {
            f64::NAN
        }
    };
    while day > month_length(month, year) || month >= 13.0 {
        if day > month_length(month, year) {
            day -= month_length(month, year);
            month += 1.0;
        }
        if month >= 13.0 {
            month -= 1.0;
            year += (month / 12.0).floor();
            month %= 12.0;
            month += 1.0;
        }
    }
    JulianDate::from_gregorian_fields([year, month, day, hour, minute, second, millisecond])
}
