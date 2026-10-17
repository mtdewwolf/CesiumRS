//! CesiumJS `Matrix2`: a column-major 2×2 matrix.

use super::cartesian2::Cartesian2;
use super::matrix::{matrix_common, matrix_scale_rotation};

/// Column-major storage: `[c0r0, c0r1, c1r0, c1r1]`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Matrix2(pub [f64; 4]);

matrix_common!(Matrix2, 2, 4, Cartesian2);
matrix_scale_rotation!(Matrix2, 2, Cartesian2);

impl Matrix2 {
    pub const IDENTITY: Self = Self([1.0, 0.0, 0.0, 1.0]);
    pub const ZERO: Self = Self([0.0; 4]);
    pub const COLUMN0ROW0: usize = 0;
    pub const COLUMN0ROW1: usize = 1;
    pub const COLUMN1ROW0: usize = 2;
    pub const COLUMN1ROW1: usize = 3;

    /// Arguments in row-major order, like Cesium's constructor.
    pub const fn new(column0_row0: f64, column1_row0: f64, column0_row1: f64, column1_row1: f64) -> Self {
        Self([column0_row0, column0_row1, column1_row0, column1_row1])
    }

    /// Counter-clockwise rotation by `angle` radians.
    pub fn from_rotation(angle: f64) -> Self {
        let (s, c) = angle.sin_cos();
        Self::new(c, -s, s, c)
    }
}
