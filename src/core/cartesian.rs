//! Component-wise API shared by `Cartesian2`, `Cartesian3`, and `Cartesian4`.

/// Generates the parts of the Cartesian API that are identical across sizes:
/// construction, packing, component-wise arithmetic, and comparisons.
macro_rules! cartesian_common {
    ($name:ident, $len:expr, $($f:ident),+) => {
        impl $name {
            pub const PACKED_LENGTH: usize = $len;

            pub const fn new($($f: f64),+) -> Self {
                Self { $($f),+ }
            }

            pub const fn from_elements($($f: f64),+) -> Self {
                Self { $($f),+ }
            }

            /// Writes the components into `array` starting at `starting_index`.
            pub fn pack(&self, array: &mut [f64], starting_index: usize) {
                let mut i = starting_index;
                $( array[i] = self.$f; i += 1; )+
                let _ = i;
            }

            pub fn unpack(array: &[f64], starting_index: usize) -> Self {
                let mut i = starting_index;
                $( let $f = array[i]; i += 1; )+
                let _ = i;
                Self { $($f),+ }
            }

            /// Cesium `fromArray`, an alias of `unpack`.
            pub fn from_array(array: &[f64], starting_index: usize) -> Self {
                Self::unpack(array, starting_index)
            }

            pub fn pack_array(values: &[Self]) -> Vec<f64> {
                let mut out = vec![0.0; values.len() * $len];
                for (i, v) in values.iter().enumerate() {
                    v.pack(&mut out, i * $len);
                }
                out
            }

            /// Panics unless `array.len()` is a multiple of the component count.
            pub fn unpack_array(array: &[f64]) -> Vec<Self> {
                assert!(array.len() % $len == 0, "array length must be a multiple of {}.", $len);
                (0..array.len() / $len).map(|i| Self::unpack(array, i * $len)).collect()
            }

            pub fn to_array(&self) -> [f64; $len] {
                [$(self.$f),+]
            }

            pub fn maximum_component(&self) -> f64 {
                let mut m = f64::NEG_INFINITY;
                $( m = $crate::core::math::js_max(m, self.$f); )+
                m
            }

            pub fn minimum_component(&self) -> f64 {
                let mut m = f64::INFINITY;
                $( m = $crate::core::math::js_min(m, self.$f); )+
                m
            }

            pub fn minimum_by_component(&self, other: Self) -> Self {
                Self { $($f: $crate::core::math::js_min(self.$f, other.$f)),+ }
            }

            pub fn maximum_by_component(&self, other: Self) -> Self {
                Self { $($f: $crate::core::math::js_max(self.$f, other.$f)),+ }
            }

            /// Clamps each component between the matching components of `min` and `max`.
            pub fn clamp(&self, min: Self, max: Self) -> Self {
                Self { $($f: $crate::core::math::clamp(self.$f, min.$f, max.$f)),+ }
            }

            pub fn magnitude_squared(&self) -> f64 {
                0.0 $(+ self.$f * self.$f)+
            }

            pub fn magnitude(&self) -> f64 {
                self.magnitude_squared().sqrt()
            }

            pub fn distance(&self, other: Self) -> f64 {
                (*self - other).magnitude()
            }

            pub fn distance_squared(&self, other: Self) -> f64 {
                (*self - other).magnitude_squared()
            }

            /// Unit vector in the same direction. A zero vector yields NaN
            /// components, as in Cesium's release build.
            pub fn normalize(&self) -> Self {
                let m = self.magnitude();
                Self { $($f: self.$f / m),+ }
            }

            /// `normalize`, or `None` when the result would contain NaN.
            pub fn try_normalize(&self) -> Option<Self> {
                let n = self.normalize();
                if false $(|| n.$f.is_nan())+ { None } else { Some(n) }
            }

            pub fn dot(&self, other: Self) -> f64 {
                0.0 $(+ self.$f * other.$f)+
            }

            pub fn multiply_components(&self, other: Self) -> Self {
                Self { $($f: self.$f * other.$f),+ }
            }

            pub fn divide_components(&self, other: Self) -> Self {
                Self { $($f: self.$f / other.$f),+ }
            }

            pub fn add(&self, other: Self) -> Self {
                Self { $($f: self.$f + other.$f),+ }
            }

            pub fn subtract(&self, other: Self) -> Self {
                Self { $($f: self.$f - other.$f),+ }
            }

            pub fn multiply_by_scalar(&self, scalar: f64) -> Self {
                Self { $($f: self.$f * scalar),+ }
            }

            pub fn divide_by_scalar(&self, scalar: f64) -> Self {
                Self { $($f: self.$f / scalar),+ }
            }

            pub fn negate(&self) -> Self {
                Self { $($f: -self.$f),+ }
            }

            pub fn abs(&self) -> Self {
                Self { $($f: self.$f.abs()),+ }
            }

            /// `start * (1 - t) + end * t`, evaluated in Cesium's order.
            pub fn lerp(&self, end: Self, t: f64) -> Self {
                let scaled_end = end.multiply_by_scalar(t);
                let scaled_start = self.multiply_by_scalar(1.0 - t);
                scaled_end.add(scaled_start)
            }

            /// Exact component equality, like Cesium `equals`.
            pub fn equals(&self, other: Self) -> bool {
                true $(&& self.$f == other.$f)+
            }

            pub fn equals_array(&self, array: &[f64], offset: usize) -> bool {
                let mut i = offset;
                $( if self.$f != array[i] { return false; } i += 1; )+
                let _ = i;
                true
            }

            /// Cesium `equalsEpsilon` with `absoluteEpsilon` defaulting to `relative_epsilon`.
            pub fn equals_epsilon(&self, other: Self, relative_epsilon: f64) -> bool {
                self.equals_epsilon_with(other, relative_epsilon, relative_epsilon)
            }

            pub fn equals_epsilon_with(&self, other: Self, relative_epsilon: f64, absolute_epsilon: f64) -> bool {
                true $(&& $crate::core::math::equals_epsilon(self.$f, other.$f, relative_epsilon, absolute_epsilon))+
            }
        }

        impl std::ops::Add for $name {
            type Output = Self;
            fn add(self, o: Self) -> Self {
                Self { $($f: self.$f + o.$f),+ }
            }
        }

        impl std::ops::Sub for $name {
            type Output = Self;
            fn sub(self, o: Self) -> Self {
                Self { $($f: self.$f - o.$f),+ }
            }
        }

        impl std::ops::Mul<f64> for $name {
            type Output = Self;
            fn mul(self, s: f64) -> Self {
                Self { $($f: self.$f * s),+ }
            }
        }

        impl std::ops::Div<f64> for $name {
            type Output = Self;
            fn div(self, s: f64) -> Self {
                Self { $($f: self.$f / s),+ }
            }
        }

        impl std::ops::Neg for $name {
            type Output = Self;
            fn neg(self) -> Self {
                Self { $($f: -self.$f),+ }
            }
        }

        impl std::ops::AddAssign for $name {
            fn add_assign(&mut self, o: Self) {
                $( self.$f += o.$f; )+
            }
        }

        impl std::ops::SubAssign for $name {
            fn sub_assign(&mut self, o: Self) {
                $( self.$f -= o.$f; )+
            }
        }

        impl std::ops::MulAssign<f64> for $name {
            fn mul_assign(&mut self, s: f64) {
                $( self.$f *= s; )+
            }
        }

        impl std::fmt::Display for $name {
            fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
                let parts = [$(self.$f.to_string()),+];
                write!(f, "({})", parts.join(", "))
            }
        }

        impl From<[f64; $len]> for $name {
            fn from(a: [f64; $len]) -> Self {
                Self::unpack(&a, 0)
            }
        }

        impl From<$name> for [f64; $len] {
            fn from(v: $name) -> Self {
                v.to_array()
            }
        }
    };
}

pub(crate) use cartesian_common;
