//! CesiumJS `StripeMaterialProperty`.
use super::record_property::record_property;
use crate::core::Color;
record_property! {
    StripeMaterialProperty;
    fields: [
        "orientation" => orientation, set_orientation;
        "evenColor" => even_color, set_even_color;
        "oddColor" => odd_color, set_odd_color;
        "offset" => offset, set_offset;
        "repeat" => repeat, set_repeat;
    ];
    material: "Stripe";
    evaluate: |out| {
        let orientation = out.read("orientation")?;
        let horizontal = matches!(orientation, None | Some(DynamicValue::Null | DynamicValue::Number(0.0)));
        out.set("horizontal", Some(horizontal.into()));
        out.field("evenColor", Color::WHITE)?;
        out.field("oddColor", Color::BLACK)?;
        out.field_without_result("offset", 0.)?;
        out.field_without_result("repeat", 1.)?;
    }
}
impl StripeMaterialProperty {
    pub fn new() -> Self {
        Self::default()
    }
}
