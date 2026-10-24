//! CesiumJS `Spdcf`: the strictly positive-definite correlation function
//! parameters of an `NGA_gpm_local` correlation group.
//!
//! The constructor's range checks (`0 < A <= 1`, `0 <= alpha < 1`,
//! `0 <= beta <= 10`, `T > 0`) are debug-build `DeveloperError`s and are not
//! ported. A parameter missing from the JSON is NaN.

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Spdcf {
    /// `A`, in (0, 1].
    pub a: f64,
    pub alpha: f64,
    pub beta: f64,
    /// `T`, the correlation time.
    pub t: f64,
}
