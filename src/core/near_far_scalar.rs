//! CesiumJS `NearFarScalar`: a value at a near and a far distance, for
//! scaling by eye distance.

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct NearFarScalar {
    pub near: f64,
    pub near_value: f64,
    pub far: f64,
    pub far_value: f64,
}

impl Default for NearFarScalar {
    fn default() -> Self {
        Self { near: 0.0, near_value: 0.0, far: 1.0, far_value: 0.0 }
    }
}

impl NearFarScalar {
    pub const PACKED_LENGTH: usize = 4;

    pub const fn new(near: f64, near_value: f64, far: f64, far_value: f64) -> Self {
        Self { near, near_value, far, far_value }
    }

    pub fn pack(&self, array: &mut [f64], starting_index: usize) {
        array[starting_index..starting_index + 4].copy_from_slice(&[
            self.near,
            self.near_value,
            self.far,
            self.far_value,
        ]);
    }

    pub fn unpack(array: &[f64], starting_index: usize) -> Self {
        let a = &array[starting_index..starting_index + 4];
        Self::new(a[0], a[1], a[2], a[3])
    }
}
