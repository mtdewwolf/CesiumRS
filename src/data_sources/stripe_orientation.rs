//! Direction of the bands in a stripe material.
use super::DynamicValue;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
#[repr(u8)]
pub enum StripeOrientation {
    Horizontal = 0,
    Vertical = 1,
}
impl From<StripeOrientation> for DynamicValue {
    fn from(value: StripeOrientation) -> Self {
        Self::Number(value as u8 as f64)
    }
}
