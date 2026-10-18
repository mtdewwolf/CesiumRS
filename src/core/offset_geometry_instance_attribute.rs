//! CesiumJS `OffsetGeometryInstanceAttribute`: an instance's translation as
//! three 32-bit floats.

use super::cartesian3::Cartesian3;
use super::component_datatype::ComponentDatatype;
use super::geometry_instance_attribute::GeometryInstanceAttribute;
use super::typed_array::TypedArray;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct OffsetGeometryInstanceAttribute {
    pub value: [f32; 3],
}

impl OffsetGeometryInstanceAttribute {
    pub const COMPONENT_DATATYPE: ComponentDatatype = ComponentDatatype::Float;
    pub const COMPONENTS_PER_ATTRIBUTE: u8 = 3;
    pub const NORMALIZE: bool = false;

    /// Each component defaults to 0.
    pub fn new(x: Option<f64>, y: Option<f64>, z: Option<f64>) -> Self {
        let f = |v: Option<f64>| v.unwrap_or(0.0) as f32;
        Self { value: [f(x), f(y), f(z)] }
    }

    pub fn from_cartesian3(offset: &Cartesian3) -> Self {
        Self::new(Some(offset.x), Some(offset.y), Some(offset.z))
    }

    pub fn to_value(offset: &Cartesian3) -> [f32; 3] {
        [offset.x as f32, offset.y as f32, offset.z as f32]
    }
}

impl From<OffsetGeometryInstanceAttribute> for GeometryInstanceAttribute {
    fn from(a: OffsetGeometryInstanceAttribute) -> Self {
        Self::new(
            OffsetGeometryInstanceAttribute::COMPONENT_DATATYPE,
            OffsetGeometryInstanceAttribute::COMPONENTS_PER_ATTRIBUTE,
            Some(OffsetGeometryInstanceAttribute::NORMALIZE),
            TypedArray::Float32(a.value.to_vec()),
        )
    }
}
