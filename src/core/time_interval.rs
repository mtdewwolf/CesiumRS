//! CesiumJS `TimeInterval`: a span of time with optional endpoints and data.

use super::julian_date::{Iso8601Error, JulianDate};

/// Combines two intervals' data where they overlap.
pub type DataMerge<'a, T> = &'a dyn Fn(&Option<T>, &Option<T>) -> Option<T>;

#[derive(Clone, Debug, PartialEq)]
pub struct TimeInterval<T = ()> {
    pub start: JulianDate,
    pub stop: JulianDate,
    pub is_start_included: bool,
    pub is_stop_included: bool,
    pub data: Option<T>,
}

impl<T> Default for TimeInterval<T> {
    fn default() -> Self {
        Self::new(JulianDate::default(), JulianDate::default(), true, true, None)
    }
}

impl<T> TimeInterval<T> {
    pub fn new(start: JulianDate, stop: JulianDate, is_start_included: bool, is_stop_included: bool, data: Option<T>) -> Self {
        Self { start, stop, is_start_included, is_stop_included, data }
    }

    /// An interval containing no time.
    pub fn empty() -> Self {
        Self::new(JulianDate::default(), JulianDate::default(), false, false, None)
    }

    /// `start/stop` in ISO 8601.
    pub fn from_iso8601(iso8601: &str, is_start_included: bool, is_stop_included: bool, data: Option<T>) -> Result<Self, Iso8601Error> {
        let dates: Vec<&str> = iso8601.split('/').collect();
        if dates.len() != 2 {
            return Err(Iso8601Error(iso8601.to_string()));
        }
        Ok(Self::new(
            JulianDate::from_iso8601(dates[0])?,
            JulianDate::from_iso8601(dates[1])?,
            is_start_included,
            is_stop_included,
            data,
        ))
    }

    pub fn to_iso8601(&self, precision: Option<usize>) -> String {
        format!("{}/{}", self.start.to_iso8601(precision), self.stop.to_iso8601(precision))
    }

    pub fn is_empty(&self) -> bool {
        let stop_compared_to_start = JulianDate::compare(&self.stop, &self.start);
        stop_compared_to_start < 0.0
            || (stop_compared_to_start == 0.0 && (!self.is_start_included || !self.is_stop_included))
    }

    pub fn contains(&self, date: &JulianDate) -> bool {
        if self.is_empty() {
            return false;
        }
        let start_compared_to_date = JulianDate::compare(&self.start, date);
        if start_compared_to_date == 0.0 {
            return self.is_start_included;
        }
        let date_compared_to_stop = JulianDate::compare(date, &self.stop);
        if date_compared_to_stop == 0.0 {
            return self.is_stop_included;
        }
        start_compared_to_date < 0.0 && date_compared_to_stop < 0.0
    }

    /// Equal endpoints and inclusion (or both empty) and data that `data_comparer` accepts.
    pub fn equals_with(&self, right: &Self, data_comparer: impl Fn(&Option<T>, &Option<T>) -> bool) -> bool {
        (self.is_empty() && right.is_empty())
            || (self.is_start_included == right.is_start_included
                && self.is_stop_included == right.is_stop_included
                && self.start == right.start
                && self.stop == right.stop
                && data_comparer(&self.data, &right.data))
    }

    pub fn equals_epsilon_with(&self, right: &Self, epsilon: f64, data_comparer: impl Fn(&Option<T>, &Option<T>) -> bool) -> bool {
        (self.is_empty() && right.is_empty())
            || (self.is_start_included == right.is_start_included
                && self.is_stop_included == right.is_stop_included
                && self.start.equals_epsilon(&right.start, epsilon)
                && self.stop.equals_epsilon(&right.stop, epsilon)
                && data_comparer(&self.data, &right.data))
    }
}

impl<T: Clone> TimeInterval<T> {
    /// Overlap of two intervals. Data comes from `merge(left, right)`, or
    /// from `self` without a merge function.
    pub fn intersect(&self, right: &Self, merge: Option<DataMerge<'_, T>>) -> Self {
        let (left_start, left_stop) = (self.start, self.stop);
        let (right_start, right_stop) = (right.start, right.stop);
        let intersects_start_right = JulianDate::greater_than_or_equals(&right_start, &left_start)
            && JulianDate::greater_than_or_equals(&left_stop, &right_start);
        let intersects_start_left = !intersects_start_right
            && JulianDate::less_than_or_equals(&right_start, &left_start)
            && JulianDate::less_than_or_equals(&left_start, &right_stop);
        if !intersects_start_right && !intersects_start_left {
            return Self::empty();
        }
        let left_less_than_right = JulianDate::less_than(&left_stop, &right_stop);
        let start = if intersects_start_right { right_start } else { left_start };
        let is_start_included = (self.is_start_included && right.is_start_included)
            || (right_start != left_start
                && ((intersects_start_right && right.is_start_included) || (intersects_start_left && self.is_start_included)));
        let stop = if left_less_than_right { left_stop } else { right_stop };
        let is_stop_included = if left_less_than_right {
            self.is_stop_included
        } else {
            (self.is_stop_included && right.is_stop_included) || (right_stop != left_stop && right.is_stop_included)
        };
        let data = match merge {
            Some(m) => m(&self.data, &right.data),
            None => self.data.clone(),
        };
        Self::new(start, stop, is_start_included, is_stop_included, data)
    }
}

impl<T: PartialEq> TimeInterval<T> {
    /// Like Cesium's `equals` with `===` on the data.
    pub fn equals(&self, right: &Self) -> bool {
        self.equals_with(right, |a, b| a == b)
    }

    pub fn equals_epsilon(&self, right: &Self, epsilon: f64) -> bool {
        self.equals_epsilon_with(right, epsilon, |a, b| a == b)
    }
}
