//! CesiumJS `KeyboardEventModifier`: a modifier key held during an input event.

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum KeyboardEventModifier {
    Shift = 0,
    Ctrl = 1,
    Alt = 2,
}

impl KeyboardEventModifier {
    pub const ALL: [Self; 3] = [Self::Shift, Self::Ctrl, Self::Alt];

    /// The variant with Cesium's numeric value.
    pub fn from_value(value: u8) -> Option<Self> {
        Self::ALL.into_iter().find(|v| *v as u8 == value)
    }

    pub fn validate(value: u8) -> bool {
        Self::from_value(value).is_some()
    }
}
