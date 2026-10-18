//! CesiumJS `Matrix3`: a column-major 3×3 matrix.

use super::cartesian3::Cartesian3;
use super::heading_pitch_roll::HeadingPitchRoll;
use super::js_math::JsMath;
use super::math::{EPSILON15, EPSILON20};
use super::matrix::{matrix_common, matrix_scale_rotation};
use super::quaternion::Quaternion;

/// Column-major storage, element `column * 3 + row`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Matrix3(pub [f64; 9]);

matrix_common!(Matrix3, 3, 9, Cartesian3);
matrix_scale_rotation!(Matrix3, 3, Cartesian3);

/// Result of [`Matrix3::compute_eigen_decomposition`]: `matrix = unitary * diagonal * unitaryᵀ`.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct EigenDecomposition {
    pub unitary: Matrix3,
    pub diagonal: Matrix3,
}

impl Matrix3 {
    pub const IDENTITY: Self = Self([1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0]);
    pub const ZERO: Self = Self([0.0; 9]);
    pub const COLUMN0ROW0: usize = 0;
    pub const COLUMN0ROW1: usize = 1;
    pub const COLUMN0ROW2: usize = 2;
    pub const COLUMN1ROW0: usize = 3;
    pub const COLUMN1ROW1: usize = 4;
    pub const COLUMN1ROW2: usize = 5;
    pub const COLUMN2ROW0: usize = 6;
    pub const COLUMN2ROW1: usize = 7;
    pub const COLUMN2ROW2: usize = 8;

    /// Arguments in row-major order, like Cesium's constructor.
    #[allow(clippy::too_many_arguments)]
    pub const fn new(
        column0_row0: f64,
        column1_row0: f64,
        column2_row0: f64,
        column0_row1: f64,
        column1_row1: f64,
        column2_row1: f64,
        column0_row2: f64,
        column1_row2: f64,
        column2_row2: f64,
    ) -> Self {
        Self([
            column0_row0,
            column0_row1,
            column0_row2,
            column1_row0,
            column1_row1,
            column1_row2,
            column2_row0,
            column2_row1,
            column2_row2,
        ])
    }

    pub fn from_quaternion(q: &Quaternion) -> Self {
        let x2 = q.x * q.x;
        let xy = q.x * q.y;
        let xz = q.x * q.z;
        let xw = q.x * q.w;
        let y2 = q.y * q.y;
        let yz = q.y * q.z;
        let yw = q.y * q.w;
        let z2 = q.z * q.z;
        let zw = q.z * q.w;
        let w2 = q.w * q.w;
        Self::new(
            x2 - y2 - z2 + w2,
            2.0 * (xy - zw),
            2.0 * (xz + yw),
            2.0 * (xy + zw),
            -x2 + y2 - z2 + w2,
            2.0 * (yz - xw),
            2.0 * (xz - yw),
            2.0 * (yz + xw),
            -x2 - y2 + z2 + w2,
        )
    }

    /// Rotation for a heading, pitch, and roll in a local frame.
    pub fn from_heading_pitch_roll(hpr: &HeadingPitchRoll) -> Self {
        let cos_theta = (-hpr.pitch).js_cos();
        let cos_psi = (-hpr.heading).js_cos();
        let cos_phi = hpr.roll.js_cos();
        let sin_theta = (-hpr.pitch).js_sin();
        let sin_psi = (-hpr.heading).js_sin();
        let sin_phi = hpr.roll.js_sin();
        Self::new(
            cos_theta * cos_psi,
            -cos_phi * sin_psi + sin_phi * sin_theta * cos_psi,
            sin_phi * sin_psi + cos_phi * sin_theta * cos_psi,
            cos_theta * sin_psi,
            cos_phi * cos_psi + sin_phi * sin_theta * sin_psi,
            -sin_phi * cos_psi + cos_phi * sin_theta * sin_psi,
            -sin_theta,
            sin_phi * cos_theta,
            cos_phi * cos_theta,
        )
    }

    /// The matrix `[v]×` with `[v]× w = v × w`.
    pub fn from_cross_product(v: Cartesian3) -> Self {
        Self::new(0.0, -v.z, v.y, v.z, 0.0, -v.x, -v.y, v.x, 0.0)
    }

    pub fn from_rotation_x(angle: f64) -> Self {
        let (s, c) = angle.js_sin_cos();
        Self::new(1.0, 0.0, 0.0, 0.0, c, -s, 0.0, s, c)
    }

    pub fn from_rotation_y(angle: f64) -> Self {
        let (s, c) = angle.js_sin_cos();
        Self::new(c, 0.0, s, 0.0, 1.0, 0.0, -s, 0.0, c)
    }

    pub fn from_rotation_z(angle: f64) -> Self {
        let (s, c) = angle.js_sin_cos();
        Self::new(c, -s, 0.0, s, c, 0.0, 0.0, 0.0, 1.0)
    }

