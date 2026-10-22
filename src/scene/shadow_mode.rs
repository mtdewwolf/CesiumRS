//! CesiumJS `ShadowMode`: whether an object casts and receives shadows.

numeric_enum!(ShadowMode: i32 {
    Disabled = 0 => "DISABLED",
    Enabled = 1 => "ENABLED",
    CastOnly = 2 => "CAST_ONLY",
    ReceiveOnly = 3 => "RECEIVE_ONLY",
});

impl ShadowMode {
    pub const NUMBER_OF_SHADOW_MODES: i32 = 4;

    pub fn cast_shadows(self) -> bool {
        matches!(self, Self::Enabled | Self::CastOnly)
    }

    pub fn receive_shadows(self) -> bool {
        matches!(self, Self::Enabled | Self::ReceiveOnly)
    }

    pub fn from_cast_receive(cast_shadows: bool, receive_shadows: bool) -> Self {
        match (cast_shadows, receive_shadows) {
            (true, true) => Self::Enabled,
            (true, false) => Self::CastOnly,
            (false, true) => Self::ReceiveOnly,
            (false, false) => Self::Disabled,
        }
    }
}
