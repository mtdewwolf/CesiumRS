//! CesiumJS `CubicRealPolynomial`: `a x³ + b x² + c x + d`.

use super::js_math::JsMath;
use super::quadratic_real_polynomial;

pub fn compute_discriminant(a: f64, b: f64, c: f64, d: f64) -> f64 {
    let a2 = a * a;
    let b2 = b * b;
    let c2 = c * c;
    let d2 = d * d;
    18.0 * a * b * c * d + b2 * c2 - 27.0 * a2 * d2 - 4.0 * (a * c2 * c + b2 * b * d)
}

fn cube_root(x: f64) -> f64 {
    if x < 0.0 {
        -(-x).powf(1.0 / 3.0)
    } else {
        x.powf(1.0 / 3.0)
    }
}

/// Blinn's method for the general case.
fn compute_real_roots_general(a: f64, b: f64, c: f64, d: f64) -> Vec<f64> {
    let big_a = a;
    let big_b = b / 3.0;
    let big_c = c / 3.0;
    let big_d = d;
    let ac = big_a * big_c;
    let bd = big_b * big_d;
    let b2 = big_b * big_b;
    let c2 = big_c * big_c;
    let delta1 = big_a * big_c - b2;
    let delta2 = big_a * big_d - big_b * big_c;
    let delta3 = big_b * big_d - c2;
    let discriminant = 4.0 * delta1 * delta3 - delta2 * delta2;

    if discriminant < 0.0 {
        let (a_bar, c_bar, d_bar) = if b2 * bd >= ac * c2 {
            (big_a, delta1, -2.0 * big_b * delta1 + big_a * delta2)
        } else {
            (big_d, delta3, -big_d * delta2 + 2.0 * big_c * delta3)
        };
        // Not `sign()`: zero counts as positive.
        let s = if d_bar < 0.0 { -1.0 } else { 1.0 };
        let temp0 = -s * a_bar.abs() * (-discriminant).sqrt();
        let temp1 = -d_bar + temp0;
        let x = temp1 / 2.0;
        let p = cube_root(x);
        let q = if temp1 == temp0 { -p } else { -c_bar / p };
        let temp = if c_bar <= 0.0 { p + q } else { -d_bar / (p * p + q * q + c_bar) };
        if b2 * bd >= ac * c2 {
            return vec![(temp - big_b) / big_a];
        }
        return vec![-big_d / (temp + big_c)];
    }

    let c_bar_a = delta1;
    let d_bar_a = -2.0 * big_b * delta1 + big_a * delta2;
    let c_bar_d = delta3;
    let d_bar_d = -big_d * delta2 + 2.0 * big_c * delta3;
    let square_root_of_discriminant = discriminant.sqrt();
    let half_square_root_of_3 = 3.0_f64.sqrt() / 2.0;

    let mut theta = ((big_a * square_root_of_discriminant).js_atan2(-d_bar_a) / 3.0).abs();
    let mut temp = 2.0 * (-c_bar_a).sqrt();
    let mut cosine = theta.js_cos();
    let mut temp1 = temp * cosine;
    let mut temp3 = temp * (-cosine / 2.0 - half_square_root_of_3 * theta.js_sin());
    let numerator_large = if temp1 + temp3 > 2.0 * big_b { temp1 - big_b } else { temp3 - big_b };
    let denominator_large = big_a;
    let root1 = numerator_large / denominator_large;

    theta = ((big_d * square_root_of_discriminant).js_atan2(-d_bar_d) / 3.0).abs();
    temp = 2.0 * (-c_bar_d).sqrt();
    cosine = theta.js_cos();
    temp1 = temp * cosine;
    temp3 = temp * (-cosine / 2.0 - half_square_root_of_3 * theta.js_sin());
    let numerator_small = -big_d;
    let denominator_small = if temp1 + temp3 < 2.0 * big_c { temp1 + big_c } else { temp3 + big_c };
    let root3 = numerator_small / denominator_small;

    let e = denominator_large * denominator_small;
    let f = -numerator_large * denominator_small - denominator_large * numerator_small;
    let g = numerator_large * numerator_small;
    let root2 = (big_c * f - big_b * g) / (-big_b * f + big_c * e);

    if root1 <= root2 {
        if root1 <= root3 {
            if root2 <= root3 {
                return vec![root1, root2, root3];
            }
            return vec![root1, root3, root2];
        }
        return vec![root3, root1, root2];
    }
    if root1 <= root3 {
        return vec![root2, root1, root3];
    }
    if root2 <= root3 {
        return vec![root2, root3, root1];
    }
    vec![root3, root2, root1]
}

/// Real roots in ascending order.
///
/// Matches Cesium including one quirk: for `b = d = 0` with no real roots
/// of `a x² + c`, Cesium returns `[undefined, 0, undefined]` (it tests
/// `roots.Length`); this returns `[NaN, 0, NaN]`.
pub fn compute_real_roots(a: f64, b: f64, c: f64, d: f64) -> Vec<f64> {
    if a == 0.0 {
        return quadratic_real_polynomial::compute_real_roots(b, c, d);
    } else if b == 0.0 {
        if c == 0.0 {
            if d == 0.0 {
                return vec![0.0, 0.0, 0.0];
            }
            let root = cube_root(-d / a);
            return vec![root, root, root];
        } else if d == 0.0 {
            let roots = quadratic_real_polynomial::compute_real_roots(a, 0.0, c);
            let r0 = roots.first().copied().unwrap_or(f64::NAN);
            let r1 = roots.get(1).copied().unwrap_or(f64::NAN);
            return vec![r0, 0.0, r1];
        }
        return compute_real_roots_general(a, 0.0, c, d);
    } else if c == 0.0 {
        if d == 0.0 {
            let ratio = -b / a;
            if ratio < 0.0 {
                return vec![ratio, 0.0, 0.0];
            }
            return vec![0.0, 0.0, ratio];
        }
        return compute_real_roots_general(a, b, 0.0, d);
    } else if d == 0.0 {
        let roots = quadratic_real_polynomial::compute_real_roots(a, b, c);
        if roots.is_empty() {
            return vec![0.0];
        } else if roots[1] <= 0.0 {
            return vec![roots[0], roots[1], 0.0];
        } else if roots[0] >= 0.0 {
            return vec![0.0, roots[0], roots[1]];
        }
        return vec![roots[0], 0.0, roots[1]];
    }
    compute_real_roots_general(a, b, c, d)
}
