//! CesiumJS `InterpolationAlgorithm`, implemented by the existing interpolators.

use super::{
    hermite_polynomial_approximation as hermite, lagrange_polynomial_approximation as lagrange,
    linear_approximation as linear,
};

pub struct InterpolationSamples<'a> {
    pub x: f64,
    pub x_table: &'a [f64],
    pub y_table: &'a [f64],
    pub y_stride: usize,
}

pub trait InterpolationAlgorithm {
    fn algorithm_type(&self) -> &'static str;
    fn get_required_data_points(&self, degree: f64, input_order: f64) -> f64;
    fn interpolate_order_zero(&self, samples: &InterpolationSamples<'_>) -> Option<Vec<f64>>;
    /// `None` means this algorithm has no higher-order interpolation method.
    fn interpolate(
        &self,
        _samples: &InterpolationSamples<'_>,
        _input_order: usize,
        _output_order: usize,
    ) -> Option<Vec<f64>> {
        None
    }
}

pub struct LinearApproximation;
pub struct LagrangePolynomialApproximation;
pub struct HermitePolynomialApproximation;

impl InterpolationAlgorithm for LinearApproximation {
    fn algorithm_type(&self) -> &'static str {
        "Linear"
    }
    fn get_required_data_points(&self, degree: f64, _: f64) -> f64 {
        linear::get_required_data_points(degree)
    }
    fn interpolate_order_zero(&self, s: &InterpolationSamples<'_>) -> Option<Vec<f64>> {
        if s.y_table.len() < s.y_stride.checked_mul(2)? {
            return None;
        }
        linear::interpolate_order_zero(s.x, s.x_table, s.y_table, s.y_stride)
    }
}
impl InterpolationAlgorithm for LagrangePolynomialApproximation {
    fn algorithm_type(&self) -> &'static str {
        "Lagrange"
    }
    fn get_required_data_points(&self, degree: f64, _: f64) -> f64 {
        lagrange::get_required_data_points(degree)
    }
    fn interpolate_order_zero(&self, s: &InterpolationSamples<'_>) -> Option<Vec<f64>> {
        if s.y_table.len() < s.x_table.len().checked_mul(s.y_stride)? {
            return None;
        }
        Some(lagrange::interpolate_order_zero(s.x, s.x_table, s.y_table, s.y_stride))
    }
}
impl InterpolationAlgorithm for HermitePolynomialApproximation {
    fn algorithm_type(&self) -> &'static str {
        "Hermite"
    }
    fn get_required_data_points(&self, degree: f64, input_order: f64) -> f64 {
        hermite::get_required_data_points(degree, input_order)
    }
    fn interpolate_order_zero(&self, s: &InterpolationSamples<'_>) -> Option<Vec<f64>> {
        if s.y_table.len() < s.x_table.len().checked_mul(s.y_stride)? {
            return None;
        }
        Some(hermite::interpolate_order_zero(s.x, s.x_table, s.y_table, s.y_stride))
    }
    fn interpolate(&self, s: &InterpolationSamples<'_>, input_order: usize, output_order: usize) -> Option<Vec<f64>> {
        let length = s.x_table.len().checked_mul(s.y_stride)?.checked_mul(input_order.checked_add(1)?)?;
        if s.y_table.len() < length {
            return None;
        }
        Some(hermite::interpolate(s.x, s.x_table, s.y_table, s.y_stride, input_order, output_order))
    }
}
