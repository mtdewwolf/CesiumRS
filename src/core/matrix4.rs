//! CesiumJS `Matrix4`: a column-major 4×4 matrix.

use super::bounding_rectangle::BoundingRectangle;
use super::cartesian3::Cartesian3;
use super::cartesian4::Cartesian4;
use super::math::{EPSILON21, EPSILON7};
use super::matrix::matrix_common;
use super::matrix3::Matrix3;
use super::quaternion::Quaternion;
use super::runtime_error::RuntimeError;
use super::translation_rotation_scale::TranslationRotationScale;

/// Column-major storage, element `column * 4 + row`.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Matrix4(pub [f64; 16]);

matrix_common!(Matrix4, 4, 16, Cartesian4);

impl Matrix4 {
    pub const IDENTITY: Self = Self([
        1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0,
    ]);
    pub const ZERO: Self = Self([0.0; 16]);
    pub const COLUMN0ROW0: usize = 0;
    pub const COLUMN0ROW1: usize = 1;
    pub const COLUMN0ROW2: usize = 2;
    pub const COLUMN0ROW3: usize = 3;
    pub const COLUMN1ROW0: usize = 4;
    pub const COLUMN1ROW1: usize = 5;
    pub const COLUMN1ROW2: usize = 6;
    pub const COLUMN1ROW3: usize = 7;
    pub const COLUMN2ROW0: usize = 8;
    pub const COLUMN2ROW1: usize = 9;
    pub const COLUMN2ROW2: usize = 10;
    pub const COLUMN2ROW3: usize = 11;
    pub const COLUMN3ROW0: usize = 12;
    pub const COLUMN3ROW1: usize = 13;
    pub const COLUMN3ROW2: usize = 14;
    pub const COLUMN3ROW3: usize = 15;

    /// Arguments in row-major order, like Cesium's constructor.
    #[allow(clippy::too_many_arguments)]
    pub const fn new(
        c0r0: f64,
        c1r0: f64,
        c2r0: f64,
        c3r0: f64,
        c0r1: f64,
        c1r1: f64,
        c2r1: f64,
        c3r1: f64,
        c0r2: f64,
        c1r2: f64,
        c2r2: f64,
        c3r2: f64,
        c0r3: f64,
        c1r3: f64,
        c2r3: f64,
        c3r3: f64,
    ) -> Self {
        Self([
            c0r0, c0r1, c0r2, c0r3, c1r0, c1r1, c1r2, c1r3, c2r0, c2r1, c2r2, c2r3, c3r0, c3r1, c3r2, c3r3,
        ])
    }

    pub fn from_rotation_translation(rotation: &Matrix3, translation: Cartesian3) -> Self {
        let r = &rotation.0;
        Self([
            r[0],
            r[1],
            r[2],
            0.0,
            r[3],
            r[4],
            r[5],
            0.0,
            r[6],
            r[7],
            r[8],
            0.0,
            translation.x,
            translation.y,
            translation.z,
            1.0,
        ])
    }

    pub fn from_translation_quaternion_rotation_scale(
        translation: Cartesian3,
        rotation: &Quaternion,
        scale: Cartesian3,
    ) -> Self {
        let q = rotation;
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
        Self([
            m00 * scale.x,
            m10 * scale.x,
            m20 * scale.x,
            0.0,
            m01 * scale.y,
            m11 * scale.y,
            m21 * scale.y,
            0.0,
            m02 * scale.z,
            m12 * scale.z,
            m22 * scale.z,
            0.0,
            translation.x,
            translation.y,
            translation.z,
            1.0,
        ])
    }

    pub fn from_translation_rotation_scale(trs: &TranslationRotationScale) -> Self {
        Self::from_translation_quaternion_rotation_scale(trs.translation, &trs.rotation, trs.scale)
    }

    pub fn from_translation(translation: Cartesian3) -> Self {
        Self::from_rotation_translation(&Matrix3::IDENTITY, translation)
    }

