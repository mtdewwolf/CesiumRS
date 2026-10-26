//! CesiumJS `Cesium3DTilesetGraphics`: A 3D Tiles tileset attached to an entity.
//!
//! Property slots begin undefined; visualizer defaults are applied when the
//! graphics object is drawn, not in this constructor. See `graphics.rs` for
//! the shared definition, event, merge and clone contracts.
use super::graphics::graphics_definition;
use super::{ComponentRef, DefinitionChanged, DynamicValue, EntityComponent, PropertyInput};
use crate::core::RuntimeError;
use std::any::Any;
graphics_definition! {
    Cesium3DTilesetGraphics;
    properties: [
        "show" => show, set_show,
        "uri" => uri, set_uri,
        "maximumScreenSpaceError" => maximum_screen_space_error, set_maximum_screen_space_error
    ];
    merge: [
        "show",
        "uri",
        "maximumScreenSpaceError"
    ];
    clone: [
        "show",
        "uri",
        "maximumScreenSpaceError"
    ];
    nested: [

    ];
}
