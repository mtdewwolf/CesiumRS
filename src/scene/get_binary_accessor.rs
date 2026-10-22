//! CesiumJS `getBinaryAccessor`: reads batch and feature table binary bodies
//! described by `{ byteOffset, componentType, type }`.

use super::AttributeType;
use crate::core::{ComponentDatatype, TypedArray};

/// The accessor Cesium returns. `class_type` stands in for the Cesium class
/// (`undefined` for scalars), which is the attribute type's math type.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BinaryAccessor {
    pub components_per_attribute: usize,
    pub attribute_type: AttributeType,
    pub component_datatype: ComponentDatatype,
}

/// A feature table component type: a `ComponentDatatype` name such as
/// `"FLOAT"`, or its WebGL value.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ComponentType<'a> {
    Name(&'a str),
    Value(ComponentDatatype),
}

/// `None` for an unknown component type name or attribute type.
pub fn get_binary_accessor(component_type: ComponentType<'_>, attribute_type: &str) -> Option<BinaryAccessor> {
    let component_datatype = match component_type {
        ComponentType::Name(name) => ComponentDatatype::from_name(name)?,
        ComponentType::Value(datatype) => datatype,
    };
    let attribute_type = AttributeType::from_str_value(attribute_type)?;
    Some(BinaryAccessor {
        components_per_attribute: attribute_type.get_number_of_components(),
        attribute_type,
        component_datatype,
    })
}

impl BinaryAccessor {
    /// `length` elements of `components_per_attribute` components starting at
    /// `byte_offset`. `None` when that runs past `buffer`, where JavaScript
    /// throws a `RangeError`.
    pub fn create_array_buffer_view(&self, buffer: &[u8], byte_offset: usize, length: usize) -> Option<TypedArray> {
        self.component_datatype.create_array_buffer_view(
            buffer,
            byte_offset,
            Some(self.components_per_attribute * length),
        )
    }
}
