//! CesiumJS `Quaternion`: `x i + y j + z k + w`.

use super::cartesian3::Cartesian3;
use super::heading_pitch_roll::HeadingPitchRoll;
use super::math::{acos_clamped, EPSILON6};
use super::matrix3::Matrix3;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Quaternion {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub w: f64,
}

impl Quaternion {
    pub const ZERO: Self = Self::new(0.0, 0.0, 0.0, 0.0);
    pub const IDENTITY: Self = Self::new(0.0, 0.0, 0.0, 1.0);
    pub const PACKED_LENGTH: usize = 4;
    /// Length of the rotation-vector form used when interpolating samples.
    pub const PACKED_INTERPOLATION_LENGTH: usize = 3;

    pub const fn new(x: f64, y: f64, z: f64, w: f64) -> Self {
        Self { x, y, z, w }
    }

    /// Rotation of `angle` radians about `axis` (normalized here).
    pub fn from_axis_angle(axis: Cartesian3, angle: f64) -> Self {
        let half = angle / 2.0;
        let s = half.sin();
        let a = axis.normalize();
        Self::new(a.x * s, a.y * s, a.z * s, half.cos())
    }

    pub fn from_rotation_matrix(matrix: &Matrix3) -> Self {
        let m = &matrix.0;
        let m00 = m[Matrix3::COLUMN0ROW0];
        let m11 = m[Matrix3::COLUMN1ROW1];
        let m22 = m[Matrix3::COLUMN2ROW2];
        let trace = m00 + m11 + m22;
        if trace > 0.0 {
            let mut root = (trace + 1.0).sqrt();
            let w = 0.5 * root;
            root = 0.5 / root;
            return Self::new(
                (m[Matrix3::COLUMN1ROW2] - m[Matrix3::COLUMN2ROW1]) * root,
                (m[Matrix3::COLUMN2ROW0] - m[Matrix3::COLUMN0ROW2]) * root,
                (m[Matrix3::COLUMN0ROW1] - m[Matrix3::COLUMN1ROW0]) * root,
                w,
            );
        }
        const NEXT: [usize; 3] = [1, 2, 0];
        let mut i = 0;
        if m11 > m00 {
            i = 1;
        }
        if m22 > m00 && m22 > m11 {
            i = 2;
        }
        let j = NEXT[i];
        let k = NEXT[j];
        let idx = Matrix3::get_element_index;
        let mut root = (m[idx(i, i)] - m[idx(j, j)] - m[idx(k, k)] + 1.0).sqrt();
        let mut quat = [0.0; 3];
        quat[i] = 0.5 * root;
        root = 0.5 / root;
        let w = (m[idx(k, j)] - m[idx(j, k)]) * root;
        quat[j] = (m[idx(j, i)] + m[idx(i, j)]) * root;
        quat[k] = (m[idx(k, i)] + m[idx(i, k)]) * root;
        Self::new(-quat[0], -quat[1], -quat[2], w)
    }

    /// `heading * pitch * roll`, rotating about -Z, -Y, and X respectively.
    pub fn from_heading_pitch_roll(hpr: &HeadingPitchRoll) -> Self {
        let roll = Self::from_axis_angle(Cartesian3::UNIT_X, hpr.roll);
        let pitch = Self::from_axis_angle(Cartesian3::UNIT_Y, -hpr.pitch);
        let pitch_roll = pitch.multiply(&roll);
        let heading = Self::from_axis_angle(Cartesian3::UNIT_Z, -hpr.heading);
        heading.multiply(&pitch_roll)
    }

    pub fn pack(&self, array: &mut [f64], starting_index: usize) {
        array[starting_index] = self.x;
        array[starting_index + 1] = self.y;
        array[starting_index + 2] = self.z;
        array[starting_index + 3] = self.w;
    }

    pub fn unpack(array: &[f64], starting_index: usize) -> Self {
        Self::new(
            array[starting_index],
            array[starting_index + 1],
            array[starting_index + 2],
            array[starting_index + 3],
        )
    }

    /// Converts packed samples `starting_index..=last_index` to rotation
    /// vectors relative to the sample at `last_index`, three values each.
    pub fn convert_packed_array_for_interpolation(packed: &[f64], starting_index: usize, last_index: usize) -> Vec<f64> {
        let q0_conjugate = Self::unpack(packed, last_index * 4).conjugate();
        let mut out = vec![0.0; (last_index - starting_index + 1) * 3];
        for i in 0..=last_index - starting_index {
            let mut t = Self::unpack(packed, (starting_index + i) * 4).multiply(&q0_conjugate);
            if t.w < 0.0 {
                t = t.negate();
            }
            let axis = t.compute_axis();
            let angle = t.compute_angle();
            out[i * 3] = axis.x * angle;
            out[i * 3 + 1] = axis.y * angle;
            out[i * 3 + 2] = axis.z * angle;
        }
        out
    }

