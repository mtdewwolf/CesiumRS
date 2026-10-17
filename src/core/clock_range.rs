//! CesiumJS `ClockRange`: what `Clock::tick` does at the start and stop times.

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ClockRange {
    Unbounded = 0,
    /// Stop at the start or stop time.
    Clamped = 1,
    /// Loop back to the start after the stop time.
    LoopStop = 2,
}
