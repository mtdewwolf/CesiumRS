//! CesiumJS `BillboardGraphics`: A billboard sprite attached to an entity.
//!
//! Property slots begin undefined; visualizer defaults are applied when the
//! graphics object is drawn, not in this constructor. See `graphics.rs` for
//! the shared definition, event, merge and clone contracts.
use super::graphics::graphics_definition;
use super::{ComponentRef, DefinitionChanged, DynamicValue, EntityComponent, PropertyInput};
use crate::core::RuntimeError;
use std::any::Any;
graphics_definition! {
    BillboardGraphics;
    properties: [
        "show" => show, set_show,
        "image" => image, set_image,
        "scale" => scale, set_scale,
        "pixelOffset" => pixel_offset, set_pixel_offset,
        "eyeOffset" => eye_offset, set_eye_offset,
        "horizontalOrigin" => horizontal_origin, set_horizontal_origin,
        "verticalOrigin" => vertical_origin, set_vertical_origin,
        "heightReference" => height_reference, set_height_reference,
        "color" => color, set_color,
        "rotation" => rotation, set_rotation,
        "alignedAxis" => aligned_axis, set_aligned_axis,
        "sizeInMeters" => size_in_meters, set_size_in_meters,
        "width" => width, set_width,
        "height" => height, set_height,
        "scaleByDistance" => scale_by_distance, set_scale_by_distance,
        "translucencyByDistance" => translucency_by_distance, set_translucency_by_distance,
        "pixelOffsetScaleByDistance" => pixel_offset_scale_by_distance, set_pixel_offset_scale_by_distance,
        "imageSubRegion" => image_sub_region, set_image_sub_region,
        "distanceDisplayCondition" => distance_display_condition, set_distance_display_condition,
        "disableDepthTestDistance" => disable_depth_test_distance, set_disable_depth_test_distance,
        "splitDirection" => split_direction, set_split_direction
    ];
    merge: [
        "show",
        "image",
        "scale",
        "pixelOffset",
        "eyeOffset",
        "horizontalOrigin",
        "verticalOrigin",
        "heightReference",
        "color",
        "rotation",
        "alignedAxis",
        "sizeInMeters",
        "width",
        "height",
        "scaleByDistance",
        "translucencyByDistance",
        "pixelOffsetScaleByDistance",
        "imageSubRegion",
        "distanceDisplayCondition",
        "disableDepthTestDistance",
        "splitDirection"
    ];
    clone: [
        "show",
        "image",
        "scale",
        "pixelOffset",
        "eyeOffset",
        "horizontalOrigin",
        "verticalOrigin",
        "heightReference",
        "color",
        "rotation",
        "alignedAxis",
        "sizeInMeters",
        "width",
        "height",
        "scaleByDistance",
        "translucencyByDistance",
        "pixelOffsetScaleByDistance",
        "imageSubRegion",
        "distanceDisplayCondition",
        "disableDepthTestDistance",
        "splitDirection"
    ];
    nested: [

    ];
}
