//! Column-major matrices and Hamilton quaternions, matching CesiumJS.

pub const EPSILON1: f64 = 0.1;
pub const EPSILON10: f64 = 1e-10;
pub const EPSILON12: f64 = 1e-12;
pub const EPSILON14: f64 = 1e-14;
pub const PI: f64 = std::f64::consts::PI;
pub const TAU: f64 = std::f64::consts::TAU;
pub const PI_OVER_TWO: f64 = std::f64::consts::FRAC_PI_2;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Vec3 {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl Vec3 {
    pub const ZERO: Self = Self { x: 0.0, y: 0.0, z: 0.0 };
    pub const UNIT_X: Self = Self { x: 1.0, y: 0.0, z: 0.0 };
    pub const UNIT_Y: Self = Self { x: 0.0, y: 1.0, z: 0.0 };
    pub const UNIT_Z: Self = Self { x: 0.0, y: 0.0, z: 1.0 };

    pub const fn new(x: f64, y: f64, z: f64) -> Self {
        Self { x, y, z }
    }

    pub fn dot(self, o: Self) -> f64 {
        self.x * o.x + self.y * o.y + self.z * o.z
    }

    pub fn cross(self, o: Self) -> Self {
        Self::new(self.y * o.z - self.z * o.y, self.z * o.x - self.x * o.z, self.x * o.y - self.y * o.x)
    }

    pub fn magnitude_squared(self) -> f64 {
        self.dot(self)
    }

    pub fn magnitude(self) -> f64 {
        self.magnitude_squared().sqrt()
    }

    pub fn normalize(self) -> Self {
        let m = self.magnitude();
        if m == 0.0 {
            Self::ZERO
        } else {
            self / m
        }
    }

    pub fn mul_components(self, o: Self) -> Self {
        Self::new(self.x * o.x, self.y * o.y, self.z * o.z)
    }

    pub fn negate(self) -> Self {
        Self::new(-self.x, -self.y, -self.z)
    }

    pub fn lerp(self, o: Self, t: f64) -> Self {
        self + (o - self) * t
    }
}

impl std::ops::Add for Vec3 {
    type Output = Self;
    fn add(self, o: Self) -> Self {
        Self::new(self.x + o.x, self.y + o.y, self.z + o.z)
    }
}

impl std::ops::Sub for Vec3 {
    type Output = Self;
    fn sub(self, o: Self) -> Self {
        Self::new(self.x - o.x, self.y - o.y, self.z - o.z)
    }
}

impl std::ops::Mul<f64> for Vec3 {
    type Output = Self;
    fn mul(self, s: f64) -> Self {
        Self::new(self.x * s, self.y * s, self.z * s)
    }
}

impl std::ops::Div<f64> for Vec3 {
    type Output = Self;
    fn div(self, s: f64) -> Self {
        self * (1.0 / s)
    }
}

impl std::ops::Neg for Vec3 {
    type Output = Self;
    fn neg(self) -> Self {
        self.negate()
    }
}

#[derive(Clone, Copy, Debug)]
pub struct Quat {
    pub x: f64,
    pub y: f64,
    pub z: f64,
    pub w: f64,
}

impl Quat {
    pub fn from_axis_angle(axis: Vec3, angle: f64) -> Self {
        let half = angle * 0.5;
        let s = half.sin();
        Self { x: axis.x * s, y: axis.y * s, z: axis.z * s, w: half.cos() }
    }

    /// Hamilton product. Cesium `Quaternion.multiply(left, right)`.
    pub fn mul(self, right: Self) -> Self {
        let (lx, ly, lz, lw) = (self.x, self.y, self.z, self.w);
        let (rx, ry, rz, rw) = (right.x, right.y, right.z, right.w);
        Self {
            x: lw * rx + lx * rw + ly * rz - lz * ry,
            y: lw * ry - lx * rz + ly * rw + lz * rx,
            z: lw * rz + lx * ry - ly * rx + lz * rw,
            w: lw * rw - lx * rx - ly * ry - lz * rz,
        }
    }
}

/// Column-major 3×3, Cesium `Matrix3`.
pub type Mat3 = [f64; 9];
/// Column-major 4×4, Cesium `Matrix4`.
pub type Mat4 = [f64; 16];

pub fn mat3_from_quaternion(q: Quat) -> Mat3 {
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
    let m00 = x2 - y2 - z2 + w2;
    let m01 = 2.0 * (xy - zw);
    let m02 = 2.0 * (xz + yw);
    let m10 = 2.0 * (xy + zw);
    let m11 = -x2 + y2 - z2 + w2;
    let m12 = 2.0 * (yz - xw);
    let m20 = 2.0 * (xz - yw);
    let m21 = 2.0 * (yz + xw);
    let m22 = -x2 - y2 + z2 + w2;
    [m00, m10, m20, m01, m11, m21, m02, m12, m22]
}

pub fn mat3_mul_vec(m: &Mat3, v: Vec3) -> Vec3 {
    Vec3::new(
        m[0] * v.x + m[3] * v.y + m[6] * v.z,
        m[1] * v.x + m[4] * v.y + m[7] * v.z,
        m[2] * v.x + m[5] * v.y + m[8] * v.z,
    )
}

pub fn mat4_mul(a: &Mat4, b: &Mat4) -> Mat4 {
    let mut o = [0.0; 16];
    for col in 0..4 {
        for row in 0..4 {
            o[col * 4 + row] = a[row] * b[col * 4]
                + a[4 + row] * b[col * 4 + 1]
                + a[8 + row] * b[col * 4 + 2]
                + a[12 + row] * b[col * 4 + 3];
        }
    }
    o
}

pub fn mat4_mul_point(m: &Mat4, v: Vec3) -> Vec3 {
    Vec3::new(
        m[0] * v.x + m[4] * v.y + m[8] * v.z + m[12],
        m[1] * v.x + m[5] * v.y + m[9] * v.z + m[13],
        m[2] * v.x + m[6] * v.y + m[10] * v.z + m[14],
    )
}

pub fn mat4_mul_dir(m: &Mat4, v: Vec3) -> Vec3 {
    Vec3::new(
        m[0] * v.x + m[4] * v.y + m[8] * v.z,
        m[1] * v.x + m[5] * v.y + m[9] * v.z,
        m[2] * v.x + m[6] * v.y + m[10] * v.z,
    )
}

pub fn sign(v: f64) -> f64 {
    if v > 0.0 {
        1.0
    } else if v < 0.0 {
        -1.0
    } else {
        0.0
    }
}

pub fn clamp(v: f64, min: f64, max: f64) -> f64 {
    v.max(min).min(max)
}

pub fn zero_to_two_pi(a: f64) -> f64 {
    let mut r = a % TAU;
    if r < 0.0 {
        r += TAU;
    }
    r
}

pub fn equals_epsilon(a: f64, b: f64, eps: f64) -> bool {
    (a - b).abs() <= eps
}

pub fn to_radians(deg: f64) -> f64 {
    deg * PI / 180.0
}

pub fn to_degrees(rad: f64) -> f64 {
    rad * 180.0 / PI
}
