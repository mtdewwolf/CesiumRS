//! CesiumJS `ScreenSpaceEventType`: mouse, wheel and touch input events.

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum ScreenSpaceEventType {
    LeftDown = 0,
    LeftUp = 1,
    LeftClick = 2,
    LeftDoubleClick = 3,
    RightDown = 5,
    RightUp = 6,
    RightClick = 7,
    MiddleDown = 10,
    MiddleUp = 11,
    MiddleClick = 12,
    MouseMove = 15,
    Wheel = 16,
    PinchStart = 17,
    PinchEnd = 18,
    PinchMove = 19,
}

impl ScreenSpaceEventType {
    pub const ALL: [Self; 15] = [
        Self::LeftDown,
        Self::LeftUp,
        Self::LeftClick,
        Self::LeftDoubleClick,
        Self::RightDown,
        Self::RightUp,
        Self::RightClick,
        Self::MiddleDown,
        Self::MiddleUp,
        Self::MiddleClick,
        Self::MouseMove,
        Self::Wheel,
        Self::PinchStart,
        Self::PinchEnd,
        Self::PinchMove,
    ];

    /// The variant with Cesium's numeric value.
    pub fn from_value(value: u8) -> Option<Self> {
        Self::ALL.into_iter().find(|v| *v as u8 == value)
    }

    pub fn validate(value: u8) -> bool {
        Self::from_value(value).is_some()
    }
}
