//! Cesium text metrics, stroke padding, canvas sizing and drawing through CanvasHost.
use super::canvas::{CanvasCommand as Op, CanvasHost};
use super::{Color, RuntimeError};
#[derive(Clone, Debug)]
pub struct WriteTextOptions {
    pub font: String,
    pub stroke: bool,
    pub fill: bool,
    pub stroke_width: f64,
    pub background_color: Color,
    pub padding: f64,
    pub stroke_color: Color,
    pub fill_color: Color,
}
impl Default for WriteTextOptions {
    fn default() -> Self {
        Self {
            font: "10px sans-serif".into(),
            stroke: false,
            fill: true,
            stroke_width: 1.,
            background_color: Color::TRANSPARENT,
            padding: 0.,
            stroke_color: Color::BLACK,
            fill_color: Color::WHITE,
        }
    }
}
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextDimensions {
    pub width: f64,
    pub height: f64,
    pub ascent: f64,
    pub descent: f64,
    pub min_x: f64,
}
pub struct TextCanvas<I> {
    pub canvas: I,
    pub dimensions: TextDimensions,
}
pub fn write_text_to_canvas<H: CanvasHost>(
    host: &H,
    text: &str,
    o: &WriteTextOptions,
) -> Result<Option<TextCanvas<H::Image>>, RuntimeError> {
    if text.is_empty() {
        return Ok(None);
    }
    let canvas = host.create_canvas(1, 1)?;
    let setup = || -> Result<(), RuntimeError> {
        host.command(&canvas, Op::Font(&o.font))?;
        host.command(&canvas, Op::LineJoin("round"))?;
        host.command(&canvas, Op::LineWidth(o.stroke_width))?;
        host.command(&canvas, Op::ImageSmoothing(false))
    };
    setup()?;
    let metrics = host.measure_text(&canvas, text, &o.font)?;
    let space = text.chars().all(js_space);
    let dimensions = if space {
        TextDimensions { width: metrics.width, height: 0., ascent: 0., descent: 0., min_x: 0. }
    } else {
        let ascent = super::math::js_round(metrics.ascent);
        let descent = super::math::js_round(metrics.descent);
        TextDimensions {
            width: metrics.width,
            height: (ascent + descent).max(1.),
            ascent,
            descent,
            min_x: -super::math::js_round(metrics.left),
        }
    };
    let stroke_padding = if o.stroke && !space { (o.stroke_width / 2.).ceil() } else { 0. };
    let x = -dimensions.min_x + stroke_padding;
    let width = dimensions.width.ceil() + x + o.padding * 2. + stroke_padding;
    let height = dimensions.height + o.padding * 2. + stroke_padding * 2.;
    let y = dimensions.ascent + o.padding + stroke_padding;
    host.resize_canvas(&canvas, width as u32, height as u32)?;
    setup()?;
    if o.background_color != Color::TRANSPARENT {
        host.command(&canvas, Op::FillStyle(&o.background_color.to_css_color_string()))?;
        host.command(&canvas, Op::FillRect(0., 0., width.trunc(), height.trunc()))?;
    }
    if o.stroke {
        host.command(&canvas, Op::StrokeStyle(&o.stroke_color.to_css_color_string()))?;
        host.command(&canvas, Op::StrokeText(text, x + o.padding, y))?;
    }
    if o.fill {
        host.command(&canvas, Op::FillStyle(&o.fill_color.to_css_color_string()))?;
        host.command(&canvas, Op::FillText(text, x + o.padding, y))?;
    }
    Ok(Some(TextCanvas { canvas, dimensions }))
}
fn js_space(c: char) -> bool {
    matches!(c,'\u{0009}'..='\u{000d}'|'\u{0020}'|'\u{00a0}'|'\u{1680}'|'\u{2000}'..='\u{200a}'|'\u{2028}'|'\u{2029}'|'\u{202f}'|'\u{205f}'|'\u{3000}'|'\u{feff}')
}
