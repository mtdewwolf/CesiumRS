//! JulianDate, TimeInterval, TimeIntervalCollection, and Clock against
//! CesiumJS fixtures.

mod common;

use cesium_rs::core::clock::ClockOptions;
use cesium_rs::core::time_interval_collection::DateArrayOptions;
use cesium_rs::core::*;
use common::*;
use serde_json::{json, Value};

/// Fixture dates are `[dayNumber, secondsOfDay]` in TAI.
fn jd(v: &Value) -> JulianDate {
    JulianDate::new(n(&v[0]), n(&v[1]), TimeStandard::Tai)
}

impl Enc for JulianDate {
    fn enc(&self) -> Value {
        json!([num(self.day_number), num(self.seconds_of_day)])
    }
}

impl Enc for GregorianDate {
    fn enc(&self) -> Value {
        json!([
            self.year,
            self.month,
            self.day,
            self.hour,
            self.minute,
            self.second,
            num(self.millisecond),
            self.is_leap_second
        ])
    }
}

impl Enc for TimeInterval<f64> {
    fn enc(&self) -> Value {
        json!([self.start.enc(), self.stop.enc(), self.is_start_included, self.is_stop_included, self.data.enc()])
    }
}

impl Enc for TimeIntervalCollection<f64> {
    fn enc(&self) -> Value {
        self.intervals().to_vec().enc()
    }
}

fn b(v: &Value) -> bool {
    v.as_bool().unwrap()
}

fn interval(v: &Value) -> TimeInterval<f64> {
    TimeInterval::new(jd(&v[0]), jd(&v[1]), b(&v[2]), b(&v[3]), opt(&v[4]))
}

/// Cesium returns "invalid" in place of the DeveloperError a bad ISO 8601 string throws.
fn or_invalid<T: Enc, E>(r: Result<T, E>) -> Option<Out> {
    match r {
        Ok(v) => ok(v),
        Err(_) => ok("invalid".to_string()),
    }
}

/// The fixtures' merge callback: `(left ?? 0) * 10 + (right ?? 0)`.
fn merge(a: &Option<f64>, b: &Option<f64>) -> Option<f64> {
    Some(a.unwrap_or(0.0) * 10.0 + b.unwrap_or(0.0))
}

fn date_options(v: &Value) -> DateArrayOptions {
    DateArrayOptions {
        is_start_included: b(&v[0]),
        is_stop_included: b(&v[1]),
        leading_interval: b(&v[2]),
        trailing_interval: b(&v[3]),
    }
}

fn index_data(_: &TimeInterval<f64>, i: usize) -> Option<f64> {
    Some(i as f64)
}

fn same(a: &Option<f64>, b: &Option<f64>) -> bool {
    a == b
}

#[test]
fn julian_date() {
    run(
        "core",
        "JulianDate",
        |_| TIGHT,
        |f, a| {
            let d = |i: usize| jd(&a[i]);
            match f {
                "new" => {
                    let standard = if n(&a[2]) == 0.0 { TimeStandard::Utc } else { TimeStandard::Tai };
                    ok(JulianDate::new(n(&a[0]), n(&a[1]), standard))
                }
                "fromGregorianDate" => {
                    let i = |k: usize| n(&a[k]) as i32;
                    let g = GregorianDate {
                        year: i(0),
                        month: i(1),
                        day: i(2),
                        hour: i(3),
                        minute: i(4),
                        second: i(5),
                        millisecond: n(&a[6]),
                        is_leap_second: b(&a[7]),
                    };
                    ok(JulianDate::from_gregorian_date(&g))
                }
                "fromDate" => ok(JulianDate::from_date(n(&a[0]))),
                "fromIso8601" => or_invalid(JulianDate::from_iso8601(s(&a[0]))),
                "toGregorianDate" => ok(d(0).to_gregorian_date()),
                "toDate" => ok(d(0).to_date()),
                "toIso8601" => ok(d(0).to_iso8601(opt(&a[1]).map(|p| p as usize))),
                "roundTripIso8601" => res(JulianDate::from_iso8601(&d(0).to_iso8601(None))),
                "compare" => ok(JulianDate::compare(&d(0), &d(1))),
                "equals" => ok(d(0).equals(&d(1))),
                "equalsEpsilon" => ok(d(0).equals_epsilon(&d(1), n(&a[2]))),
                "lessThan" => ok(JulianDate::less_than(&d(0), &d(1))),
                "lessThanOrEquals" => ok(JulianDate::less_than_or_equals(&d(0), &d(1))),
                "greaterThan" => ok(JulianDate::greater_than(&d(0), &d(1))),
                "greaterThanOrEquals" => ok(JulianDate::greater_than_or_equals(&d(0), &d(1))),
                "totalDays" => ok(d(0).total_days()),
                "secondsDifference" => ok(JulianDate::seconds_difference(&d(0), &d(1))),
                "daysDifference" => ok(JulianDate::days_difference(&d(0), &d(1))),
                "computeTaiMinusUtc" => ok(d(0).compute_tai_minus_utc()),
                "addSeconds" => ok(d(0).add_seconds(n(&a[1]))),
                "addMinutes" => ok(d(0).add_minutes(n(&a[1]))),
                "addHours" => ok(d(0).add_hours(n(&a[1]))),
                "addDays" => ok(d(0).add_days(n(&a[1]))),
                _ => None,
            }
        },
    );
}

