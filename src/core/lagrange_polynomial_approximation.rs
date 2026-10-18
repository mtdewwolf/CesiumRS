//! CesiumJS `LagrangePolynomialApproximation`.

pub fn get_required_data_points(degree: f64) -> f64 {
    (degree + 1.0).max(2.0)
}

/// The Lagrange polynomial through `(x_table[i], y_table[i * y_stride + j])`
/// at `x`, for each of the `y_stride` dependent values.
pub fn interpolate_order_zero(x: f64, x_table: &[f64], y_table: &[f64], y_stride: usize) -> Vec<f64> {
    let mut result = vec![0.0; y_stride];
    let length = x_table.len();
    for i in 0..length {
        let mut coefficient = 1.0;
        for j in 0..length {
            if j != i {
                let diff_x = x_table[i] - x_table[j];
                coefficient *= (x - x_table[j]) / diff_x;
            }
        }
        for (j, r) in result.iter_mut().enumerate() {
            *r += coefficient * y_table[i * y_stride + j];
        }
    }
    result
}
