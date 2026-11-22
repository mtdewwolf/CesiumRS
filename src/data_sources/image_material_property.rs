//! CesiumJS `ImageMaterialProperty`.
use super::record_property::record_property;
use crate::core::{Cartesian2, Color};
record_property! {
    ImageMaterialProperty;
    fields: [
        "image" => image, set_image;
        "repeat" => repeat, set_repeat;
        "color" => color, set_color;
        "transparent" => transparent, set_transparent;
    ];
    // Cesium 23.0.0 checks only these descriptors for constancy.
    constant: ["image", "repeat"];
    material: "Image";
    evaluate: |out| {
        out.set("image", out.read("image")?);
        out.field("repeat", Cartesian2::new(1., 1.))?;
        out.field("color", Color::WHITE)?;
        if matches!(out.read("transparent")?, Some(DynamicValue::Boolean(true))) {
            if let Some(DynamicValue::Color(color)) = out.get_mut("color") {
                if color.alpha > 0.99 { color.alpha = 0.99; }
            }
        }
    }
}
impl ImageMaterialProperty {
    pub fn new() -> Self {
        Self::default()
    }
}