#[test]
fn time_interval() {
    run(
        "core",
        "TimeInterval",
        |_| TIGHT,
        |f, a| {
            let i = |k: usize| interval(&a[k]);
            match f {
                "fromIso8601" => or_invalid(TimeInterval::<f64>::from_iso8601(s(&a[0]), b(&a[1]), b(&a[2]), None)),
                "toIso8601" => ok(i(0).to_iso8601(opt(&a[1]).map(|p| p as usize))),
                "isEmpty" => ok(i(0).is_empty()),
                "contains" => ok(i(0).contains(&jd(&a[1]))),
                "equals" => ok(i(0).equals(&i(1))),
                "equalsEpsilon" => ok(i(0).equals_epsilon(&i(1), n(&a[2]))),
                "intersect" => {
                    let m: Option<time_interval::DataMerge<'_, f64>> = if b(&a[2]) { Some(&merge) } else { None };
                    ok(i(0).intersect(&i(1), m))
                }
                _ => None,
            }
        },
    );
}

/// Applies `["add" | "remove", interval]` operations; returns the
/// collection, each removal's result, and how often `changed_event` fired.
fn build(ops: &Value) -> (TimeIntervalCollection<f64>, Vec<bool>, u32) {
    let mut c = TimeIntervalCollection::new();
    let changed = std::sync::Arc::new(std::sync::atomic::AtomicU32::new(0));
    let counter = changed.clone();
    c.changed_event.add_event_listener(move |_| {
        counter.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
    });
    let mut removed = Vec::new();
    for op in ops.as_array().unwrap() {
        let i = interval(&op[1]);
        if s(&op[0]) == "add" {
            c.add_interval(i);
        } else {
            removed.push(c.remove_interval(&i));
        }
    }
    (c, removed, changed.load(std::sync::atomic::Ordering::SeqCst))
}

#[test]
fn time_interval_collection() {
    let mut grid = Vec::new();
    for d in 0..2 {
        for k in 0..=8 {
            grid.push(JulianDate::new(f64::from(2451545 + d), f64::from(k * 10800), TimeStandard::Tai));
        }
    }
    run(
        "core",
        "TimeIntervalCollection",
        |_| TIGHT,
        |f, a| match f {
            "ops" => {
                let (c, removed, changed) = build(&a[0]);
                let index: Vec<f64> = grid.iter().map(|d| c.index_of(d) as f64).collect();
                let data: Vec<Option<f64>> =
                    grid.iter().map(|d| c.find_data_for_interval_containing_date(d).copied()).collect();
                let bounds = (!c.is_empty()).then(|| {
                    json!([
                        c.start().unwrap().enc(),
                        c.is_start_included(),
                        c.stop().unwrap().enc(),
                        c.is_stop_included()
                    ])
                });
                ok(json!([c.enc(), removed.enc(), changed, index.enc(), data.enc(), bounds.enc()]))
            }
            "intersect" => {
                let (left, ..) = build(&a[0]);
                let (right, ..) = build(&a[1]);
                let m: Option<time_interval::DataMerge<'_, f64>> = if b(&a[2]) { Some(&merge) } else { None };
                ok(left.intersect_with(&right, same, m))
            }
            "fromJulianDateArray" => {
                let dates = list(&a[0], jd);
                ok(TimeIntervalCollection::from_julian_date_array(&dates, date_options(&a[1]), index_data, same))
            }
            "fromIso8601" => {
                or_invalid(TimeIntervalCollection::from_iso8601(s(&a[0]), date_options(&a[1]), index_data, same))
            }
            "fromIso8601DateArray" => {
                let dates = list(&a[0], |v| s(v).to_string());
                let dates: Vec<&str> = dates.iter().map(String::as_str).collect();
                or_invalid(TimeIntervalCollection::from_iso8601_date_array(
                    &dates,
                    date_options(&a[1]),
                    index_data,
                    same,
                ))
            }
            "fromIso8601DurationArray" => {
                let durations = list(&a[1], |v| s(v).to_string());
                let durations: Vec<&str> = durations.iter().map(String::as_str).collect();
                or_invalid(TimeIntervalCollection::from_iso8601_duration_array(
                    &jd(&a[0]),
                    &durations,
                    b(&a[2]),
                    date_options(&a[3]),
                    index_data,
                    same,
                ))
            }
            _ => None,
        },
    );
}

