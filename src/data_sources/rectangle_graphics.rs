//! CesiumJS `RectangleGraphics`: A rectangle on the ellipsoid surface.
//!
//! Property slots begin undefined; visualizer defaults are applied when the
//! graphics object is drawn, not in this constructor. See `graphics.rs` for
//! the shared definition, event, merge and clone contracts.
use super::graphics::graphics_definition;
use super::{ComponentRef, DefinitionChanged, DynamicValue, EntityComponent, PropertyInput};
use crate::core::RuntimeError;
use std::any::Any;
graphics_definition! {
    RectangleGraphics;
    properties: [
        "show" => show, set_show,
        "coordinates" => coordinates, set_coordinates,
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
        "shadows" => shadows, set_shadows,
        "distanceDisplayCondition" => distance_display_condition, set_distance_display_condition,
        "classificationType" => classification_type, set_classification_type,
        "zIndex" => z_index, set_z_index
    ];
    merge: [
        "show",
        "coordinates",
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
        "shadows",
        "distanceDisplayCondition",
        "classificationType",
        "zIndex"
    ];
    clone: [
        "show",
        "coordinates",
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
        "shadows",
        "distanceDisplayCondition",
        "classificationType",
        "zIndex"
    ];
    nested: [

    ];
}
