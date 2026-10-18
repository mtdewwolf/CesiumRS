//! CesiumJS `DistanceDisplayConditionGeometryInstanceAttribute`: the near
//! and far display distances as two 32-bit floats.

use super::component_datatype::ComponentDatatype;
use super::distance_display_condition::DistanceDisplayCondition;
use super::geometry_instance_attribute::GeometryInstanceAttribute;
use super::typed_array::TypedArray;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DistanceDisplayConditionGeometryInstanceAttribute {
    pub value: [f32; 2],
}

impl Default for DistanceDisplayConditionGeometryInstanceAttribute {
    fn default() -> Self {
        Self::new(None, None)
    }
}

impl DistanceDisplayConditionGeometryInstanceAttribute {
    pub const COMPONENT_DATATYPE: ComponentDatatype = ComponentDatatype::Float;
    pub const COMPONENTS_PER_ATTRIBUTE: u8 = 2;
    pub const NORMALIZE: bool = false;

    /// `near` defaults to 0 and `far` to `Number.MAX_VALUE`, which rounds to
    /// infinity as a 32-bit float. Cesium's debug build rejects
    /// `far <= near`; the release build, like this, stores them anyway.
    pub fn new(near: Option<f64>, far: Option<f64>) -> Self {
        Self { value: [near.unwrap_or(0.0) as f32, far.unwrap_or(f64::MAX) as f32] }
    }

    pub fn from_distance_display_condition(condition: &DistanceDisplayCondition) -> Self {
        Self::new(Some(condition.near), Some(condition.far))
    }

    pub fn to_value(condition: &DistanceDisplayCondition) -> [f32; 2] {
        [condition.near as f32, condition.far as f32]
    }
}

impl From<DistanceDisplayConditionGeometryInstanceAttribute> for GeometryInstanceAttribute {
    fn from(a: DistanceDisplayConditionGeometryInstanceAttribute) -> Self {
        Self::new(
            DistanceDisplayConditionGeometryInstanceAttribute::COMPONENT_DATATYPE,
            DistanceDisplayConditionGeometryInstanceAttribute::COMPONENTS_PER_ATTRIBUTE,
            Some(DistanceDisplayConditionGeometryInstanceAttribute::NORMALIZE),
            TypedArray::Float32(a.value.to_vec()),
        )
    }
}