    pub fn from_scale(scale: Cartesian3) -> Self {
        Self::new(
            scale.x, 0.0, 0.0, 0.0, 0.0, scale.y, 0.0, 0.0, 0.0, 0.0, scale.z, 0.0, 0.0, 0.0, 0.0, 1.0,
        )
    }

    pub fn from_uniform_scale(scale: f64) -> Self {
        Self::from_scale(Cartesian3::new(scale, scale, scale))
    }

    pub fn from_rotation(rotation: &Matrix3) -> Self {
        Self::from_rotation_translation(rotation, Cartesian3::ZERO)
    }

    /// View matrix for a camera at `position` looking along `direction`.
    pub fn from_camera(position: Cartesian3, direction: Cartesian3, up: Cartesian3) -> Self {
        let f = direction.normalize();
        let r = f.cross(up).normalize();
        let u = r.cross(f).normalize();
        let t0 = r.x * -position.x + r.y * -position.y + r.z * -position.z;
        let t1 = u.x * -position.x + u.y * -position.y + u.z * -position.z;
        let t2 = f.x * position.x + f.y * position.y + f.z * position.z;
        Self::new(r.x, r.y, r.z, t0, u.x, u.y, u.z, t1, -f.x, -f.y, -f.z, t2, 0.0, 0.0, 0.0, 1.0)
    }

    pub fn compute_perspective_field_of_view(fov_y: f64, aspect_ratio: f64, near: f64, far: f64) -> Self {
        let bottom = (fov_y * 0.5).tan();
        let column1_row1 = 1.0 / bottom;
        let column0_row0 = column1_row1 / aspect_ratio;
        let column2_row2 = (far + near) / (near - far);
        let column3_row2 = (2.0 * far * near) / (near - far);
        Self([
            column0_row0,
            0.0,
            0.0,
            0.0,
            0.0,
            column1_row1,
            0.0,
            0.0,
            0.0,
            0.0,
            column2_row2,
            -1.0,
            0.0,
            0.0,
            column3_row2,
            0.0,
        ])
    }

    pub fn compute_orthographic_off_center(left: f64, right: f64, bottom: f64, top: f64, near: f64, far: f64) -> Self {
        let mut a = 1.0 / (right - left);
        let mut b = 1.0 / (top - bottom);
        let mut c = 1.0 / (far - near);
        let tx = -(right + left) * a;
        let ty = -(top + bottom) * b;
        let tz = -(far + near) * c;
        a *= 2.0;
        b *= 2.0;
        c *= -2.0;
        Self([a, 0.0, 0.0, 0.0, 0.0, b, 0.0, 0.0, 0.0, 0.0, c, 0.0, tx, ty, tz, 1.0])
    }

    pub fn compute_perspective_off_center(left: f64, right: f64, bottom: f64, top: f64, near: f64, far: f64) -> Self {
        let column0_row0 = (2.0 * near) / (right - left);
        let column1_row1 = (2.0 * near) / (top - bottom);
        let column2_row0 = (right + left) / (right - left);
        let column2_row1 = (top + bottom) / (top - bottom);
        let column2_row2 = -(far + near) / (far - near);
        let column3_row2 = (-2.0 * far * near) / (far - near);
        Self([
            column0_row0,
            0.0,
            0.0,
            0.0,
            0.0,
            column1_row1,
            0.0,
            0.0,
            column2_row0,
            column2_row1,
            column2_row2,
            -1.0,
            0.0,
            0.0,
            column3_row2,
            0.0,
        ])
    }

