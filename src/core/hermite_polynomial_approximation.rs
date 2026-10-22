//! CesiumJS `HermitePolynomialApproximation`: Hermite interpolation from
//! values and, optionally, derivatives (divided differences).

use super::math::factorial;

fn calculate_coefficient_term(
    x: f64,
    z_indices: &[usize],
    x_table: &[f64],
    deriv_order: usize,
    term_order: usize,
    reserved_indices: &mut Vec<usize>,
) -> f64 {
    if deriv_order > 0 {
        let mut result = 0.0;
        for i in 0..term_order {
            if !reserved_indices.contains(&i) {
                reserved_indices.push(i);
                result +=
                    calculate_coefficient_term(x, z_indices, x_table, deriv_order - 1, term_order, reserved_indices);
                reserved_indices.pop();
            }
        }
        return result;
    }
    let mut result = 1.0;
    for i in 0..term_order {
        if !reserved_indices.contains(&i) {
            result *= x - x_table[z_indices[i]];
        }
    }
    result
}

/// Samples needed for a polynomial of `degree` when each sample carries
/// `input_order` derivatives.
pub fn get_required_data_points(degree: f64, input_order: f64) -> f64 {
    ((degree + 1.0) / (input_order + 1.0)).floor().max(2.0)
}

/// Interpolates values only. Cesium's quirks are kept: the highest
/// coefficient used is reset by the last all-zero difference row of any
/// dependent value.
pub fn interpolate_order_zero(x: f64, x_table: &[f64], y_table: &[f64], y_stride: usize) -> Vec<f64> {
    let mut result = vec![0.0; y_stride];
    let length = x_table.len();
    let mut coefficients: Vec<Vec<Vec<f64>>> = vec![vec![Vec::new(); length]; y_stride];
    let z_indices: Vec<usize> = (0..length).collect();

    let mut highest_non_zero_coef = length as isize - 1;
    for s in 0..y_stride {
        for &z in &z_indices {
            coefficients[s][0].push(y_table[z * y_stride + s]);
        }
        for i in 1..length {
            let mut non_zero_coefficients = false;
            for j in 0..length - i {
                let zj = x_table[z_indices[j]];
                let zn = x_table[z_indices[j + i]];
                let numerator;
                if zn - zj <= 0.0 {
                    numerator = y_table[z_indices[j] * y_stride + y_stride * i + s];
                    coefficients[s][i].push(numerator / factorial(i as u32));
                } else {
                    numerator = coefficients[s][i - 1][j + 1] - coefficients[s][i - 1][j];
                    coefficients[s][i].push(numerator / (zn - zj));
                }
                non_zero_coefficients = non_zero_coefficients || numerator != 0.0;
            }
            if !non_zero_coefficients {
                highest_non_zero_coef = i as isize - 1;
            }
        }
    }

    for i in 0..=highest_non_zero_coef.max(-1) {
        let i = i as usize;
        let temp_term = calculate_coefficient_term(x, &z_indices, x_table, 0, i, &mut Vec::new());
        for s in 0..y_stride {
            result[s] += coefficients[s][i][0] * temp_term;
        }
    }
    result
}

/// Interpolates values and up to `output_order` derivatives from samples that
/// carry `input_order` derivatives each. The result holds `y_stride` values
/// per order.
pub fn interpolate(
    x: f64,
    x_table: &[f64],
    y_table: &[f64],
    y_stride: usize,
    input_order: usize,
    output_order: usize,
) -> Vec<f64> {
    let mut result = vec![0.0; y_stride * (output_order + 1)];
    let length = x_table.len();
    let mut z_indices = Vec::with_capacity(length * (input_order + 1));
    for i in 0..length {
        for _ in 0..=input_order {
            z_indices.push(i);
        }
    }
    let z_len = z_indices.len() as i64;
    let tmp = (z_len * (z_len + 1)) / 2;
    let dim_two = |i: i64| (i * (1 - i)).div_euclid(2) + z_len * i;

    // fillCoefficientList, with a flat array like Cesium's.
    let mut coefficients = vec![0.0; (tmp * y_stride as i64).max(1) as usize];
    let mut highest_non_zero: i64 = -1;
    for s in 0..y_stride {
        let dim_one = s as i64 * tmp;
        for j in 0..z_len {
            let index = z_indices[j as usize] * y_stride * (input_order + 1) + s;
            coefficients[(dim_one + j) as usize] = y_table[index];
        }
        for i in 1..z_len {
            let dim_two_i = dim_two(i);
            let mut non_zero_coefficients = false;
            for (coef_index, j) in (0..z_len - i).enumerate() {
                let coef_index = coef_index as i64;
                let zj = x_table[z_indices[j as usize]];
                let zn = x_table[z_indices[(j + i) as usize]];
                let (numerator, coefficient) = if zn - zj <= 0.0 {
                    let index = z_indices[j as usize] * y_stride * (input_order + 1) + y_stride * i as usize + s;
                    let numerator = y_table[index];
                    (numerator, numerator / factorial(i as u32))
                } else {
                    let dim_two_minus_one = ((i - 1) * (2 - i)).div_euclid(2) + z_len * (i - 1);
                    let numerator = coefficients[(dim_one + dim_two_minus_one + j + 1) as usize]
                        - coefficients[(dim_one + dim_two_minus_one + j) as usize];
                    (numerator, numerator / (zn - zj))
                };
                coefficients[(dim_one + dim_two_i + coef_index) as usize] = coefficient;
                non_zero_coefficients = non_zero_coefficients || numerator != 0.0;
            }
            if non_zero_coefficients {
                highest_non_zero = highest_non_zero.max(i);
            }
        }
    }

    let loop_stop = highest_non_zero.min(output_order as i64);
    let mut reserved_indices = Vec::new();
    let mut d = 0;
    while d <= loop_stop {
        for i in d..=highest_non_zero {
            reserved_indices.clear();
            let temp_term =
                calculate_coefficient_term(x, &z_indices, x_table, d as usize, i as usize, &mut reserved_indices);
            let dim_two_i = dim_two(i);
            for s in 0..y_stride {
                let dim_one = s as i64 * tmp;
                result[s + d as usize * y_stride] += coefficients[(dim_one + dim_two_i) as usize] * temp_term;
            }
        }
        d += 1;
    }
    result
}
