//! CesiumJS `QuarticRealPolynomial`: `a x⁴ + b x³ + c x² + d x + e`.

use super::cubic_real_polynomial;
use super::math::{sign, EPSILON14, EPSILON15};
use super::quadratic_real_polynomial;

#[allow(clippy::too_many_arguments)]
pub fn compute_discriminant(a: f64, b: f64, c: f64, d: f64, e: f64) -> f64 {
    let a2 = a * a;
    let a3 = a2 * a;
    let b2 = b * b;
    let b3 = b2 * b;
    let c2 = c * c;
    let c3 = c2 * c;
    let d2 = d * d;
    let d3 = d2 * d;
    let e2 = e * e;
    let e3 = e2 * e;
    b2 * c2 * d2 - 4.0 * b3 * d3 - 4.0 * a * c3 * d2 + 18.0 * a * b * c * d3 - 27.0 * a2 * d2 * d2
        + 256.0 * a3 * e3
        + e * (18.0 * b3 * c * d - 4.0 * b2 * c3 + 16.0 * a * c2 * c2 - 80.0 * a * b * c2 * d - 6.0 * a * b2 * d2
            + 144.0 * a2 * c * d2)
        + e2 * (144.0 * a * b2 * c - 27.0 * b2 * b2 - 128.0 * a2 * c2 - 192.0 * a2 * b * d)
}

/// Orders the roots of two monic quadratics, as Cesium does.
fn merge(roots1: &[f64], roots2: &[f64]) -> Vec<f64> {
    if roots1[1] <= roots2[0] {
        vec![roots1[0], roots1[1], roots2[0], roots2[1]]
    } else if roots2[1] <= roots1[0] {
        vec![roots2[0], roots2[1], roots1[0], roots1[1]]
    } else if roots1[0] >= roots2[0] && roots1[1] <= roots2[1] {
        vec![roots2[0], roots1[0], roots1[1], roots2[1]]
    } else if roots2[0] >= roots1[0] && roots2[1] <= roots1[1] {
        vec![roots1[0], roots2[0], roots2[1], roots1[1]]
    } else if roots1[0] > roots2[0] && roots1[0] < roots2[1] {
        vec![roots2[0], roots1[0], roots2[1], roots1[1]]
    } else {
        vec![roots1[0], roots2[0], roots1[1], roots2[1]]
    }
}

/// Ferrari's method on the depressed quartic.
fn original(a3: f64, a2: f64, a1: f64, a0: f64) -> Vec<f64> {
    let a3_squared = a3 * a3;
    let p = a2 - (3.0 * a3_squared) / 8.0;
    let q = a1 - (a2 * a3) / 2.0 + (a3_squared * a3) / 8.0;
    let r = a0 - (a1 * a3) / 4.0 + (a2 * a3_squared) / 16.0 - (3.0 * a3_squared * a3_squared) / 256.0;
    let cubic_roots = cubic_real_polynomial::compute_real_roots(1.0, 2.0 * p, p * p - 4.0 * r, -q * q);
    if cubic_roots.is_empty() {
        return vec![];
    }
    let temp = -a3 / 4.0;
    let h_squared = cubic_roots[cubic_roots.len() - 1];
    if h_squared.abs() < EPSILON14 {
        let roots = quadratic_real_polynomial::compute_real_roots(1.0, p, r);
        if roots.len() == 2 {
            let (root0, root1) = (roots[0], roots[1]);
            if root0 >= 0.0 && root1 >= 0.0 {
                let y0 = root0.sqrt();
                let y1 = root1.sqrt();
                return vec![temp - y1, temp - y0, temp + y0, temp + y1];
            } else if root0 >= 0.0 && root1 < 0.0 {
                let y = root0.sqrt();
                return vec![temp - y, temp + y];
            } else if root0 < 0.0 && root1 >= 0.0 {
                let y = root1.sqrt();
                return vec![temp - y, temp + y];
            }
        }
        return vec![];
    } else if h_squared > 0.0 {
        let h = h_squared.sqrt();
        let m = (p + h_squared - q / h) / 2.0;
        let n = (p + h_squared + q / h) / 2.0;
        let mut roots1 = quadratic_real_polynomial::compute_real_roots(1.0, h, m);
        let mut roots2 = quadratic_real_polynomial::compute_real_roots(1.0, -h, n);
        if !roots1.is_empty() {
            roots1[0] += temp;
            roots1[1] += temp;
            if !roots2.is_empty() {
                roots2[0] += temp;
                roots2[1] += temp;
                return merge(&roots1, &roots2);
            }
            return roots1;
        }
        if !roots2.is_empty() {
            roots2[0] += temp;
            roots2[1] += temp;
            return roots2;
        }
        return vec![];
    }
    vec![]
}

