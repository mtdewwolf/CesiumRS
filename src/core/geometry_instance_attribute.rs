//! CesiumJS `GeometryInstanceAttribute`: a per-instance value, such as a
//! color, that applies to every vertex of a geometry instance.

use super::component_datatype::ComponentDatatype;
use super::typed_array::TypedArray;

#[derive(Clone, Debug, PartialEq)]
pub struct GeometryInstanceAttribute {
    pub component_datatype: ComponentDatatype,
    /// 1 to 4.
    pub components_per_attribute: u8,
    pub normalize: bool,
    pub value: TypedArray,
}

impl GeometryInstanceAttribute {
    pub fn new(
        component_datatype: ComponentDatatype,
        components_per_attribute: u8,
        normalize: Option<bool>,
        value: TypedArray,
    ) -> Self {
        Self { component_datatype, components_per_attribute, normalize: normalize.unwrap_or(false), value }
    }
}
