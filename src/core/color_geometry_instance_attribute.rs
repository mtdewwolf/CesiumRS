//! CesiumJS `ColorGeometryInstanceAttribute`: an instance color stored as
//! four normalized unsigned bytes.

use super::color::Color;
use super::component_datatype::ComponentDatatype;
use super::geometry_instance_attribute::GeometryInstanceAttribute;
use super::typed_array::TypedArray;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ColorGeometryInstanceAttribute {
    pub value: [u8; 4],
}

impl Default for ColorGeometryInstanceAttribute {
    fn default() -> Self {
        Self::new(None, None, None, None)
    }
}

impl ColorGeometryInstanceAttribute {
    pub const COMPONENT_DATATYPE: ComponentDatatype = ComponentDatatype::UnsignedByte;
    pub const COMPONENTS_PER_ATTRIBUTE: u8 = 4;
    pub const NORMALIZE: bool = true;

    /// Each component defaults to 1.0 and is stored with `Color.floatToByte`,
    /// wrapping like a `Uint8Array`.
    pub fn new(red: Option<f64>, green: Option<f64>, blue: Option<f64>, alpha: Option<f64>) -> Self {
        let byte = |v: Option<f64>| Color::float_to_byte(v.unwrap_or(1.0)) as u8;
        Self { value: [byte(red), byte(green), byte(blue), byte(alpha)] }
    }

    pub fn from_color(color: &Color) -> Self {
        Self::new(Some(color.red), Some(color.green), Some(color.blue), Some(color.alpha))
    }

    pub fn to_value(color: &Color) -> [u8; 4] {
        color.to_bytes().map(|b| b as u8)
    }

    pub fn equals(left: Option<&Self>, right: Option<&Self>) -> bool {
        match (left, right) {
            (Some(l), Some(r)) => l.value == r.value,
            (None, None) => true,
            _ => false,
        }
    }
}

impl From<ColorGeometryInstanceAttribute> for GeometryInstanceAttribute {
    fn from(a: ColorGeometryInstanceAttribute) -> Self {
        Self::new(
            ColorGeometryInstanceAttribute::COMPONENT_DATATYPE,
            ColorGeometryInstanceAttribute::COMPONENTS_PER_ATTRIBUTE,
            Some(ColorGeometryInstanceAttribute::NORMALIZE),
            TypedArray::Uint8(a.value.to_vec()),
        )
    }
}
