//! CesiumJS `CylinderGraphics`: A cylinder centered on the entity position.
//!
//! Property slots begin undefined; visualizer defaults are applied when the
//! graphics object is drawn, not in this constructor. See `graphics.rs` for
//! the shared definition, event, merge and clone contracts.
use super::graphics::graphics_definition;
use super::{ComponentRef, DefinitionChanged, DynamicValue, EntityComponent, PropertyInput};
use crate::core::RuntimeError;
use std::any::Any;
graphics_definition! {
    CylinderGraphics;
    properties: [
        "show" => show, set_show,
        "length" => length, set_length,
        "topRadius" => top_radius, set_top_radius,
        "bottomRadius" => bottom_radius, set_bottom_radius,
        "heightReference" => height_reference, set_height_reference,
        "fill" => fill, set_fill,
        "material" => material, set_material,
        "outline" => outline, set_outline,
        "outlineColor" => outline_color, set_outline_color,
        "outlineWidth" => outline_width, set_outline_width,
        "numberOfVerticalLines" => number_of_vertical_lines, set_number_of_vertical_lines,
        "slices" => slices, set_slices,
        "shadows" => shadows, set_shadows,
        "distanceDisplayCondition" => distance_display_condition, set_distance_display_condition
    ];
    merge: [
        "show",
        "length",
        "topRadius",
        "bottomRadius",
        "heightReference",
        "fill",
        "material",
        "outline",
        "outlineColor",
        "outlineWidth",
        "numberOfVerticalLines",
        "slices",
        "shadows",
        "distanceDisplayCondition"
    ];
    clone: [
        "show",
        "length",
        "topRadius",
        "bottomRadius",
        "heightReference",
        "fill",
        "material",
        "outline",
        "outlineColor",
        "outlineWidth",
        "numberOfVerticalLines",
        "slices",
        "shadows",
        "distanceDisplayCondition"
    ];
    nested: [

    ];
}
