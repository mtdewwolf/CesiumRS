//! CesiumJS `IauOrientationParameters`: a body's pole and rotation, in radians.

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct IauOrientationParameters {
    /// Right ascension of the north pole in ICRF.
    pub right_ascension: f64,
    /// Declination of the north pole in ICRF.
    pub declination: f64,
    /// Rotation about the north pole for the prime meridian.
    pub rotation: f64,
    /// Rotation rate in radians per second.
    pub rotation_rate: f64,
}
