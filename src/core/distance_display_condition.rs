//! CesiumJS `DistanceDisplayCondition`: the camera-distance range in which
//! something is shown.

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DistanceDisplayCondition {
    pub near: f64,
    pub far: f64,
}

impl Default for DistanceDisplayCondition {
    fn default() -> Self {
        Self { near: 0.0, far: f64::MAX }
    }
}

impl DistanceDisplayCondition {
    pub const PACKED_LENGTH: usize = 2;

    pub const fn new(near: f64, far: f64) -> Self {
        Self { near, far }
    }

    pub fn pack(&self, array: &mut [f64], starting_index: usize) {
        array[starting_index] = self.near;
        array[starting_index + 1] = self.far;
    }

    pub fn unpack(array: &[f64], starting_index: usize) -> Self {
        Self::new(array[starting_index], array[starting_index + 1])
    }
}