    pub fn compute_infinite_perspective_off_center(left: f64, right: f64, bottom: f64, top: f64, near: f64) -> Self {
        let column0_row0 = (2.0 * near) / (right - left);
        let column1_row1 = (2.0 * near) / (top - bottom);
        let column2_row0 = (right + left) / (right - left);
        let column2_row1 = (top + bottom) / (top - bottom);
        let column3_row2 = -2.0 * near;
        Self([
            column0_row0,
            0.0,
            0.0,
            0.0,
            0.0,
            column1_row1,
            0.0,
            0.0,
            column2_row0,
            column2_row1,
            -1.0,
            -1.0,
            0.0,
            0.0,
            column3_row2,
            0.0,
        ])
    }

    /// Maps normalized device coordinates to window coordinates. Cesium's
    /// defaults are a zero viewport and a depth range of `[0, 1]`.
    pub fn compute_viewport_transformation(viewport: &BoundingRectangle, near_depth_range: f64, far_depth_range: f64) -> Self {
        let half_width = viewport.width * 0.5;
        let half_height = viewport.height * 0.5;
        let half_depth = (far_depth_range - near_depth_range) * 0.5;
        Self([
            half_width,
            0.0,
            0.0,
            0.0,
            0.0,
            half_height,
            0.0,
            0.0,
            0.0,
            0.0,
            half_depth,
            0.0,
            viewport.x + half_width,
            viewport.y + half_height,
            near_depth_range + half_depth,
            1.0,
        ])
    }

    pub fn compute_view(position: Cartesian3, direction: Cartesian3, up: Cartesian3, right: Cartesian3) -> Self {
        Self([
            right.x,
            up.x,
            -direction.x,
            0.0,
            right.y,
            up.y,
            -direction.y,
            0.0,
            right.z,
            up.z,
            -direction.z,
            0.0,
            -right.dot(position),
            -up.dot(position),
            direction.dot(position),
            1.0,
        ])
    }

    pub fn set_translation(&self, translation: Cartesian3) -> Self {
        let mut m = *self;
        m.0[12] = translation.x;
        m.0[13] = translation.y;
        m.0[14] = translation.z;
        m
    }

    /// Length of each of the first three columns.
    pub fn get_scale(&self) -> Cartesian3 {
        let m = &self.0;
        Cartesian3::new(
            Cartesian3::new(m[0], m[1], m[2]).magnitude(),
            Cartesian3::new(m[4], m[5], m[6]).magnitude(),
            Cartesian3::new(m[8], m[9], m[10]).magnitude(),
        )
    }

    pub fn get_maximum_scale(&self) -> f64 {
        self.get_scale().maximum_component()
    }

    pub fn set_scale(&self, scale: Cartesian3) -> Self {
        let existing = self.get_scale();
        self.scale_upper_columns(scale.x / existing.x, scale.y / existing.y, scale.z / existing.z)
    }

    pub fn set_uniform_scale(&self, scale: f64) -> Self {
        let existing = self.get_scale();
        self.scale_upper_columns(scale / existing.x, scale / existing.y, scale / existing.z)
    }

    fn scale_upper_columns(&self, x: f64, y: f64, z: f64) -> Self {
        let mut m = *self;
        for (col, ratio) in [x, y, z].into_iter().enumerate() {
            for row in 0..3 {
                m.0[col * 4 + row] = self.0[col * 4 + row] * ratio;
            }
        }
        m
    }

    /// Replaces the upper 3×3 with `rotation`, keeping this matrix's scale.
    pub fn set_rotation(&self, rotation: &Matrix3) -> Self {
        let scale = self.get_scale();
        let r = &rotation.0;
        let mut m = *self;
        for (col, s) in [scale.x, scale.y, scale.z].into_iter().enumerate() {
            for row in 0..3 {
                m.0[col * 4 + row] = r[col * 3 + row] * s;
            }
        }
        m
    }

    /// The upper 3×3 with each column divided by its length.
    pub fn get_rotation(&self) -> Matrix3 {
        let scale = self.get_scale();
        let m = &self.0;
        Matrix3([
            m[0] / scale.x,
            m[1] / scale.x,
            m[2] / scale.x,
            m[4] / scale.y,
            m[5] / scale.y,
            m[6] / scale.y,
            m[8] / scale.z,
            m[9] / scale.z,
            m[10] / scale.z,
        ])
    }