/// Neumark's method, used when the coefficient signs make Ferrari's unstable.
fn neumark(a3: f64, a2: f64, a1: f64, a0: f64) -> Vec<f64> {
    let a1_squared = a1 * a1;
    let a2_squared = a2 * a2;
    let a3_squared = a3 * a3;
    let p = -2.0 * a2;
    let q = a1 * a3 + a2_squared - 4.0 * a0;
    let r = a3_squared * a0 - a1 * a2 * a3 + a1_squared;
    let cubic_roots = cubic_real_polynomial::compute_real_roots(1.0, p, q, r);
    if cubic_roots.is_empty() {
        return vec![];
    }
    let y = cubic_roots[0];
    let temp = a2 - y;
    let temp_squared = temp * temp;
    let g1 = a3 / 2.0;
    let h1 = temp / 2.0;
    let m = temp_squared - 4.0 * a0;
    let m_error = temp_squared + 4.0 * a0.abs();
    let n = a3_squared - 4.0 * y;
    let n_error = a3_squared + 4.0 * y.abs();
    let (g2, h2) = if y < 0.0 || m * n_error < n * m_error {
        let square_root_of_n = n.sqrt();
        (square_root_of_n / 2.0, if square_root_of_n == 0.0 { 0.0 } else { (a3 * h1 - a1) / square_root_of_n })
    } else {
        let square_root_of_m = m.sqrt();
        (if square_root_of_m == 0.0 { 0.0 } else { (a3 * h1 - a1) / square_root_of_m }, square_root_of_m / 2.0)
    };
    let (big_g, g) = if g1 == 0.0 && g2 == 0.0 {
        (0.0, 0.0)
    } else if sign(g1) == sign(g2) {
        let big = g1 + g2;
        (big, y / big)
    } else {
        let small = g1 - g2;
        (y / small, small)
    };
    let (big_h, h) = if h1 == 0.0 && h2 == 0.0 {
        (0.0, 0.0)
    } else if sign(h1) == sign(h2) {
        let big = h1 + h2;
        (big, a0 / big)
    } else {
        let small = h1 - h2;
        (a0 / small, small)
    };
    let roots1 = quadratic_real_polynomial::compute_real_roots(1.0, big_g, big_h);
    let roots2 = quadratic_real_polynomial::compute_real_roots(1.0, g, h);
    if !roots1.is_empty() {
        if !roots2.is_empty() {
            return merge(&roots1, &roots2);
        }
        return roots1;
    }
    roots2
}

/// Real roots in ascending order.
pub fn compute_real_roots(a: f64, b: f64, c: f64, d: f64, e: f64) -> Vec<f64> {
    if a.abs() < EPSILON15 {
        return cubic_real_polynomial::compute_real_roots(b, c, d, e);
    }
    let a3 = b / a;
    let a2 = c / a;
    let a1 = d / a;
    let a0 = e / a;
    // Cesium's selector: each negative coefficient adds `k + 1`, otherwise `k`.
    let mut k: u32 = if a3 < 0.0 { 1 } else { 0 };
    k += if a2 < 0.0 { k + 1 } else { k };
    k += if a1 < 0.0 { k + 1 } else { k };
    k += if a0 < 0.0 { k + 1 } else { k };
    match k {
        1 | 2 | 5 | 8 | 11 => neumark(a3, a2, a1, a0),
        _ => original(a3, a2, a1, a0),
    }
}
