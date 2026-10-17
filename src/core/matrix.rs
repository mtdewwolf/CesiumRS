//! Column-major API shared by `Matrix2`, `Matrix3`, and `Matrix4`.

/// Generates the parts of the square-matrix API that are identical across
/// sizes. `$n` is the dimension, `$len` the element count, `$col` the column
/// vector type. Products sum terms left to right, as Cesium does, so results
/// match bit for bit.
macro_rules! matrix_common {
    ($name:ident, $n:expr, $len:expr, $col:ident) => {
        impl $name {
            pub const PACKED_LENGTH: usize = $len;

            /// Wraps column-major storage.
            pub const fn from_column_major_array(values: [f64; $len]) -> Self {
                Self(values)
            }

            pub fn from_row_major_array(values: [f64; $len]) -> Self {
                let mut m = [0.0; $len];
                for row in 0..$n {
                    for col in 0..$n {
                        m[col * $n + row] = values[row * $n + col];
                    }
                }
                Self(m)
            }

            pub fn pack(&self, array: &mut [f64], starting_index: usize) {
                array[starting_index..starting_index + $len].copy_from_slice(&self.0);
            }

            pub fn unpack(array: &[f64], starting_index: usize) -> Self {
                let mut m = [0.0; $len];
                m.copy_from_slice(&array[starting_index..starting_index + $len]);
                Self(m)
            }

            pub fn from_array(array: &[f64], starting_index: usize) -> Self {
                Self::unpack(array, starting_index)
            }

            pub fn pack_array(values: &[Self]) -> Vec<f64> {
                values.iter().flat_map(|m| m.0).collect()
            }

            pub fn unpack_array(array: &[f64]) -> Vec<Self> {
                assert!(array.len() % $len == 0, "array length must be a multiple of {}.", $len);
                (0..array.len() / $len).map(|i| Self::unpack(array, i * $len)).collect()
            }

            pub fn to_array(&self) -> [f64; $len] {
                self.0
            }

            pub const fn get_element_index(column: usize, row: usize) -> usize {
                column * $n + row
            }

            pub fn get_column(&self, index: usize) -> $col {
                $col::unpack(&self.0, index * $n)
            }

            pub fn set_column(&self, index: usize, value: $col) -> Self {
                let mut m = *self;
                value.pack(&mut m.0, index * $n);
                m
            }

            pub fn get_row(&self, index: usize) -> $col {
                let mut v = [0.0; $n];
                for (col, item) in v.iter_mut().enumerate() {
                    *item = self.0[col * $n + index];
                }
                $col::unpack(&v, 0)
            }

            pub fn set_row(&self, index: usize, value: $col) -> Self {
                let mut m = *self;
                let v = value.to_array();
                for (col, item) in v.iter().enumerate() {
                    m.0[col * $n + index] = *item;
                }
                m
            }

            pub fn multiply(&self, right: &Self) -> Self {
                let mut m = [0.0; $len];
                for col in 0..$n {
                    for row in 0..$n {
                        let mut sum = self.0[row] * right.0[col * $n];
                        for k in 1..$n {
                            sum += self.0[k * $n + row] * right.0[col * $n + k];
                        }
                        m[col * $n + row] = sum;
                    }
                }
                Self(m)
            }

            pub fn add(&self, right: &Self) -> Self {
                let mut m = self.0;
                for (a, b) in m.iter_mut().zip(right.0) {
                    *a += b;
                }
                Self(m)
            }

            pub fn subtract(&self, right: &Self) -> Self {
                let mut m = self.0;
                for (a, b) in m.iter_mut().zip(right.0) {
                    *a -= b;
                }
                Self(m)
            }

            pub fn multiply_by_scalar(&self, scalar: f64) -> Self {
                Self(self.0.map(|v| v * scalar))
            }

            pub fn negate(&self) -> Self {
                Self(self.0.map(|v| -v))
            }

            pub fn abs(&self) -> Self {
                Self(self.0.map(f64::abs))
            }

            pub fn transpose(&self) -> Self {
                let mut m = [0.0; $len];
                for col in 0..$n {
                    for row in 0..$n {
                        m[row * $n + col] = self.0[col * $n + row];
                    }
                }
                Self(m)
            }

            /// Exact element equality.
            pub fn equals(&self, right: &Self) -> bool {
                self.0 == right.0
            }

            /// Every element within `epsilon` (absolute), like Cesium.
            pub fn equals_epsilon(&self, right: &Self, epsilon: f64) -> bool {
                self.0.iter().zip(right.0.iter()).all(|(a, b)| (a - b).abs() <= epsilon)
            }

            pub fn equals_array(&self, array: &[f64], offset: usize) -> bool {
                self.0[..] == array[offset..offset + $len]
            }
        }

        impl std::ops::Index<usize> for $name {
            type Output = f64;
            fn index(&self, i: usize) -> &f64 {
                &self.0[i]
            }
        }

        impl std::ops::IndexMut<usize> for $name {
            fn index_mut(&mut self, i: usize) -> &mut f64 {
                &mut self.0[i]
            }
        }

        impl std::ops::Mul for $name {
            type Output = Self;
            fn mul(self, right: Self) -> Self {
                self.multiply(&right)
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                for row in 0..$n {
                    if row > 0 {
                        writeln!(f)?;
                    }
                    let cells: Vec<String> = (0..$n).map(|col| self.0[col * $n + row].to_string()).collect();
                    write!(f, "({})", cells.join(", "))?;
                }
                Ok(())
            }
        }
    };
}