    /// Product of two affine transforms; assumes both bottom rows are `[0, 0, 0, 1]`.
    pub fn multiply_transformation(&self, right: &Self) -> Self {
        let l = &self.0;
        let r = &right.0;
        Self([
            l[0] * r[0] + l[4] * r[1] + l[8] * r[2],
            l[1] * r[0] + l[5] * r[1] + l[9] * r[2],
            l[2] * r[0] + l[6] * r[1] + l[10] * r[2],
            0.0,
            l[0] * r[4] + l[4] * r[5] + l[8] * r[6],
            l[1] * r[4] + l[5] * r[5] + l[9] * r[6],
            l[2] * r[4] + l[6] * r[5] + l[10] * r[6],
            0.0,
            l[0] * r[8] + l[4] * r[9] + l[8] * r[10],
            l[1] * r[8] + l[5] * r[9] + l[9] * r[10],
            l[2] * r[8] + l[6] * r[9] + l[10] * r[10],
            0.0,
            l[0] * r[12] + l[4] * r[13] + l[8] * r[14] + l[12],
            l[1] * r[12] + l[5] * r[13] + l[9] * r[14] + l[13],
            l[2] * r[12] + l[6] * r[13] + l[10] * r[14] + l[14],
            1.0,
        ])
    }

    /// `self * fromRotation(rotation)`, keeping this matrix's last column.
    pub fn multiply_by_matrix3(&self, rotation: &Matrix3) -> Self {
        let l = &self.0;
        let r = &rotation.0;
        Self([
            l[0] * r[0] + l[4] * r[1] + l[8] * r[2],
            l[1] * r[0] + l[5] * r[1] + l[9] * r[2],
            l[2] * r[0] + l[6] * r[1] + l[10] * r[2],
            0.0,
            l[0] * r[3] + l[4] * r[4] + l[8] * r[5],
            l[1] * r[3] + l[5] * r[4] + l[9] * r[5],
            l[2] * r[3] + l[6] * r[4] + l[10] * r[5],
            0.0,
            l[0] * r[6] + l[4] * r[7] + l[8] * r[8],
            l[1] * r[6] + l[5] * r[7] + l[9] * r[8],
            l[2] * r[6] + l[6] * r[7] + l[10] * r[8],
            0.0,
            l[12],
            l[13],
            l[14],
            l[15],
        ])
    }

    /// `self * fromTranslation(translation)`.
    pub fn multiply_by_translation(&self, t: Cartesian3) -> Self {
        let m = &self.0;
        let mut out = *self;
        out.0[12] = t.x * m[0] + t.y * m[4] + t.z * m[8] + m[12];
        out.0[13] = t.x * m[1] + t.y * m[5] + t.z * m[9] + m[13];
        out.0[14] = t.x * m[2] + t.y * m[6] + t.z * m[10] + m[14];
        out
    }

    /// `self * fromScale(scale)`.
    pub fn multiply_by_scale(&self, scale: Cartesian3) -> Self {
        if scale.x == 1.0 && scale.y == 1.0 && scale.z == 1.0 {
            return *self;
        }
        let m = &self.0;
        let mut out = *self;
        for (col, s) in [scale.x, scale.y, scale.z].into_iter().enumerate() {
            for row in 0..3 {
                out.0[col * 4 + row] = s * m[col * 4 + row];
            }
        }
        out
    }

    pub fn multiply_by_uniform_scale(&self, scale: f64) -> Self {
        self.scale_upper_columns(scale, scale, scale)
    }

