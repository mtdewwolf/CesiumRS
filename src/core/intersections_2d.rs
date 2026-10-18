//! CesiumJS `Intersections2D`: triangle clipping, barycentric coordinates,
//! and segment intersection in 2D.

use super::cartesian2::Cartesian2;
use super::cartesian3::Cartesian3;

/// Clips a triangle, given one coordinate per vertex, against an
/// axis-aligned threshold. The result lists the kept polygon's vertices in
/// Cesium's encoding: an index 0–2 for an original vertex, or `-1, a, b, t`
/// for a new vertex between `a` and `b` at ratio `t`.
pub fn clip_triangle_at_axis_aligned_threshold(
    threshold: f64,
    keep_above: bool,
    u0: f64,
    u1: f64,
    u2: f64,
) -> Vec<f64> {
    let mut result = Vec::new();
    let (u0_behind, u1_behind, u2_behind) = if keep_above {
        (u0 < threshold, u1 < threshold, u2 < threshold)
    } else {
        (u0 > threshold, u1 > threshold, u2 > threshold)
    };
    let num_behind = u0_behind as u8 + u1_behind as u8 + u2_behind as u8;

    if num_behind == 1 {
        if u0_behind {
            let u01_ratio = (threshold - u0) / (u1 - u0);
            let u02_ratio = (threshold - u0) / (u2 - u0);
            result.extend([1.0, 2.0]);
            if u02_ratio != 1.0 {
                result.extend([-1.0, 0.0, 2.0, u02_ratio]);
            }
            if u01_ratio != 1.0 {
                result.extend([-1.0, 0.0, 1.0, u01_ratio]);
            }
        } else if u1_behind {
            let u12_ratio = (threshold - u1) / (u2 - u1);
            let u10_ratio = (threshold - u1) / (u0 - u1);
            result.extend([2.0, 0.0]);
            if u10_ratio != 1.0 {
                result.extend([-1.0, 1.0, 0.0, u10_ratio]);
            }
            if u12_ratio != 1.0 {
                result.extend([-1.0, 1.0, 2.0, u12_ratio]);
            }
        } else if u2_behind {
            let u20_ratio = (threshold - u2) / (u0 - u2);
            let u21_ratio = (threshold - u2) / (u1 - u2);
            result.extend([0.0, 1.0]);
            if u21_ratio != 1.0 {
                result.extend([-1.0, 2.0, 1.0, u21_ratio]);
            }
            if u20_ratio != 1.0 {
                result.extend([-1.0, 2.0, 0.0, u20_ratio]);
            }
        }
    } else if num_behind == 2 {
        if !u0_behind && u0 != threshold {
            let u10_ratio = (threshold - u1) / (u0 - u1);
            let u20_ratio = (threshold - u2) / (u0 - u2);
            result.extend([0.0, -1.0, 1.0, 0.0, u10_ratio, -1.0, 2.0, 0.0, u20_ratio]);
        } else if !u1_behind && u1 != threshold {
            let u21_ratio = (threshold - u2) / (u1 - u2);
            let u01_ratio = (threshold - u0) / (u1 - u0);
            result.extend([1.0, -1.0, 2.0, 1.0, u21_ratio, -1.0, 0.0, 1.0, u01_ratio]);
        } else if !u2_behind && u2 != threshold {
            let u02_ratio = (threshold - u0) / (u2 - u0);
            let u12_ratio = (threshold - u1) / (u2 - u1);
            result.extend([2.0, -1.0, 0.0, 2.0, u02_ratio, -1.0, 1.0, 2.0, u12_ratio]);
        }
    } else if num_behind != 3 {
        result.extend([0.0, 1.0, 2.0]);
    }
    result
}

/// Barycentric coordinates of `(x, y)` in the triangle.
#[allow(clippy::too_many_arguments)]
pub fn compute_barycentric_coordinates(
    x: f64,
    y: f64,
    x1: f64,
    y1: f64,
    x2: f64,
    y2: f64,
    x3: f64,
    y3: f64,
) -> Cartesian3 {
    let x1mx3 = x1 - x3;
    let x3mx2 = x3 - x2;
    let y2my3 = y2 - y3;
    let y1my3 = y1 - y3;
    let inverse_determinant = 1.0 / (y2my3 * x1mx3 + x3mx2 * y1my3);
    let ymy3 = y - y3;
    let xmx3 = x - x3;
    let l1 = (y2my3 * xmx3 + x3mx2 * ymy3) * inverse_determinant;
    let l2 = (-y1my3 * xmx3 + x1mx3 * ymy3) * inverse_determinant;
    let l3 = 1.0 - l1 - l2;
    Cartesian3::new(l1, l2, l3)
}

/// Where segments (x00, y00)–(x01, y01) and (x10, y10)–(x11, y11) cross;
/// `None` if they are parallel or do not meet.
#[allow(clippy::too_many_arguments)]
pub fn compute_line_segment_line_segment_intersection(
    x00: f64,
    y00: f64,
    x01: f64,
    y01: f64,
    x10: f64,
    y10: f64,
    x11: f64,
    y11: f64,
) -> Option<Cartesian2> {
    let numerator1_a = (x11 - x10) * (y00 - y10) - (y11 - y10) * (x00 - x10);
    let numerator1_b = (x01 - x00) * (y00 - y10) - (y01 - y00) * (x00 - x10);
    let denominator1 = (y11 - y10) * (x01 - x00) - (x11 - x10) * (y01 - y00);
    if denominator1 == 0.0 {
        return None;
    }
    let ua1 = numerator1_a / denominator1;
    let ub1 = numerator1_b / denominator1;
    if (0.0..=1.0).contains(&ua1) && (0.0..=1.0).contains(&ub1) {
        return Some(Cartesian2::new(x00 + ua1 * (x01 - x00), y00 + ua1 * (y01 - y00)));
    }
    None
}
