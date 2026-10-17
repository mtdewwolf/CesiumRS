//! CesiumJS `ClockStep`: how `Clock::tick` advances time.

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ClockStep {
    /// A fixed number of seconds per tick (`multiplier`).
    TickDependent = 0,
    /// Wall-clock time since the last tick, times `multiplier`.
    SystemClockMultiplier = 1,
    /// The current wall-clock time.
    SystemClock = 2,
}