    pub fn multiply_by_vector(&self, v: Cartesian4) -> Cartesian4 {
        let m = &self.0;
        Cartesian4::new(
            m[0] * v.x + m[4] * v.y + m[8] * v.z + m[12] * v.w,
            m[1] * v.x + m[5] * v.y + m[9] * v.z + m[13] * v.w,
            m[2] * v.x + m[6] * v.y + m[10] * v.z + m[14] * v.w,
            m[3] * v.x + m[7] * v.y + m[11] * v.z + m[15] * v.w,
        )
    }

    /// Transforms a direction (`w = 0`).
    pub fn multiply_by_point_as_vector(&self, v: Cartesian3) -> Cartesian3 {
        let m = &self.0;
        Cartesian3::new(
            m[0] * v.x + m[4] * v.y + m[8] * v.z,
            m[1] * v.x + m[5] * v.y + m[9] * v.z,
            m[2] * v.x + m[6] * v.y + m[10] * v.z,
        )
    }

    /// Transforms a point (`w = 1`) without the perspective divide.
    pub fn multiply_by_point(&self, v: Cartesian3) -> Cartesian3 {
        let m = &self.0;
        Cartesian3::new(
            m[0] * v.x + m[4] * v.y + m[8] * v.z + m[12],
            m[1] * v.x + m[5] * v.y + m[9] * v.z + m[13],
            m[2] * v.x + m[6] * v.y + m[10] * v.z + m[14],
        )
    }

    pub fn get_translation(&self) -> Cartesian3 {
        Cartesian3::new(self.0[12], self.0[13], self.0[14])
    }

    pub fn get_matrix3(&self) -> Matrix3 {
        let m = &self.0;
        Matrix3([m[0], m[1], m[2], m[4], m[5], m[6], m[8], m[9], m[10]])
    }

