//! CesiumJS `ShowGeometryInstanceAttribute`: instance visibility as one
//! unsigned byte.

use super::component_datatype::ComponentDatatype;
use super::geometry_instance_attribute::GeometryInstanceAttribute;
use super::typed_array::TypedArray;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ShowGeometryInstanceAttribute {
    pub value: [u8; 1],
}

impl Default for ShowGeometryInstanceAttribute {
    fn default() -> Self {
        Self::new(None)
    }
}

impl ShowGeometryInstanceAttribute {
    pub const COMPONENT_DATATYPE: ComponentDatatype = ComponentDatatype::UnsignedByte;
    pub const COMPONENTS_PER_ATTRIBUTE: u8 = 1;
    pub const NORMALIZE: bool = false;

    /// `show` defaults to true.
    pub fn new(show: Option<bool>) -> Self {
        Self { value: Self::to_value(show.unwrap_or(true)) }
    }

    pub fn to_value(show: bool) -> [u8; 1] {
        [u8::from(show)]
    }
}

impl From<ShowGeometryInstanceAttribute> for GeometryInstanceAttribute {
    fn from(a: ShowGeometryInstanceAttribute) -> Self {
        Self::new(
            ShowGeometryInstanceAttribute::COMPONENT_DATATYPE,
            ShowGeometryInstanceAttribute::COMPONENTS_PER_ATTRIBUTE,
            Some(ShowGeometryInstanceAttribute::NORMALIZE),
            TypedArray::Uint8(a.value.to_vec()),
        )
    }
}
