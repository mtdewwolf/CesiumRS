//! CesiumJS `PolygonGraphics`: A polygon on the ellipsoid.
//!
//! Property slots begin undefined; visualizer defaults are applied when the
//! graphics object is drawn, not in this constructor. See `graphics.rs` for
//! the shared definition, event, merge and clone contracts.
use super::graphics::graphics_definition;
use super::{ComponentRef, DefinitionChanged, DynamicValue, EntityComponent, PropertyInput};
use crate::core::RuntimeError;
use std::any::Any;
graphics_definition! {
    PolygonGraphics;
    properties: [
        "show" => show, set_show,
        "hierarchy" => hierarchy, set_hierarchy,
        "height" => height, set_height,
        "heightReference" => height_reference, set_height_reference,
        "extrudedHeight" => extruded_height, set_extruded_height,
        "extrudedHeightReference" => extruded_height_reference, set_extruded_height_reference,
        "stRotation" => st_rotation, set_st_rotation,
        "granularity" => granularity, set_granularity,
        "fill" => fill, set_fill,
        "material" => material, set_material,
        "outline" => outline, set_outline,
        "outlineColor" => outline_color, set_outline_color,
        "outlineWidth" => outline_width, set_outline_width,
        "perPositionHeight" => per_position_height, set_per_position_height,
        "closeTop" => close_top, set_close_top,
        "closeBottom" => close_bottom, set_close_bottom,
        "arcType" => arc_type, set_arc_type,
        "shadows" => shadows, set_shadows,
        "distanceDisplayCondition" => distance_display_condition, set_distance_display_condition,
        "classificationType" => classification_type, set_classification_type,
        "zIndex" => z_index, set_z_index,
        "textureCoordinates" => texture_coordinates, set_texture_coordinates
    ];
    merge: [
        "show",
        "hierarchy",
        "height",
        "heightReference",
        "extrudedHeight",
        "extrudedHeightReference",
        "stRotation",
        "granularity",
        "fill",
        "material",
        "outline",
        "outlineColor",
        "outlineWidth",
        "perPositionHeight",
        "closeTop",
        "closeBottom",
        "arcType",
        "shadows",
        "distanceDisplayCondition",
        "classificationType",
        "zIndex",
        "textureCoordinates"
    ];
    clone: [
        "show",
        "hierarchy",
        "height",
        "heightReference",
        "extrudedHeight",
        "extrudedHeightReference",
        "stRotation",
        "granularity",
        "fill",
        "material",
        "outline",
        "outlineColor",
        "outlineWidth",
        "perPositionHeight",
        "closeTop",
        "closeBottom",
        "arcType",
        "shadows",
        "distanceDisplayCondition",
        "classificationType",
        "zIndex",
        "textureCoordinates"
    ];
    nested: [

    ];
}