    /// Inverse of [`Quaternion::convert_packed_array_for_interpolation`] for one result.
    pub fn unpack_interpolation_result(array: &[f64], source: &[f64], _first_index: usize, last_index: usize) -> Self {
        let rotation = Cartesian3::from_array(array, 0);
        let magnitude = rotation.magnitude();
        let q0 = Self::unpack(source, last_index * 4);
        let t = if magnitude == 0.0 {
            Self::IDENTITY
        } else {
            Self::from_axis_angle(rotation, magnitude)
        };
        t.multiply(&q0)
    }

    pub fn conjugate(&self) -> Self {
        Self::new(-self.x, -self.y, -self.z, self.w)
    }

    pub fn magnitude_squared(&self) -> f64 {
        self.x * self.x + self.y * self.y + self.z * self.z + self.w * self.w
    }

    pub fn magnitude(&self) -> f64 {
        self.magnitude_squared().sqrt()
    }

    pub fn normalize(&self) -> Self {
        let inv = 1.0 / self.magnitude();
        Self::new(self.x * inv, self.y * inv, self.z * inv, self.w * inv)
    }

    pub fn inverse(&self) -> Self {
        let m2 = self.magnitude_squared();
        self.conjugate().multiply_by_scalar(1.0 / m2)
    }

    pub fn add(&self, r: &Self) -> Self {
        Self::new(self.x + r.x, self.y + r.y, self.z + r.z, self.w + r.w)
    }

    pub fn subtract(&self, r: &Self) -> Self {
        Self::new(self.x - r.x, self.y - r.y, self.z - r.z, self.w - r.w)
    }

    pub fn negate(&self) -> Self {
        Self::new(-self.x, -self.y, -self.z, -self.w)
    }

    pub fn dot(&self, r: &Self) -> f64 {
        self.x * r.x + self.y * r.y + self.z * r.z + self.w * r.w
    }

    /// Hamilton product `self * right`.
    pub fn multiply(&self, right: &Self) -> Self {
        let (lx, ly, lz, lw) = (self.x, self.y, self.z, self.w);
        let (rx, ry, rz, rw) = (right.x, right.y, right.z, right.w);
        Self::new(
            lw * rx + lx * rw + ly * rz - lz * ry,
            lw * ry - lx * rz + ly * rw + lz * rx,
            lw * rz + lx * ry - ly * rx + lz * rw,
            lw * rw - lx * rx - ly * ry - lz * rz,
        )
    }

    pub fn multiply_by_scalar(&self, s: f64) -> Self {
        Self::new(self.x * s, self.y * s, self.z * s, self.w * s)
    }

    pub fn divide_by_scalar(&self, s: f64) -> Self {
        Self::new(self.x / s, self.y / s, self.z / s, self.w / s)
    }

    /// Rotation axis; `UNIT_X` for a (near-)identity rotation.
    pub fn compute_axis(&self) -> Cartesian3 {
        let w = self.w;
        if (w - 1.0).abs() < EPSILON6 || (w + 1.0).abs() < EPSILON6 {
            return Cartesian3::UNIT_X;
        }
        let scalar = 1.0 / (1.0 - w * w).sqrt();
        Cartesian3::new(self.x * scalar, self.y * scalar, self.z * scalar)
    }

    pub fn compute_angle(&self) -> f64 {
        if (self.w - 1.0).abs() < EPSILON6 {
            return 0.0;
        }
        2.0 * self.w.acos()
    }

    pub fn lerp(&self, end: &Self, t: f64) -> Self {
        let scaled_end = end.multiply_by_scalar(t);
        let scaled_start = self.multiply_by_scalar(1.0 - t);
        scaled_end.add(&scaled_start)
    }

    /// Spherical linear interpolation along the shorter arc.
    pub fn slerp(&self, end: &Self, t: f64) -> Self {
        let mut dot = self.dot(end);
        let mut r = *end;
        if dot < 0.0 {
            dot = -dot;
            r = end.negate();
        }
        if 1.0 - dot < EPSILON6 {
            return self.lerp(&r, t);
        }
        let theta = dot.acos();
        let p = self.multiply_by_scalar(((1.0 - t) * theta).sin());
        let q = r.multiply_by_scalar((t * theta).sin());
        p.add(&q).multiply_by_scalar(1.0 / theta.sin())
    }

