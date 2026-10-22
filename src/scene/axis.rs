//! CesiumJS `Axis`: the up axis of a model, and the rotations between them.

use crate::core::Matrix4;

numeric_enum!(Axis: i32 {
    X = 0 => "X",
    Y = 1 => "Y",
    Z = 2 => "Z",
});

/// `Matrix4.fromRotationTranslation` of a column-major 3x3 rotation.
const fn rotation(m: [f64; 9]) -> Matrix4 {
    Matrix4([m[0], m[1], m[2], 0.0, m[3], m[4], m[5], 0.0, m[6], m[7], m[8], 0.0, 0.0, 0.0, 0.0, 1.0])
}

impl Axis {
    pub const Y_UP_TO_Z_UP: Matrix4 = rotation([1.0, 0.0, 0.0, 0.0, 0.0, 1.0, 0.0, -1.0, 0.0]);
    pub const Z_UP_TO_Y_UP: Matrix4 = rotation([1.0, 0.0, 0.0, 0.0, 0.0, -1.0, 0.0, 1.0, 0.0]);
    pub const X_UP_TO_Z_UP: Matrix4 = rotation([0.0, 0.0, 1.0, 0.0, 1.0, 0.0, -1.0, 0.0, 0.0]);
    pub const Z_UP_TO_X_UP: Matrix4 = rotation([0.0, 0.0, -1.0, 0.0, 1.0, 0.0, 1.0, 0.0, 0.0]);
    pub const X_UP_TO_Y_UP: Matrix4 = rotation([0.0, 1.0, 0.0, -1.0, 0.0, 0.0, 0.0, 0.0, 1.0]);
    pub const Y_UP_TO_X_UP: Matrix4 = rotation([0.0, -1.0, 0.0, 1.0, 0.0, 0.0, 0.0, 0.0, 1.0]);

    /// The axis named `"X"`, `"Y"` or `"Z"`. Cesium indexes the whole `Axis`
    /// object, so it also returns the matrices for their names; use the
    /// constants for those.
    pub fn from_name(name: &str) -> Option<Self> {
        Self::ALL.iter().copied().find(|axis| axis.name() == name)
    }
}
