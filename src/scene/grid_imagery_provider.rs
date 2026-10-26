//! CesiumJS `GridImageryProvider`: a procedural wireframe grid drawn once
//! through the host canvas and returned for every tile.

use super::{FrameResource, ImageryErrorEvent, ImageryFuture, ImageryImage, ImageryProvider};
use crate::core::{
    CanvasCommand, CanvasHost, Color, Ellipsoid, GeographicTilingScheme, Rectangle, RuntimeError, TilingScheme,
};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

#[derive(Clone)]
pub struct GridImageryProviderOptions {
    pub tiling_scheme: Option<Arc<dyn TilingScheme>>,
    pub ellipsoid: Option<Ellipsoid>,
    pub cells: u32,
    pub color: Color,
    pub glow_color: Color,
    pub glow_width: f64,
    pub background_color: Color,
    pub tile_width: f64,
    pub tile_height: f64,
    pub canvas_size: f64,
}

impl Default for GridImageryProviderOptions {
    fn default() -> Self {
        Self {
            tiling_scheme: None,
            ellipsoid: None,
            cells: 8,
            color: Color::new(1.0, 1.0, 1.0, 0.4),
            glow_color: Color::new(0.0, 1.0, 0.0, 0.05),
            glow_width: 6.0,
            background_color: Color::new(0.0, 0.5, 0.0, 0.2),
            tile_width: 256.0,
            tile_height: 256.0,
            canvas_size: 256.0,
        }
    }
}

pub struct GridImageryProvider<H: CanvasHost> {
    host: Rc<H>,
    tiling_scheme: Arc<dyn TilingScheme>,
    tile_width: f64,
    tile_height: f64,
    error_event: ImageryErrorEvent,
    canvas: H::Image,
}

/// The exact 2D command sequence Cesium's `_createGridCanvas` issues. Kept
/// public so hosts and tests can inspect the drawing contract without a real
/// browser canvas.
pub fn grid_canvas_commands(options: &GridImageryProviderOptions) -> Vec<OwnedCanvasCommand> {
    let size = options.canvas_size;
    let min_pixel = 0.0;
    let max_pixel = size;
    let mut commands = vec![
        OwnedCanvasCommand::FillStyle(options.background_color.to_css_color_string()),
        OwnedCanvasCommand::FillRect(min_pixel, min_pixel, max_pixel, max_pixel),
        OwnedCanvasCommand::StrokeStyle(options.glow_color.to_css_color_string()),
        OwnedCanvasCommand::LineWidth(options.glow_width),
        OwnedCanvasCommand::StrokeRect(min_pixel, min_pixel, max_pixel, max_pixel),
    ];
    push_grid(&mut commands, options.cells, size);
    commands.push(OwnedCanvasCommand::LineWidth(options.glow_width * 0.5));
    commands.push(OwnedCanvasCommand::StrokeRect(min_pixel, min_pixel, max_pixel, max_pixel));
    push_grid(&mut commands, options.cells, size);
    commands.push(OwnedCanvasCommand::StrokeStyle(options.color.to_css_color_string()));
    commands.push(OwnedCanvasCommand::LineWidth(2.0));
    commands.push(OwnedCanvasCommand::StrokeRect(min_pixel, min_pixel, max_pixel, max_pixel));
    commands.push(OwnedCanvasCommand::LineWidth(1.0));
    push_grid(&mut commands, options.cells, size);
    commands
}

fn push_grid(commands: &mut Vec<OwnedCanvasCommand>, cells: u32, max_pixel: f64) {
    for x in 0..=cells {
        let nx = f64::from(x) / f64::from(cells);
        let val = 1.0 + nx * (max_pixel - 1.0);
        commands.push(OwnedCanvasCommand::MoveTo(val, 0.0));
        commands.push(OwnedCanvasCommand::LineTo(val, max_pixel));
        commands.push(OwnedCanvasCommand::MoveTo(0.0, val));
        commands.push(OwnedCanvasCommand::LineTo(max_pixel, val));
    }
    commands.push(OwnedCanvasCommand::Stroke);
}