    /// General inverse by Cramer's rule. A pure translation with a zero
    /// upper 3×3 inverts to the negated translation, as in Cesium.
    pub fn inverse(&self) -> Result<Self, RuntimeError> {
        let m = &self.0;
        let (src0, src1, src2, src3) = (m[0], m[4], m[8], m[12]);
        let (src4, src5, src6, src7) = (m[1], m[5], m[9], m[13]);
        let (src8, src9, src10, src11) = (m[2], m[6], m[10], m[14]);
        let (src12, src13, src14, src15) = (m[3], m[7], m[11], m[15]);

        let mut tmp0 = src10 * src15;
        let mut tmp1 = src11 * src14;
        let mut tmp2 = src9 * src15;
        let mut tmp3 = src11 * src13;
        let mut tmp4 = src9 * src14;
        let mut tmp5 = src10 * src13;
        let mut tmp6 = src8 * src15;
        let mut tmp7 = src11 * src12;
        let mut tmp8 = src8 * src14;
        let mut tmp9 = src10 * src12;
        let mut tmp10 = src8 * src13;
        let mut tmp11 = src9 * src12;

        let dst0 = tmp0 * src5 + tmp3 * src6 + tmp4 * src7 - (tmp1 * src5 + tmp2 * src6 + tmp5 * src7);
        let dst1 = tmp1 * src4 + tmp6 * src6 + tmp9 * src7 - (tmp0 * src4 + tmp7 * src6 + tmp8 * src7);
        let dst2 = tmp2 * src4 + tmp7 * src5 + tmp10 * src7 - (tmp3 * src4 + tmp6 * src5 + tmp11 * src7);
        let dst3 = tmp5 * src4 + tmp8 * src5 + tmp11 * src6 - (tmp4 * src4 + tmp9 * src5 + tmp10 * src6);
        let dst4 = tmp1 * src1 + tmp2 * src2 + tmp5 * src3 - (tmp0 * src1 + tmp3 * src2 + tmp4 * src3);
        let dst5 = tmp0 * src0 + tmp7 * src2 + tmp8 * src3 - (tmp1 * src0 + tmp6 * src2 + tmp9 * src3);
        let dst6 = tmp3 * src0 + tmp6 * src1 + tmp11 * src3 - (tmp2 * src0 + tmp7 * src1 + tmp10 * src3);
        let dst7 = tmp4 * src0 + tmp9 * src1 + tmp10 * src2 - (tmp5 * src0 + tmp8 * src1 + tmp11 * src2);

        tmp0 = src2 * src7;
        tmp1 = src3 * src6;
        tmp2 = src1 * src7;
        tmp3 = src3 * src5;
        tmp4 = src1 * src6;
        tmp5 = src2 * src5;
        tmp6 = src0 * src7;
        tmp7 = src3 * src4;
        tmp8 = src0 * src6;
        tmp9 = src2 * src4;
        tmp10 = src0 * src5;
        tmp11 = src1 * src4;

        let dst8 = tmp0 * src13 + tmp3 * src14 + tmp4 * src15 - (tmp1 * src13 + tmp2 * src14 + tmp5 * src15);
        let dst9 = tmp1 * src12 + tmp6 * src14 + tmp9 * src15 - (tmp0 * src12 + tmp7 * src14 + tmp8 * src15);
        let dst10 = tmp2 * src12 + tmp7 * src13 + tmp10 * src15 - (tmp3 * src12 + tmp6 * src13 + tmp11 * src15);
        let dst11 = tmp5 * src12 + tmp8 * src13 + tmp11 * src14 - (tmp4 * src12 + tmp9 * src13 + tmp10 * src14);
        let dst12 = tmp2 * src10 + tmp5 * src11 + tmp1 * src9 - (tmp4 * src11 + tmp0 * src9 + tmp3 * src10);
        let dst13 = tmp8 * src11 + tmp0 * src8 + tmp7 * src10 - (tmp6 * src10 + tmp9 * src11 + tmp1 * src8);
        let dst14 = tmp6 * src9 + tmp11 * src11 + tmp3 * src8 - (tmp10 * src11 + tmp2 * src8 + tmp7 * src9);
        let dst15 = tmp10 * src10 + tmp4 * src8 + tmp9 * src9 - (tmp8 * src9 + tmp11 * src10 + tmp5 * src8);

        let det = src0 * dst0 + src1 * dst1 + src2 * dst2 + src3 * dst3;
        if det.abs() < EPSILON21 {
            if self.get_matrix3().equals_epsilon(&Matrix3::ZERO, EPSILON7)
                && self.get_row(3).equals(Cartesian4::new(0.0, 0.0, 0.0, 1.0))
            {
                let mut out = Self::ZERO;
                out.0[12] = -m[12];
                out.0[13] = -m[13];
                out.0[14] = -m[14];
                out.0[15] = 1.0;
                return Ok(out);
            }
            return Err(RuntimeError::new("matrix is not invertible because its determinate is zero."));
        }
        let det = 1.0 / det;
        Ok(Self([
            dst0 * det,
            dst1 * det,
            dst2 * det,
            dst3 * det,
            dst4 * det,
            dst5 * det,
            dst6 * det,
            dst7 * det,
            dst8 * det,
            dst9 * det,
            dst10 * det,
            dst11 * det,
            dst12 * det,
            dst13 * det,
            dst14 * det,
            dst15 * det,
        ]))
    }

    /// Fast inverse of a rotation-plus-translation matrix.
    pub fn inverse_transformation(&self) -> Self {
        let m = &self.0;
        let (vx, vy, vz) = (m[12], m[13], m[14]);
        let x = -m[0] * vx - m[1] * vy - m[2] * vz;
        let y = -m[4] * vx - m[5] * vy - m[6] * vz;
        let z = -m[8] * vx - m[9] * vy - m[10] * vz;
        Self([
            m[0], m[4], m[8], 0.0, m[1], m[5], m[9], 0.0, m[2], m[6], m[10], 0.0, x, y, z, 1.0,
        ])
    }

    pub fn inverse_transpose(&self) -> Result<Self, RuntimeError> {
        self.transpose().inverse()
    }
}