fn clock_step(v: &Value) -> ClockStep {
    match n(v) as i32 {
        0 => ClockStep::TickDependent,
        1 => ClockStep::SystemClockMultiplier,
        _ => ClockStep::SystemClock,
    }
}

fn clock_range(v: &Value) -> ClockRange {
    match n(v) as i32 {
        0 => ClockRange::Unbounded,
        1 => ClockRange::Clamped,
        _ => ClockRange::LoopStop,
    }
}

/// `[start, stop, current, multiplier, clockStep, clockRange, canAnimate, shouldAnimate]`
fn clock(v: &Value, system_time: f64) -> Clock {
    let options = ClockOptions {
        start_time: Some(jd(&v[0])),
        stop_time: Some(jd(&v[1])),
        current_time: Some(jd(&v[2])),
        multiplier: Some(n(&v[3])),
        clock_step: Some(clock_step(&v[4])),
        clock_range: Some(clock_range(&v[5])),
        can_animate: Some(b(&v[6])),
        should_animate: Some(b(&v[7])),
    };
    Clock::new_at(options, system_time).unwrap()
}

fn clock_state(c: &Clock) -> Value {
    json!([c.current_time().enc(), num(c.multiplier()), c.clock_step() as i32, c.should_animate()])
}

#[test]
fn clock_tick() {
    run(
        "core",
        "Clock",
        |_| TIGHT,
        |f, a| match f {
            "tick" => {
                let mut now = 1000.0;
                let mut c = clock(&a[0], now);
                use std::sync::atomic::{AtomicU32, Ordering};
                use std::sync::Arc;
                let stops = Arc::new(AtomicU32::new(0));
                let ticks = Arc::new(AtomicU32::new(0));
                let (st, tk) = (stops.clone(), ticks.clone());
                c.on_stop.add_event_listener(move |_| {
                    st.fetch_add(1, Ordering::SeqCst);
                });
                c.on_tick.add_event_listener(move |_| {
                    tk.fetch_add(1, Ordering::SeqCst);
                });
                let out: Vec<Value> = a[1]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|dt| {
                        now += n(dt);
                        c.tick_at(now).enc()
                    })
                    .collect();
                ok(json!([out, stops.load(Ordering::SeqCst), ticks.load(Ordering::SeqCst), clock_state(&c)]))
            }
            "setters" => {
                let mut c = clock(&a[0], 0.0);
                let mut from_system = false;
                let states: Vec<Value> = a[1]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|op| {
                        let v = &op[1];
                        match s(&op[0]) {
                            "clockStep" => {
                                let step = clock_step(v);
                                c.set_clock_step(step);
                                from_system |= step == ClockStep::SystemClock;
                            }
                            "multiplier" => c.set_multiplier(n(v)),
                            "shouldAnimate" => c.set_should_animate(b(v)),
                            _ => {
                                c.set_current_time(jd(v));
                                from_system = false;
                            }
                        }
                        let mut state = clock_state(&c);
                        if from_system {
                            state[0] = json!("now");
                        }
                        state
                    })
                    .collect();
                ok(states)
            }
            _ => None,
        },
    );
}

#[test]
fn simon1994_planetary_positions() {
    use cesium_rs::core::simon1994_planetary_positions as simon;
    run(
        "core",
        "Simon1994PlanetaryPositions",
        |_| TIGHT_VECTOR,
        |f, a| {
            let d = JulianDate::new(n(&a[0]), n(&a[1]), TimeStandard::Tai);
            match f {
                "sun" => ok(simon::compute_sun_position_in_earth_inertial_frame(&d)),
                "moon" => ok(simon::compute_moon_position_in_earth_inertial_frame(&d)),
                _ => None,
            }
        },
    );
}
