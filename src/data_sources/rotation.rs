//! Packed counter-clockwise angle in radians for `SampledProperty<Rotation>`.
use super::{ComponentValue, DynamicValue, PropertyValue, SampledValue};
use std::f64::consts::{PI, TAU};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Rotation(pub f64);

impl Rotation {
    pub const PACKED_LENGTH: usize = 1;
    pub fn pack(self, array: &mut [f64], offset: usize) {
        array[offset] = self.0;
    }
    pub fn unpack(array: &[f64], offset: usize) -> Self {
        Self(array[offset])
    }
    /// Cesium compares adjacent *packed* values and subtracts one full turn
    /// from the current value when the absolute difference reaches pi.
    pub fn convert_packed_array_for_interpolation(values: &[f64], first: usize, last: usize) -> Vec<f64> {
        (first..=last)
            .map(|i| {
                let value = values[i];
                if i == first || (values[i - 1] - value).abs() < PI {
                    value
                } else {
                    value - TAU
                }
            })
            .collect()
    }
    pub fn unpack_interpolation_result(array: &[f64]) -> Self {
        Self(if array[0] < 0. { array[0] + TAU } else { array[0] })
    }
}
impl PropertyValue for Rotation {
    fn property_equals(&self, other: &Self) -> bool {
        self == other
    }
    fn has_equals_method(&self) -> bool {
        false
    }
    fn property_string(&self) -> String {
        self.0.property_string()
    }
}
impl ComponentValue for Rotation {
    fn into_dynamic(self) -> DynamicValue {
        self.0.into()
    }
    fn from_dynamic(value: &DynamicValue) -> Option<Self> {
        f64::from_dynamic(value).map(Self)
    }
}
impl SampledValue for Rotation {
    const PACKED_LENGTH: usize = Self::PACKED_LENGTH;
    const TYPE_NAME: &'static str = "Rotation";
    fn pack(&self, array: &mut [f64], offset: usize) {
        Self::pack(*self, array, offset);
    }
    fn unpack(array: &[f64], offset: usize) -> Self {
        Self::unpack(array, offset)
    }
    fn convert_for_interpolation(values: &[f64], first: usize, last: usize) -> Option<Vec<f64>> {
        Some(Self::convert_packed_array_for_interpolation(values, first, last))
    }
    fn unpack_interpolated(array: &[f64], _: &[f64], _: usize, _: usize) -> Self {
        Self::unpack_interpolation_result(array)
    }
}
