//! CesiumJS `PathGraphics`: A time-dynamic path trailing an entity.
//!
//! Property slots begin undefined; visualizer defaults are applied when the
//! graphics object is drawn, not in this constructor. See `graphics.rs` for
//! the shared definition, event, merge and clone contracts.
use super::graphics::graphics_definition;
use super::{ComponentRef, DefinitionChanged, DynamicValue, EntityComponent, PropertyInput};
use crate::core::RuntimeError;
use std::any::Any;
graphics_definition! {
    PathGraphics;
    properties: [
        "show" => show, set_show,
        "leadTime" => lead_time, set_lead_time,
        "trailTime" => trail_time, set_trail_time,
        "width" => width, set_width,
        "resolution" => resolution, set_resolution,
        "material" => material, set_material,
        "distanceDisplayCondition" => distance_display_condition, set_distance_display_condition
    ];
    merge: [
        "show",
        "leadTime",
        "trailTime",
        "width",
        "resolution",
        "material",
        "distanceDisplayCondition"
    ];
    clone: [
        "show",
        "leadTime",
        "trailTime",
        "width",
        "resolution",
        "material",
        "distanceDisplayCondition"
    ];
    nested: [

    ];
}
