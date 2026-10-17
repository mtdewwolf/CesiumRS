//! CesiumJS `Clock`: simulation time that advances on each tick.

use super::clock_range::ClockRange;
use super::clock_step::ClockStep;
use super::event::Event;
use super::julian_date::JulianDate;

/// Milliseconds from a monotonic clock, like Cesium's `getTimestamp`.
pub fn get_timestamp() -> f64 {
    thread_local! {
        static START: std::time::Instant = std::time::Instant::now();
    }
    START.with(|s| s.elapsed().as_secs_f64() * 1000.0)
}

/// Options for [`Clock::new`]; Cesium's defaults when `None`.
#[derive(Clone, Copy, Debug, Default)]
pub struct ClockOptions {
    pub start_time: Option<JulianDate>,
    pub stop_time: Option<JulianDate>,
    pub current_time: Option<JulianDate>,
    pub multiplier: Option<f64>,
    pub clock_step: Option<ClockStep>,
    pub clock_range: Option<ClockRange>,
    pub can_animate: Option<bool>,
    pub should_animate: Option<bool>,
}

pub struct Clock {
    pub start_time: JulianDate,
    pub stop_time: JulianDate,
    pub clock_range: ClockRange,
    pub can_animate: bool,
    /// Raised after every tick with the new current time.
    pub on_tick: Event<JulianDate>,
    /// Raised when the clock reaches its stop time.
    pub on_stop: Event<JulianDate>,
    current_time: JulianDate,
    multiplier: f64,
    clock_step: ClockStep,
    should_animate: bool,
    last_system_time: f64,
}

impl Clock {
    /// `None` if the start time is after the stop time (Cesium checks this
    /// only in debug builds; `LoopStop` would never terminate).
    pub fn new(options: ClockOptions) -> Option<Self> {
        Self::new_at(options, get_timestamp())
    }

    /// [`Self::new`] with the monotonic time (ms) that [`Self::tick_at`] measures from.
    pub fn new_at(options: ClockOptions, system_time: f64) -> Option<Self> {
        let current_time = match (options.current_time, options.start_time, options.stop_time) {
            (Some(c), _, _) => c,
            (None, Some(s), _) => s,
            (None, None, Some(stop)) => stop.add_days(-1.0),
            (None, None, None) => JulianDate::now(),
        };
        let start_time = options.start_time.unwrap_or(current_time);
        let stop_time = options.stop_time.unwrap_or_else(|| start_time.add_days(1.0));
        if JulianDate::greater_than(&start_time, &stop_time) {
            return None;
        }
        let mut clock = Self {
            start_time,
            stop_time,
            clock_range: options.clock_range.unwrap_or(ClockRange::Unbounded),
            can_animate: options.can_animate.unwrap_or(true),
            on_tick: Event::new(),
            on_stop: Event::new(),
            current_time,
            multiplier: options.multiplier.unwrap_or(1.0),
            clock_step: ClockStep::SystemClockMultiplier,
            should_animate: options.should_animate.unwrap_or(false),
            last_system_time: system_time,
        };
        clock.set_clock_step(options.clock_step.unwrap_or(ClockStep::SystemClockMultiplier));
        Some(clock)
    }

    pub fn current_time(&self) -> JulianDate {
        self.current_time
    }

    /// Changing the time leaves `SystemClock` mode.
    pub fn set_current_time(&mut self, value: JulianDate) {
        if self.current_time == value {
            return;
        }
        if self.clock_step == ClockStep::SystemClock {
            self.clock_step = ClockStep::SystemClockMultiplier;
        }
        self.current_time = value;
    }

    pub fn multiplier(&self) -> f64 {
        self.multiplier
    }

    pub fn set_multiplier(&mut self, value: f64) {
        if self.multiplier == value {
            return;
        }
        if self.clock_step == ClockStep::SystemClock {
            self.clock_step = ClockStep::SystemClockMultiplier;
        }
        self.multiplier = value;
    }

    pub fn clock_step(&self) -> ClockStep {
        self.clock_step
    }

    /// `SystemClock` also sets the multiplier to 1, starts animating, and
    /// jumps to the current time.
    pub fn set_clock_step(&mut self, value: ClockStep) {
        if value == ClockStep::SystemClock {
            self.multiplier = 1.0;
            self.should_animate = true;
            self.current_time = JulianDate::now();
        }
        self.clock_step = value;
    }

    pub fn should_animate(&self) -> bool {
        self.should_animate
    }

    pub fn set_should_animate(&mut self, value: bool) {
        if self.should_animate == value {
            return;
        }
        if self.clock_step == ClockStep::SystemClock {
            self.clock_step = ClockStep::SystemClockMultiplier;
        }
        self.should_animate = value;
    }

    /// Advances the clock using the wall clock.
    pub fn tick(&mut self) -> JulianDate {
        self.tick_at(get_timestamp())
    }

    /// Advances the clock as if the monotonic time were `current_system_time` (ms).
    pub fn tick_at(&mut self, current_system_time: f64) -> JulianDate {
        let mut current_time = self.current_time;
        if self.can_animate && self.should_animate {
            if self.clock_step == ClockStep::SystemClock {
                current_time = JulianDate::now();
            } else {
                let multiplier = self.multiplier;
                current_time = if self.clock_step == ClockStep::TickDependent {
                    current_time.add_seconds(multiplier)
                } else {
                    let milliseconds = current_system_time - self.last_system_time;
                    current_time.add_seconds(multiplier * (milliseconds / 1000.0))
                };
                match self.clock_range {
                    ClockRange::Clamped => {
                        if JulianDate::less_than(&current_time, &self.start_time) {
                            current_time = self.start_time;
                        } else if JulianDate::greater_than(&current_time, &self.stop_time) {
                            current_time = self.stop_time;
                            self.on_stop.raise_event(&current_time);
                        }
                    }
                    ClockRange::LoopStop => {
                        if JulianDate::less_than(&current_time, &self.start_time) {
                            current_time = self.start_time;
                        }
                        while JulianDate::greater_than(&current_time, &self.stop_time) {
                            current_time = self
                                .start_time
                                .add_seconds(JulianDate::seconds_difference(&current_time, &self.stop_time));
                            self.on_stop.raise_event(&current_time);
                        }
                    }
                    ClockRange::Unbounded => {}
                }
            }
        }
        self.current_time = current_time;
        self.last_system_time = current_system_time;
        self.on_tick.raise_event(&current_time);
        current_time
    }
}
