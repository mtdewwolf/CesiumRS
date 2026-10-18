//! CesiumJS `TridiagonalSystemSolver`: the Thomas algorithm for a tridiagonal
//! system with Cartesian3 right-hand sides.

use super::cartesian3::Cartesian3;

/// Solves the system with sub-diagonal `lower`, `diagonal`, super-diagonal
/// `upper` and right-hand side `right`. `None` if the lengths do not fit
/// (`diagonal` and `right` of length n, `lower` and `upper` of n − 1, n ≥ 2).
pub fn solve(lower: &[f64], diagonal: &[f64], upper: &[f64], right: &[Cartesian3]) -> Option<Vec<Cartesian3>> {
    let n = diagonal.len();
    if n < 2 || right.len() != n || lower.len() != n - 1 || upper.len() != n - 1 {
        return None;
    }
    let mut c = vec![0.0; n - 1];
    let mut d = vec![Cartesian3::ZERO; n];
    c[0] = upper[0] / diagonal[0];
    d[0] = right[0].multiply_by_scalar(1.0 / diagonal[0]);
    let mut i = 1;
    while i < c.len() {
        let scalar = 1.0 / (diagonal[i] - c[i - 1] * lower[i - 1]);
        c[i] = upper[i] * scalar;
        d[i] = right[i].subtract(d[i - 1].multiply_by_scalar(lower[i - 1])).multiply_by_scalar(scalar);
        i += 1;
    }
    let scalar = 1.0 / (diagonal[i] - c[i - 1] * lower[i - 1]);
    d[i] = right[i].subtract(d[i - 1].multiply_by_scalar(lower[i - 1])).multiply_by_scalar(scalar);

    let mut x = vec![Cartesian3::ZERO; n];
    x[n - 1] = d[n - 1];
    for i in (0..n - 1).rev() {
        x[i] = d[i].subtract(x[i + 1].multiply_by_scalar(c[i]));
    }
    Some(x)
}
