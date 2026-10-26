//! CesiumJS `EllipsoidGraphics`: An ellipsoid centered on the entity position.
//!
//! Property slots begin undefined; visualizer defaults are applied when the
//! graphics object is drawn, not in this constructor. See `graphics.rs` for
//! the shared definition, event, merge and clone contracts.
use super::graphics::graphics_definition;
use super::{ComponentRef, DefinitionChanged, DynamicValue, EntityComponent, PropertyInput};
use crate::core::RuntimeError;
use std::any::Any;
graphics_definition! {
    EllipsoidGraphics;
    properties: [
        "show" => show, set_show,
        "radii" => radii, set_radii,
        "innerRadii" => inner_radii, set_inner_radii,
        "minimumClock" => minimum_clock, set_minimum_clock,
        "maximumClock" => maximum_clock, set_maximum_clock,
        "minimumCone" => minimum_cone, set_minimum_cone,
        "maximumCone" => maximum_cone, set_maximum_cone,
        "heightReference" => height_reference, set_height_reference,
        "fill" => fill, set_fill,
        "material" => material, set_material,
        "outline" => outline, set_outline,
        "outlineColor" => outline_color, set_outline_color,
        "outlineWidth" => outline_width, set_outline_width,
        "stackPartitions" => stack_partitions, set_stack_partitions,
        "slicePartitions" => slice_partitions, set_slice_partitions,
        "subdivisions" => subdivisions, set_subdivisions,
        "shadows" => shadows, set_shadows,
        "distanceDisplayCondition" => distance_display_condition, set_distance_display_condition
    ];
    merge: [
        "show",
        "radii",
        "innerRadii",
        "minimumClock",
        "maximumClock",
        "minimumCone",
        "maximumCone",
        "heightReference",
        "fill",
        "material",
        "outline",
        "outlineColor",
        "outlineWidth",
        "stackPartitions",
        "slicePartitions",
        "subdivisions",
        "shadows",
        "distanceDisplayCondition"
    ];
    clone: [
        "show",
        "radii",
        "innerRadii",
        "minimumClock",
        "maximumClock",
        "minimumCone",
        "maximumCone",
        "heightReference",
        "fill",
        "material",
        "outline",
        "outlineColor",
        "outlineWidth",
        "stackPartitions",
        "slicePartitions",
        "subdivisions",
        "shadows",
        "distanceDisplayCondition"
    ];
    nested: [

    ];
}
