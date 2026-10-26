//! CesiumJS `createOsmBuildingsAsync`: constructor helper for the Cesium OSM Buildings tileset.

use super::{
    cesium_3d_tile_style::{Cesium3DTileStyle, StyleInput},
    expression::Expression,
    style_expression::StyleExpression,
};
use crate::core::Color;
use std::rc::Rc;

/// The canonical Ion asset id for Cesium OSM Buildings.
pub const CESIUM_OSM_BUILDINGS_ION_ASSET_ID: u64 = 96188;

/// Generates the default OSM Buildings styling expression string using the given default color.
pub fn default_osm_buildings_style_expression(default_color: Option<Color>) -> String {
    let color_css = default_color.unwrap_or(Color::WHITE).to_css_color_string();
    format!("Boolean(${{feature['cesium#color']}}) ? color(${{feature['cesium#color']}}) : {color_css}")
}

/// Creates the default [`Cesium3DTileStyle`] for Cesium OSM Buildings.
pub fn default_osm_buildings_style(default_color: Option<Color>) -> Cesium3DTileStyle {
    let expr = default_osm_buildings_style_expression(default_color);
    let mut style = Cesium3DTileStyle::default();
    let parsed: Rc<dyn StyleExpression> =
        Rc::new(Expression::new(&expr, None).expect("default OSM style expression is valid"));
    style.set_color(Some(StyleInput::Expression(parsed))).expect("set_color succeeds");
    style
}

/// Options for configuring a Cesium OSM Buildings tileset.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CreateOsmBuildingsOptions {
    /// The default color to use for buildings that do not specify a color tag.
    pub default_color: Option<Color>,

    /// A custom style to use instead of the default tagging style.
    pub style: Option<Cesium3DTileStyle>,

    /// Whether to enable outline rendering.
    pub enable_show_outline: Option<bool>,

    /// Whether outlines are displayed.
    pub show_outline: Option<bool>,
}

impl CreateOsmBuildingsOptions {
    /// Resolves the effective style for the options.
    pub fn resolve_style(&self) -> Cesium3DTileStyle {
        self.style.clone().unwrap_or_else(|| default_osm_buildings_style(self.default_color))
    }
}
