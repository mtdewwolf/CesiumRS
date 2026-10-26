//! CesiumJS `EllipseGraphics`: An ellipse on the ellipsoid surface.
//!
//! Property slots begin undefined; visualizer defaults are applied when the
//! graphics object is drawn, not in this constructor. See `graphics.rs` for
//! the shared definition, event, merge and clone contracts.
use super::graphics::graphics_definition;
use super::{ComponentRef, DefinitionChanged, DynamicValue, EntityComponent, PropertyInput};
use crate::core::RuntimeError;
use std::any::Any;
graphics_definition! {
    EllipseGraphics;
    properties: [
        "show" => show, set_show,
        "semiMajorAxis" => semi_major_axis, set_semi_major_axis,
        "semiMinorAxis" => semi_minor_axis, set_semi_minor_axis,
        "height" => height, set_height,
        "heightReference" => height_reference, set_height_reference,
        "extrudedHeight" => extruded_height, set_extruded_height,
        "extrudedHeightReference" => extruded_height_reference, set_extruded_height_reference,
        "rotation" => rotation, set_rotation,
        "stRotation" => st_rotation, set_st_rotation,
        "granularity" => granularity, set_granularity,
        "fill" => fill, set_fill,
        "material" => material, set_material,
        "outline" => outline, set_outline,
        "outlineColor" => outline_color, set_outline_color,
        "outlineWidth" => outline_width, set_outline_width,
        "numberOfVerticalLines" => number_of_vertical_lines, set_number_of_vertical_lines,
        "shadows" => shadows, set_shadows,
        "distanceDisplayCondition" => distance_display_condition, set_distance_display_condition,
        "classificationType" => classification_type, set_classification_type,
        "zIndex" => z_index, set_z_index
    ];
    merge: [
        "show",
        "semiMajorAxis",
        "semiMinorAxis",
        "height",
        "heightReference",
        "extrudedHeight",
        "extrudedHeightReference",
        "rotation",
        "stRotation",
        "granularity",
        "fill",
        "material",
        "outline",
        "outlineColor",
        "outlineWidth",
        "numberOfVerticalLines",
        "shadows",
        "distanceDisplayCondition",
        "classificationType",
        "zIndex"
    ];
    clone: [
        "show",
        "semiMajorAxis",
        "semiMinorAxis",
        "height",
        "heightReference",
        "extrudedHeight",
        "extrudedHeightReference",
        "rotation",
        "stRotation",
        "granularity",
        "fill",
        "material",
        "outline",
        "outlineColor",
        "outlineWidth",
        "numberOfVerticalLines",
        "shadows",
        "distanceDisplayCondition",
        "classificationType",
        "zIndex"
    ];
    nested: [

    ];
}
