//! CesiumJS `LabelGraphics`: A text label attached to an entity.
//!
//! Property slots begin undefined; visualizer defaults are applied when the
//! graphics object is drawn, not in this constructor. See `graphics.rs` for
//! the shared definition, event, merge and clone contracts.
use super::graphics::graphics_definition;
use super::{ComponentRef, DefinitionChanged, DynamicValue, EntityComponent, PropertyInput};
use crate::core::RuntimeError;
use std::any::Any;
graphics_definition! {
    LabelGraphics;
    properties: [
        "show" => show, set_show,
        "text" => text, set_text,
        "font" => font, set_font,
        "style" => style, set_style,
        "scale" => scale, set_scale,
        "showBackground" => show_background, set_show_background,
        "backgroundColor" => background_color, set_background_color,
        "backgroundPadding" => background_padding, set_background_padding,
        "pixelOffset" => pixel_offset, set_pixel_offset,
        "eyeOffset" => eye_offset, set_eye_offset,
        "horizontalOrigin" => horizontal_origin, set_horizontal_origin,
        "verticalOrigin" => vertical_origin, set_vertical_origin,
        "heightReference" => height_reference, set_height_reference,
        "fillColor" => fill_color, set_fill_color,
        "outlineColor" => outline_color, set_outline_color,
        "outlineWidth" => outline_width, set_outline_width,
        "translucencyByDistance" => translucency_by_distance, set_translucency_by_distance,
        "pixelOffsetScaleByDistance" => pixel_offset_scale_by_distance, set_pixel_offset_scale_by_distance,
        "scaleByDistance" => scale_by_distance, set_scale_by_distance,
        "distanceDisplayCondition" => distance_display_condition, set_distance_display_condition,
        "disableDepthTestDistance" => disable_depth_test_distance, set_disable_depth_test_distance
    ];
    merge: [
        "show",
        "text",
        "font",
        "style",
        "scale",
        "showBackground",
        "backgroundColor",
        "backgroundPadding",
        "pixelOffset",
        "eyeOffset",
        "horizontalOrigin",
        "verticalOrigin",
        "heightReference",
        "fillColor",
        "outlineColor",
        "outlineWidth",
        "translucencyByDistance",
        "pixelOffsetScaleByDistance",
        "scaleByDistance",
        "distanceDisplayCondition",
        "disableDepthTestDistance"
    ];
    clone: [
        "show",
        "text",
        "font",
        "style",
        "scale",
        "showBackground",
        "backgroundColor",
        "backgroundPadding",
        "pixelOffset",
        "eyeOffset",
        "horizontalOrigin",
        "verticalOrigin",
        "heightReference",
        "fillColor",
        "outlineColor",
        "outlineWidth",
        "translucencyByDistance",
        "pixelOffsetScaleByDistance",
        "scaleByDistance",
        "distanceDisplayCondition",
        "disableDepthTestDistance"
    ];
    nested: [

    ];
}
