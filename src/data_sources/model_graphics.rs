//! CesiumJS `ModelGraphics`: A glTF model attached to an entity.
//!
//! Property slots begin undefined; visualizer defaults are applied when the
//! graphics object is drawn, not in this constructor. See `graphics.rs` for
//! the shared definition, event, merge and clone contracts.
use super::graphics::graphics_definition;
use super::{ComponentRef, DefinitionChanged, DynamicValue, EntityComponent, PropertyInput};
use crate::core::RuntimeError;
use std::any::Any;
graphics_definition! {
    ModelGraphics;
    properties: [
        "show" => show, set_show,
        "uri" => uri, set_uri,
        "scale" => scale, set_scale,
        "enableVerticalExaggeration" => enable_vertical_exaggeration, set_enable_vertical_exaggeration,
        "minimumPixelSize" => minimum_pixel_size, set_minimum_pixel_size,
        "maximumScale" => maximum_scale, set_maximum_scale,
        "incrementallyLoadTextures" => incrementally_load_textures, set_incrementally_load_textures,
        "runAnimations" => run_animations, set_run_animations,
        "clampAnimations" => clamp_animations, set_clamp_animations,
        "shadows" => shadows, set_shadows,
        "heightReference" => height_reference, set_height_reference,
        "silhouetteColor" => silhouette_color, set_silhouette_color,
        "silhouetteSize" => silhouette_size, set_silhouette_size,
        "color" => color, set_color,
        "colorBlendMode" => color_blend_mode, set_color_blend_mode,
        "colorBlendAmount" => color_blend_amount, set_color_blend_amount,
        "imageBasedLightingFactor" => image_based_lighting_factor, set_image_based_lighting_factor,
        "environmentMapOptions" => environment_map_options, set_environment_map_options,
        "lightColor" => light_color, set_light_color,
        "distanceDisplayCondition" => distance_display_condition, set_distance_display_condition,
        "nodeTransformations" => node_transformations, set_node_transformations,
        "articulations" => articulations, set_articulations,
        "clippingPlanes" => clipping_planes, set_clipping_planes,
        "customShader" => custom_shader, set_custom_shader
    ];
    merge: [
        "show",
        "uri",
        "scale",
        "enableVerticalExaggeration",
        "minimumPixelSize",
        "maximumScale",
        "incrementallyLoadTextures",
        "runAnimations",
        "clampAnimations",
        "shadows",
        "heightReference",
        "silhouetteColor",
        "silhouetteSize",
        "color",
        "colorBlendMode",
        "colorBlendAmount",
        "imageBasedLightingFactor",
        "environmentMapOptions",
        "lightColor",
        "distanceDisplayCondition",
        "clippingPlanes",
        "customShader",
        "nodeTransformations",
        "articulations"
    ];
    clone: [
        "show",
        "uri",
        "scale",
        "enableVerticalExaggeration",
        "minimumPixelSize",
        "maximumScale",
        "incrementallyLoadTextures",
        "runAnimations",
        "clampAnimations",
        "heightReference",
        "silhouetteColor",
        "silhouetteSize",
        "color",
        "colorBlendMode",
        "colorBlendAmount",
        "imageBasedLightingFactor",
        "environmentMapOptions",
        "lightColor",
        "distanceDisplayCondition",
        "nodeTransformations",
        "articulations",
        "clippingPlanes",
        "customShader"
    ];
    nested: [
        "nodeTransformations",
        "articulations"
    ];
}