/// Owned form of [`CanvasCommand`] for the static grid sequence.
#[derive(Clone, Debug, PartialEq)]
pub enum OwnedCanvasCommand {
    FillStyle(String),
    StrokeStyle(String),
    LineWidth(f64),
    MoveTo(f64, f64),
    LineTo(f64, f64),
    FillRect(f64, f64, f64, f64),
    StrokeRect(f64, f64, f64, f64),
    Stroke,
}

impl<H: CanvasHost + 'static> GridImageryProvider<H>
where
    H::Image: Clone,
{
    pub fn new(host: Rc<H>, options: GridImageryProviderOptions) -> Result<Self, RuntimeError> {
        let tiling_scheme = options.tiling_scheme.clone().unwrap_or_else(|| {
            Arc::new(GeographicTilingScheme::new(
                options.ellipsoid.unwrap_or(Ellipsoid::WGS84),
                Rectangle::MAX_VALUE,
                2,
                1,
            ))
        });
        let canvas = host.create_canvas(options.canvas_size as u32, options.canvas_size as u32)?;
        for command in grid_canvas_commands(&options) {
            match command {
                OwnedCanvasCommand::FillStyle(v) => host.command(&canvas, CanvasCommand::FillStyle(&v))?,
                OwnedCanvasCommand::StrokeStyle(v) => host.command(&canvas, CanvasCommand::StrokeStyle(&v))?,
                OwnedCanvasCommand::LineWidth(v) => host.command(&canvas, CanvasCommand::LineWidth(v))?,
                OwnedCanvasCommand::MoveTo(x, y) => host.command(&canvas, CanvasCommand::MoveTo(x, y))?,
                OwnedCanvasCommand::LineTo(x, y) => host.command(&canvas, CanvasCommand::LineTo(x, y))?,
                OwnedCanvasCommand::FillRect(x, y, w, h) => {
                    host.command(&canvas, CanvasCommand::FillRect(x, y, w, h))?
                }
                OwnedCanvasCommand::StrokeRect(x, y, w, h) => {
                    // The canvas API expresses a stroked rectangle as a path;
                    // hosts receive the same primitive path commands.
                    host.command(&canvas, CanvasCommand::BeginPath)?;
                    host.command(&canvas, CanvasCommand::MoveTo(x, y))?;
                    host.command(&canvas, CanvasCommand::LineTo(x + w, y))?;
                    host.command(&canvas, CanvasCommand::LineTo(x + w, y + h))?;
                    host.command(&canvas, CanvasCommand::LineTo(x, y + h))?;
                    host.command(&canvas, CanvasCommand::ClosePath)?;
                    host.command(&canvas, CanvasCommand::Stroke)?;
                }
                OwnedCanvasCommand::Stroke => host.command(&canvas, CanvasCommand::Stroke)?,
            }
        }
        Ok(Self {
            host,
            tiling_scheme,
            tile_width: options.tile_width,
            tile_height: options.tile_height,
            error_event: Rc::new(RefCell::new(Default::default())),
            canvas,
        })
    }

    pub fn canvas(&self) -> &H::Image {
        &self.canvas
    }
}

impl<H: CanvasHost + 'static> ImageryProvider for GridImageryProvider<H>
where
    H::Image: Clone,
{
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
    fn request_image(&self, _x: i32, _y: i32, _level: u32) -> Option<ImageryFuture> {
        let image = self.canvas.clone();
        let _ = &self.host;
        Some(Box::pin(std::future::ready(Ok(ImageryImage::Image(FrameResource::new(image))))))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grid_commands_match_upstream_order() {
        let options = GridImageryProviderOptions::default();
        let commands = grid_canvas_commands(&options);
        assert!(matches!(&commands[0], OwnedCanvasCommand::FillStyle(v) if v == "rgba(0,128,0,0.2)"));
        assert_eq!(commands.iter().filter(|c| matches!(c, OwnedCanvasCommand::Stroke)).count(), 3);
        // 3 passes × (9 horizontal + 9 vertical) line segments.
        assert_eq!(commands.iter().filter(|c| matches!(c, OwnedCanvasCommand::MoveTo(..))).count(), 54);
    }
}
