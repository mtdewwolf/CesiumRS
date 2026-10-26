//! CesiumJS `PolylineGraphics`: A polyline attached to an entity.
//!
//! Property slots begin undefined; visualizer defaults are applied when the
//! graphics object is drawn, not in this constructor. See `graphics.rs` for
//! the shared definition, event, merge and clone contracts.
use super::graphics::graphics_definition;
use super::{ComponentRef, DefinitionChanged, DynamicValue, EntityComponent, PropertyInput};
use crate::core::RuntimeError;
use std::any::Any;
graphics_definition! {
    PolylineGraphics;
    properties: [
        "show" => show, set_show,
        "positions" => positions, set_positions,
        "width" => width, set_width,
        "granularity" => granularity, set_granularity,
        "material" => material, set_material,
        "depthFailMaterial" => depth_fail_material, set_depth_fail_material,
        "arcType" => arc_type, set_arc_type,
        "clampToGround" => clamp_to_ground, set_clamp_to_ground,
        "shadows" => shadows, set_shadows,
        "distanceDisplayCondition" => distance_display_condition, set_distance_display_condition,
        "classificationType" => classification_type, set_classification_type,
        "zIndex" => z_index, set_z_index
    ];
    merge: [
        "show",
        "positions",
        "width",
        "granularity",
        "material",
        "depthFailMaterial",
        "arcType",
        "clampToGround",
        "shadows",
        "distanceDisplayCondition",
        "classificationType",
        "zIndex"
    ];
    clone: [
        "show",
        "positions",
        "width",
        "granularity",
        "material",
        "depthFailMaterial",
        "arcType",
        "clampToGround",
        "shadows",
        "distanceDisplayCondition",
        "classificationType",
        "zIndex"
    ];
    nested: [

    ];
}
