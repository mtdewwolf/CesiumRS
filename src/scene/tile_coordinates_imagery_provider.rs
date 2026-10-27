//! CesiumJS `TileCoordinatesImageryProvider`: a procedural tile border and
//! the tile's level/X/Y labels, drawn through the host canvas for each tile.

use super::{FrameResource, ImageryErrorEvent, ImageryFuture, ImageryImage, ImageryProvider};
use crate::core::{
    CanvasCommand, CanvasHost, Color, Ellipsoid, GeographicTilingScheme, Rectangle, RuntimeError, TilingScheme,
};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

#[derive(Clone)]
pub struct TileCoordinatesImageryProviderOptions {
    pub tiling_scheme: Option<Arc<dyn TilingScheme>>,
    pub ellipsoid: Option<Ellipsoid>,
    pub color: Color,
    pub tile_width: f64,
    pub tile_height: f64,
}

impl Default for TileCoordinatesImageryProviderOptions {
    fn default() -> Self {
        Self { tiling_scheme: None, ellipsoid: None, color: Color::YELLOW, tile_width: 256.0, tile_height: 256.0 }
    }
}

pub struct TileCoordinatesImageryProvider<H: CanvasHost> {
    host: Rc<H>,
    tiling_scheme: Arc<dyn TilingScheme>,
    color: Color,
    tile_width: f64,
    tile_height: f64,
    error_event: ImageryErrorEvent,
}

impl<H: CanvasHost + 'static> TileCoordinatesImageryProvider<H> {
    pub fn new(host: Rc<H>, options: TileCoordinatesImageryProviderOptions) -> Self {
        let tiling_scheme = options.tiling_scheme.unwrap_or_else(|| {
            Arc::new(GeographicTilingScheme::new(
                options.ellipsoid.unwrap_or(Ellipsoid::WGS84),
                Rectangle::MAX_VALUE,
                2,
                1,
            ))
        });
        Self {
            host,
            tiling_scheme,
            color: options.color,
            tile_width: options.tile_width,
            tile_height: options.tile_height,
            error_event: Rc::new(RefCell::new(Default::default())),
        }
    }

    /// Draws one tile canvas. The host owns rasterization; the same six text
    /// and border operations as Cesium are issued.
    pub fn draw(&self, x: i32, y: i32, level: u32) -> Result<H::Image, RuntimeError> {
        let canvas = self.host.create_canvas(256, 256)?;
        let css = self.color.to_css_color_string();
        self.host.command(&canvas, CanvasCommand::StrokeStyle(&css))?;
        self.host.command(&canvas, CanvasCommand::LineWidth(2.0))?;
        self.host.command(&canvas, CanvasCommand::BeginPath)?;
        self.host.command(&canvas, CanvasCommand::MoveTo(1.0, 1.0))?;
        self.host.command(&canvas, CanvasCommand::LineTo(256.0, 1.0))?;
        self.host.command(&canvas, CanvasCommand::LineTo(256.0, 256.0))?;
        self.host.command(&canvas, CanvasCommand::LineTo(1.0, 256.0))?;
        self.host.command(&canvas, CanvasCommand::ClosePath)?;
        self.host.command(&canvas, CanvasCommand::Stroke)?;

        let font = "bold 25px Arial";
        self.host.command(&canvas, CanvasCommand::Font(font))?;
        self.host.command(&canvas, CanvasCommand::FillStyle(&css))?;
        for (text, baseline) in [(format!("L: {level}"), 86.0), (format!("X: {x}"), 136.0), (format!("Y: {y}"), 186.0)]
        {
            // `textAlign = "center"`: the host has no alignment command, so
            // measure the font and center the glyph run at Cesium's x=124.
            let width = self.host.measure_text(&canvas, &text, font)?.width;
            self.host.command(&canvas, CanvasCommand::FillText(&text, 124.0 - width / 2.0, baseline))?;
        }
        Ok(canvas)
    }
}

impl<H: CanvasHost + 'static> ImageryProvider for TileCoordinatesImageryProvider<H> {
    fn rectangle(&self) -> &Rectangle {
        self.tiling_scheme.rectangle()
    }
    fn tile_width(&self) -> f64 {
        self.tile_width
    }
    fn tile_height(&self) -> f64 {
        self.tile_height
    }
    fn maximum_level(&self) -> Option<u32> {
        None
    }
    fn minimum_level(&self) -> u32 {
        0
    }
    fn tiling_scheme(&self) -> &dyn TilingScheme {
        self.tiling_scheme.as_ref()
    }
    fn error_event(&self) -> &ImageryErrorEvent {
        &self.error_event
    }
    fn has_alpha_channel(&self) -> bool {
        true
    }
    fn request_image(&self, x: i32, y: i32, level: u32) -> Option<ImageryFuture> {
        let result = self.draw(x, y, level);
        Some(Box::pin(std::future::ready(result.map(|image| ImageryImage::Image(FrameResource::new(image))))))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_upstream_values() {
        let options = TileCoordinatesImageryProviderOptions::default();
        assert_eq!(options.color, Color::YELLOW);
        assert_eq!((options.tile_width, options.tile_height), (256.0, 256.0));
    }
}
