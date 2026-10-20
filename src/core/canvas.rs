//! Canvas drawing capabilities used by text and pin algorithms; hosts own rasterization.
use super::{ImageHost, RuntimeError};
#[derive(Clone, Copy, Debug)]
pub struct CanvasTextMetrics {
    pub width: f64,
    pub ascent: f64,
    pub descent: f64,
    pub left: f64,
}
pub enum CanvasCommand<'a, I> {
    Save,
    Restore,
    Scale(f64, f64),
    Font(&'a str),
    LineJoin(&'a str),
    LineWidth(f64),
    ImageSmoothing(bool),
    FillStyle(&'a str),
    StrokeStyle(&'a str),
    Composite(&'a str),
    BeginPath,
    ClosePath,
    MoveTo(f64, f64),
    LineTo(f64, f64),
    BezierCurveTo([f64; 6]),
    Fill,
    Stroke,
    FillRect(f64, f64, f64, f64),
    FillText(&'a str, f64, f64),
    StrokeText(&'a str, f64, f64),
    DrawImage(&'a I, f64, f64, f64, f64),
}
pub trait CanvasHost: ImageHost {
    fn create_canvas(&self, width: u32, height: u32) -> Result<Self::Image, RuntimeError>;
    fn resize_canvas(&self, canvas: &Self::Image, width: u32, height: u32) -> Result<(), RuntimeError>;
    fn command(&self, canvas: &Self::Image, command: CanvasCommand<'_, Self::Image>) -> Result<(), RuntimeError>;
    /// Measure while attached with hidden visibility, preserving browser font layout.
    fn measure_text(&self, canvas: &Self::Image, text: &str, font: &str) -> Result<CanvasTextMetrics, RuntimeError>;
}
