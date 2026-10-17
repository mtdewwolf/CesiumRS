//! CesiumJS `QuadraticRealPolynomial`: `a x² + b x + c`.

use super::math::{js_max, sign, EPSILON14};

pub fn compute_discriminant(a: f64, b: f64, c: f64) -> f64 {
    b * b - 4.0 * a * c
}

/// `left + right`, or 0 when they nearly cancel (relative difference below `tolerance`).
/// A NaN tolerance never cancels, like JS's `x < undefined`.
pub(crate) fn add_with_cancellation_check(left: f64, right: f64, tolerance: f64) -> f64 {
    let difference = left + right;
    if sign(left) != sign(right) && (difference / js_max(left.abs(), right.abs())).abs() < tolerance {
        return 0.0;
    }
    difference
}

/// Real roots in ascending order (repeated roots are listed twice).
pub fn compute_real_roots(a: f64, b: f64, c: f64) -> Vec<f64> {
    if a == 0.0 {
        if b == 0.0 {
            return vec![];
        }
        return vec![-c / b];
    } else if b == 0.0 {
        if c == 0.0 {
            return vec![0.0, 0.0];
        }
        let c_magnitude = c.abs();
        let a_magnitude = a.abs();
        if c_magnitude < a_magnitude && c_magnitude / a_magnitude < EPSILON14 {
            return vec![0.0, 0.0];
        } else if c_magnitude > a_magnitude && a_magnitude / c_magnitude < EPSILON14 {
            return vec![];
        }
        let ratio = -c / a;
        if ratio < 0.0 {
            return vec![];
        }
        let root = ratio.sqrt();
        return vec![-root, root];
    } else if c == 0.0 {
        let ratio = -b / a;
        if ratio < 0.0 {
            return vec![ratio, 0.0];
        }
        return vec![0.0, ratio];
    }
    let b2 = b * b;
    let four_ac = 4.0 * a * c;
    let radicand = add_with_cancellation_check(b2, -four_ac, EPSILON14);
    if radicand < 0.0 {
        return vec![];
    }
    let q = -0.5 * add_with_cancellation_check(b, sign(b) * radicand.sqrt(), EPSILON14);
    if b > 0.0 {
        vec![q / a, c / q]
    } else {
        vec![c / q, q / a]
    }
}
