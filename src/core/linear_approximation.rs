//! CesiumJS `LinearApproximation`.

pub fn get_required_data_points(_degree: f64) -> f64 {
    2.0
}

/// Linear interpolation between two samples. `None` unless `x_table` has two
/// distinct values and `y_stride` is positive (Cesium throws).
pub fn interpolate_order_zero(x: f64, x_table: &[f64], y_table: &[f64], y_stride: usize) -> Option<Vec<f64>> {
    if x_table.len() != 2 || y_stride == 0 || x_table[0] == x_table[1] {
        return None;
    }
    let (x0, x1) = (x_table[0], x_table[1]);
    Some(
        (0..y_stride)
            .map(|i| {
                let y0 = y_table[i];
                let y1 = y_table[i + y_stride];
                ((y1 - y0) * x + x1 * y0 - x0 * y1) / (x1 - x0)
            })
            .collect(),
    )
}
