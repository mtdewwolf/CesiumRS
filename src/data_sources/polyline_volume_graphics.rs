//! CesiumJS `PolylineVolumeGraphics`: A volume produced by extruding a shape along a polyline.
//!
//! Property slots begin undefined; visualizer defaults are applied when the
//! graphics object is drawn, not in this constructor. See `graphics.rs` for
//! the shared definition, event, merge and clone contracts.
use super::graphics::graphics_definition;
use super::{ComponentRef, DefinitionChanged, DynamicValue, EntityComponent, PropertyInput};
use crate::core::RuntimeError;
use std::any::Any;
graphics_definition! {
    PolylineVolumeGraphics;
    properties: [
        "show" => show, set_show,
        "positions" => positions, set_positions,
        "shape" => shape, set_shape,
        "cornerType" => corner_type, set_corner_type,
        "granularity" => granularity, set_granularity,
        "fill" => fill, set_fill,
        "material" => material, set_material,
        "outline" => outline, set_outline,
        "outlineColor" => outline_color, set_outline_color,
        "outlineWidth" => outline_width, set_outline_width,
        "shadows" => shadows, set_shadows,
        "distanceDisplayCondition" => distance_display_condition, set_distance_display_condition
    ];
    merge: [
        "show",
        "positions",
        "shape",
        "cornerType",
        "granularity",
        "fill",
        "material",
        "outline",
        "outlineColor",
        "outlineWidth",
        "shadows",
        "distanceDisplayCondition"
    ];
    clone: [
        "show",
        "positions",
        "shape",
        "cornerType",
        "granularity",
        "fill",
        "material",
        "outline",
        "outlineColor",
        "outlineWidth",
        "shadows",
        "distanceDisplayCondition"
    ];
    nested: [

    ];
}