/// Scale and rotation helpers for matrices whose columns are the full basis
/// (`Matrix2`, `Matrix3`). `Matrix4` has its own versions over its upper 3×3.
macro_rules! matrix_scale_rotation {
    ($name:ident, $n:expr, $col:ident) => {
        impl $name {
            pub fn from_scale(scale: $col) -> Self {
                let s = scale.to_array();
                let mut m = Self::ZERO;
                for i in 0..$n {
                    m.0[i * $n + i] = s[i];
                }
                m
            }

            pub fn from_uniform_scale(scale: f64) -> Self {
                let mut m = Self::ZERO;
                for i in 0..$n {
                    m.0[i * $n + i] = scale;
                }
                m
            }

            /// Length of each column.
            pub fn get_scale(&self) -> $col {
                let mut s = [0.0; $n];
                for (i, item) in s.iter_mut().enumerate() {
                    *item = self.get_column(i).magnitude();
                }
                $col::unpack(&s, 0)
            }

            pub fn get_maximum_scale(&self) -> f64 {
                self.get_scale().maximum_component()
            }

            pub fn set_scale(&self, scale: $col) -> Self {
                let existing = self.get_scale().to_array();
                let wanted = scale.to_array();
                let mut m = *self;
                for col in 0..$n {
                    let ratio = wanted[col] / existing[col];
                    for row in 0..$n {
                        m.0[col * $n + row] = self.0[col * $n + row] * ratio;
                    }
                }
                m
            }

            pub fn set_uniform_scale(&self, scale: f64) -> Self {
                let existing = self.get_scale().to_array();
                let mut m = *self;
                for col in 0..$n {
                    let ratio = scale / existing[col];
                    for row in 0..$n {
                        m.0[col * $n + row] = self.0[col * $n + row] * ratio;
                    }
                }
                m
            }

            /// `rotation` with this matrix's column scales applied.
            pub fn set_rotation(&self, rotation: &Self) -> Self {
                let scale = self.get_scale().to_array();
                let mut m = *rotation;
                for col in 0..$n {
                    for row in 0..$n {
                        m.0[col * $n + row] = rotation.0[col * $n + row] * scale[col];
                    }
                }
                m
            }

            /// This matrix with each column divided by its length.
            pub fn get_rotation(&self) -> Self {
                let scale = self.get_scale().to_array();
                let mut m = *self;
                for col in 0..$n {
                    for row in 0..$n {
                        m.0[col * $n + row] = self.0[col * $n + row] / scale[col];
                    }
                }
                m
            }

            pub fn multiply_by_vector(&self, v: $col) -> $col {
                let a = v.to_array();
                let mut out = [0.0; $n];
                for (row, item) in out.iter_mut().enumerate() {
                    let mut sum = self.0[row] * a[0];
                    for k in 1..$n {
                        sum += self.0[k * $n + row] * a[k];
                    }
                    *item = sum;
                }
                $col::unpack(&out, 0)
            }

            /// Scales column `i` by component `i` of `scale` (`self * fromScale(scale)`).
            pub fn multiply_by_scale(&self, scale: $col) -> Self {
                let s = scale.to_array();
                let mut m = *self;
                for col in 0..$n {
                    for row in 0..$n {
                        m.0[col * $n + row] = self.0[col * $n + row] * s[col];
                    }
                }
                m
            }

            pub fn multiply_by_uniform_scale(&self, scale: f64) -> Self {
                Self(self.0.map(|v| v * scale))
            }
        }
    };
}

pub(crate) use matrix_common;
pub(crate) use matrix_scale_rotation;
