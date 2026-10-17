//! CesiumJS `TimeStandard`.

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TimeStandard {
    /// Coordinated Universal Time, which has leap seconds.
    Utc = 0,
    /// International Atomic Time, a continuous count.
    Tai = 1,
}
