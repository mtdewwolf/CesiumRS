//! CesiumJS `Interval`: a closed range `[start, stop]`.

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Interval {
    pub start: f64,
    pub stop: f64,
}

impl Interval {
    pub const fn new(start: f64, stop: f64) -> Self {
        Self { start, stop }
    }
}
