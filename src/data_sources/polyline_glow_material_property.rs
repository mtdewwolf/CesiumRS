//! CesiumJS `PolylineGlowMaterialProperty`.
use super::record_property::record_property;
use crate::core::Color;
record_property! {
    PolylineGlowMaterialProperty;
    fields: [
        "color" => color, set_color;
        "glowPower" => glow_power, set_glow_power;
        "taperPower" => taper_power, set_taper_power;
    ];
    // Cesium 23.0.0 checks only these descriptors for constancy.
    constant: ["color"];
    material: "PolylineGlow";
    evaluate: |out| {
        out.field("color", Color::WHITE)?;
        out.field("glowPower", 0.25)?;
        out.field("taperPower", 1.)?;
    }
}
impl PolylineGlowMaterialProperty {
    pub fn new() -> Self {
        Self::default()
    }
}
