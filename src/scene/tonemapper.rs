//! CesiumJS `Tonemapper`: the tonemapping operator applied to HDR output.

string_enum!(Tonemapper {
    Reinhard = "REINHARD" => "REINHARD",
    ModifiedReinhard = "MODIFIED_REINHARD" => "MODIFIED_REINHARD",
    Filmic = "FILMIC" => "FILMIC",
    Aces = "ACES" => "ACES",
    PbrNeutral = "PBR_NEUTRAL" => "PBR_NEUTRAL",
});

/// Cesium's `validateTonemapper`: whether `tonemapper` names an operator.
pub fn validate_tonemapper(tonemapper: &str) -> bool {
    Tonemapper::from_str_value(tonemapper).is_some()
}