    /// Logarithm of a unit quaternion as a rotation vector.
    pub fn log(&self) -> Cartesian3 {
        let theta = acos_clamped(self.w);
        let k = if theta != 0.0 { theta / theta.sin() } else { 0.0 };
        Cartesian3::new(self.x * k, self.y * k, self.z * k)
    }

    pub fn exp(cartesian: Cartesian3) -> Self {
        let theta = cartesian.magnitude();
        let k = if theta != 0.0 { theta.sin() / theta } else { 0.0 };
        Self::new(cartesian.x * k, cartesian.y * k, cartesian.z * k, theta.cos())
    }

    /// Inner quadrangle point for `squad` at `q1`.
    pub fn compute_inner_quadrangle(q0: &Self, q1: &Self, q2: &Self) -> Self {
        let q_inv = q1.conjugate();
        let cart0 = q_inv.multiply(q2).log();
        let cart1 = q_inv.multiply(q0).log();
        let c = cart0.add(cart1).multiply_by_scalar(0.25).negate();
        q1.multiply(&Self::exp(c))
    }

    /// Spherical quadrangle interpolation.
    pub fn squad(q0: &Self, q1: &Self, s0: &Self, s1: &Self, t: f64) -> Self {
        let slerp0 = q0.slerp(q1, t);
        let slerp1 = s0.slerp(s1, t);
        slerp0.slerp(&slerp1, 2.0 * t * (1.0 - t))
    }

    /// Eberly's polynomial approximation of `slerp`. Cesium keeps the
    /// coefficients in `Float32Array`s, so they are rounded to f32 here too.
    pub fn fast_slerp(&self, end: &Self, t: f64) -> Self {
        const OPMU: f64 = 1.901_107_453_517_300_37;
        let f32r = |v: f64| v as f32 as f64;
        let mut u = [0.0; 8];
        let mut v = [0.0; 8];
        for i in 0..7 {
            let s = i as f64 + 1.0;
            let tt = 2.0 * s + 1.0;
            u[i] = f32r(1.0 / (s * tt));
            v[i] = f32r(s / tt);
        }
        u[7] = f32r(OPMU / (8.0 * 17.0));
        v[7] = f32r((OPMU * 8.0) / 17.0);

        let mut x = self.dot(end);
        let sign = if x >= 0.0 {
            1.0
        } else {
            x = -x;
            -1.0
        };
        let xm1 = x - 1.0;
        let d = 1.0 - t;
        let sqr_t = t * t;
        let sqr_d = d * d;
        let mut bt = [0.0; 8];
        let mut bd = [0.0; 8];
        for i in (0..8).rev() {
            bt[i] = f32r((u[i] * sqr_t - v[i]) * xm1);
            bd[i] = f32r((u[i] * sqr_d - v[i]) * xm1);
        }
        let horner = |b: &[f64; 8]| {
            1.0 + b[0]
                * (1.0
                    + b[1] * (1.0 + b[2] * (1.0 + b[3] * (1.0 + b[4] * (1.0 + b[5] * (1.0 + b[6] * (1.0 + b[7])))))))
        };
        let ct = sign * t * horner(&bt);
        let cd = d * horner(&bd);
        let temp = self.multiply_by_scalar(cd);
        let r = end.multiply_by_scalar(ct);
        temp.add(&r)
    }

    pub fn fast_squad(q0: &Self, q1: &Self, s0: &Self, s1: &Self, t: f64) -> Self {
        let slerp0 = q0.fast_slerp(q1, t);
        let slerp1 = s0.fast_slerp(s1, t);
        slerp0.fast_slerp(&slerp1, 2.0 * t * (1.0 - t))
    }

    pub fn equals(&self, r: &Self) -> bool {
        self == r
    }

    /// Every component within `epsilon` (absolute).
    pub fn equals_epsilon(&self, r: &Self, epsilon: f64) -> bool {
        (self.x - r.x).abs() <= epsilon
            && (self.y - r.y).abs() <= epsilon
            && (self.z - r.z).abs() <= epsilon
            && (self.w - r.w).abs() <= epsilon
    }
}

impl std::ops::Mul for Quaternion {
    type Output = Self;
    fn mul(self, r: Self) -> Self {
        self.multiply(&r)
    }
}

impl std::fmt::Display for Quaternion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "({}, {}, {}, {})", self.x, self.y, self.z, self.w)
    }
}
