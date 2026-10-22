//! CesiumJS `StyleCommandsNeeded`: which draw passes a styled model needs.

numeric_enum!(StyleCommandsNeeded: i32 {
    AllOpaque = 0 => "ALL_OPAQUE",
    AllTranslucent = 1 => "ALL_TRANSLUCENT",
    OpaqueAndTranslucent = 2 => "OPAQUE_AND_TRANSLUCENT",
});

impl StyleCommandsNeeded {
    pub fn get_style_commands_needed(features_length: f64, translucent_features_length: f64) -> Self {
        if translucent_features_length == 0.0 {
            Self::AllOpaque
        } else if translucent_features_length == features_length {
            Self::AllTranslucent
        } else {
            Self::OpaqueAndTranslucent
        }
    }
}
