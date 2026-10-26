//! CesiumJS `PointGraphics`: A point attached to an entity.
//!
//! Property slots begin undefined; visualizer defaults are applied when the
//! graphics object is drawn, not in this constructor. See `graphics.rs` for
//! the shared definition, event, merge and clone contracts.
use super::graphics::graphics_definition;
use super::{ComponentRef, DefinitionChanged, DynamicValue, EntityComponent, PropertyInput};
use crate::core::RuntimeError;
use std::any::Any;
graphics_definition! {
    PointGraphics;
    properties: [
        "show" => show, set_show,
        "pixelSize" => pixel_size, set_pixel_size,
        "heightReference" => height_reference, set_height_reference,
        "color" => color, set_color,
        "outlineColor" => outline_color, set_outline_color,
        "outlineWidth" => outline_width, set_outline_width,
        "scaleByDistance" => scale_by_distance, set_scale_by_distance,
        "translucencyByDistance" => translucency_by_distance, set_translucency_by_distance,
        "distanceDisplayCondition" => distance_display_condition, set_distance_display_condition,
        "disableDepthTestDistance" => disable_depth_test_distance, set_disable_depth_test_distance,
        "splitDirection" => split_direction, set_split_direction
    ];
    merge: [
        "show",
        "pixelSize",
        "heightReference",
        "color",
        "outlineColor",
        "outlineWidth",
        "scaleByDistance",
        "translucencyByDistance",
        "distanceDisplayCondition",
        "disableDepthTestDistance",
        "splitDirection"
    ];
    clone: [
        "show",
        "pixelSize",
        "heightReference",
        "color",
        "outlineColor",
        "outlineWidth",
        "scaleByDistance",
        "translucencyByDistance",
        "distanceDisplayCondition",
        "disableDepthTestDistance",
        "splitDirection"
    ];
    nested: [

    ];
}