    pub fn determinant(&self) -> f64 {
        let m = &self.0;
        let (m11, m21, m31) = (m[0], m[3], m[6]);
        let (m12, m22, m32) = (m[1], m[4], m[7]);
        let (m13, m23, m33) = (m[2], m[5], m[8]);
        m11 * (m22 * m33 - m23 * m32) + m12 * (m23 * m31 - m21 * m33) + m13 * (m21 * m32 - m22 * m31)
    }

    /// Inverse by cofactors. A singular matrix yields infinite or NaN
    /// elements, as in Cesium's release build; see [`Matrix3::try_inverse`].
    pub fn inverse(&self) -> Self {
        let m = &self.0;
        let (m11, m21, m31) = (m[0], m[1], m[2]);
        let (m12, m22, m32) = (m[3], m[4], m[5]);
        let (m13, m23, m33) = (m[6], m[7], m[8]);
        let scale = 1.0 / self.determinant();
        Self([
            m22 * m33 - m23 * m32,
            m23 * m31 - m21 * m33,
            m21 * m32 - m22 * m31,
            m13 * m32 - m12 * m33,
            m11 * m33 - m13 * m31,
            m12 * m31 - m11 * m32,
            m12 * m23 - m13 * m22,
            m13 * m21 - m11 * m23,
            m11 * m22 - m12 * m21,
        ])
        .multiply_by_scalar(scale)
    }

    /// `None` when `|det| <= EPSILON15`, Cesium's debug-build check.
    pub fn try_inverse(&self) -> Option<Self> {
        (self.determinant().abs() > EPSILON15).then(|| self.inverse())
    }

    pub fn inverse_transpose(&self) -> Self {
        self.transpose().inverse()
    }

    /// Jacobi eigen-decomposition of a symmetric matrix.
    pub fn compute_eigen_decomposition(&self) -> EigenDecomposition {
        const MAX_SWEEPS: u32 = 10;
        let mut count = 0;
        let mut sweep = 0;
        let mut unitary = Self::IDENTITY;
        let mut diagonal = *self;
        let epsilon = EPSILON20 * frobenius_norm(&diagonal);
        while sweep < MAX_SWEEPS && off_diagonal_frobenius_norm(&diagonal) > epsilon {
            let j = shur_decomposition(&diagonal);
            let jt = j.transpose();
            diagonal = diagonal.multiply(&j);
            diagonal = jt.multiply(&diagonal);
            unitary = unitary.multiply(&j);
            count += 1;
            if count > 2 {
                sweep += 1;
                count = 0;
            }
        }
        EigenDecomposition { unitary, diagonal }
    }
}

const ROW_VAL: [usize; 3] = [1, 0, 0];
const COL_VAL: [usize; 3] = [2, 2, 1];

fn frobenius_norm(m: &Matrix3) -> f64 {
    let mut norm = 0.0;
    for v in m.0 {
        norm += v * v;
    }
    norm.sqrt()
}

fn off_diagonal_frobenius_norm(m: &Matrix3) -> f64 {
    let mut norm = 0.0;
    for i in 0..3 {
        let t = m.0[Matrix3::get_element_index(COL_VAL[i], ROW_VAL[i])];
        norm += 2.0 * t * t;
    }
    norm.sqrt()
}

/// One Jacobi rotation that zeroes the largest off-diagonal element.
fn shur_decomposition(m: &Matrix3) -> Matrix3 {
    let mut max_diagonal = 0.0;
    let mut rot_axis = 1;
    for i in 0..3 {
        let t = m.0[Matrix3::get_element_index(COL_VAL[i], ROW_VAL[i])].abs();
        if t > max_diagonal {
            rot_axis = i;
            max_diagonal = t;
        }
    }
    let mut c = 1.0;
    let mut s = 0.0;
    let p = ROW_VAL[rot_axis];
    let q = COL_VAL[rot_axis];
    let idx = Matrix3::get_element_index;
    if m.0[idx(q, p)].abs() > EPSILON15 {
        let qq = m.0[idx(q, q)];
        let pp = m.0[idx(p, p)];
        let qp = m.0[idx(q, p)];
        let tau = (qq - pp) / 2.0 / qp;
        let t =
            if tau < 0.0 { -1.0 / (-tau + (1.0 + tau * tau).sqrt()) } else { 1.0 / (tau + (1.0 + tau * tau).sqrt()) };
        c = 1.0 / (1.0 + t * t).sqrt();
        s = t * c;
    }
    let mut r = Matrix3::IDENTITY;
    r.0[idx(p, p)] = c;
    r.0[idx(q, q)] = c;
    r.0[idx(q, p)] = s;
    r.0[idx(p, q)] = -s;
    r
}
