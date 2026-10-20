//! Cached pin canvases, text/icons and shared asynchronous resource image loading.
use super::canvas::{CanvasCommand as Op, CanvasHost};
use super::network_terrain::TerrainFuture;
use super::write_text_to_canvas::{write_text_to_canvas, WriteTextOptions};
use super::*;
use futures_util::{
    future::{LocalBoxFuture, Shared},
    FutureExt,
};
use std::{cell::RefCell, collections::HashMap, rc::Rc};
type PinFuture<I> = Shared<LocalBoxFuture<'static, Result<I, RuntimeError>>>;
enum Cached<I: Clone> {
    Canvas(I),
    Pending(PinFuture<I>),
}
pub struct PinBuilder<H: CanvasHost>
where
    H::Image: Clone,
{
    host: Rc<H>,
    client: ResourceClient,
    cache: Rc<RefCell<HashMap<String, Cached<H::Image>>>>,
}
impl<H: CanvasHost + 'static> PinBuilder<H>
where
    H::Image: Clone + 'static,
{
    pub fn new(host: Rc<H>, client: ResourceClient) -> Self {
        Self { host, client, cache: Default::default() }
    }
    fn key(url: Option<&str>, label: Option<&str>, color: Color, size: f64) -> String {
        serde_json::json!([url, label, [color.red, color.green, color.blue, color.alpha], size]).to_string()
    }
    pub fn from_color(&self, color: Color, size: f64) -> Result<H::Image, RuntimeError> {
        self.synchronous(None, color, size)
    }
    pub fn from_text(&self, text: &str, color: Color, size: f64) -> Result<H::Image, RuntimeError> {
        self.synchronous(Some(text), color, size)
    }
    fn synchronous(&self, label: Option<&str>, color: Color, size: f64) -> Result<H::Image, RuntimeError> {
        let key = Self::key(None, label, color, size);
        if let Some(Cached::Canvas(c)) = self.cache.borrow().get(&key) {
            return Ok(c.clone());
        }
        let canvas = self.host.create_canvas(size as u32, size as u32)?;
        draw_pin(self.host.as_ref(), &canvas, color, size)?;
        if let Some(label) = label {
            let text = write_text_to_canvas(
                self.host.as_ref(),
                label,
                &WriteTextOptions { font: format!("bold {size}px sans-serif"), ..Default::default() },
            )?
            .ok_or_else(|| RuntimeError::new("Cannot draw an empty pin label."))?;
            draw_icon(self.host.as_ref(), &canvas, &text.canvas, size)?;
        }
        self.cache.borrow_mut().insert(key, Cached::Canvas(canvas.clone()));
        Ok(canvas)
    }
    pub fn from_url(&self, resource: Resource, color: Color, size: f64) -> Option<TerrainFuture<'static, H::Image>> {
        let key = Self::key(Some(&resource.url()), None, color, size);
        if let Some(item) = self.cache.borrow().get(&key) {
            return Some(match item {
                Cached::Canvas(c) => {
                    let c = c.clone();
                    Box::pin(async { Ok(c) })
                }
                Cached::Pending(f) => Box::pin(f.clone()),
            });
        }
        let canvas = match self.host.create_canvas(size as u32, size as u32).and_then(|c| {
            draw_pin(self.host.as_ref(), &c, color, size)?;
            Ok(c)
        }) {
            Ok(c) => c,
            Err(e) => return Some(Box::pin(async { Err(e) })),
        };
        let fetch = self.client.fetch_image(&resource, self.host.clone(), FetchImageOptions::default())?;
        let host = self.host.clone();
        let weak = Rc::downgrade(&self.cache);
        let cache_key = key.clone();
        let future = async move {
            let image = fetch
                .await
                .map_err(super::network_terrain::net_error)?
                .ok_or_else(|| RuntimeError::new("Pin image is empty."))?;
            draw_icon(host.as_ref(), &canvas, &image, size)?;
            if let Some(cache) = weak.upgrade() {
                cache.borrow_mut().insert(cache_key, Cached::Canvas(canvas.clone()));
            }
            Ok(canvas)
        }
        .boxed_local()
        .shared();
        self.cache.borrow_mut().insert(key, Cached::Pending(future.clone()));
        Some(Box::pin(future))
    }
    pub fn from_maki_icon_id(
        &self,
        id: &str,
        color: Color,
        size: f64,
        base: &Resource,
    ) -> Option<TerrainFuture<'static, H::Image>> {
        let r = base.get_derived_resource(super::resource::DerivedResourceOptions {
            url: Some(format!("Assets/Textures/maki/{}.png", super::uri::encode_uri_component(id))),
            ..Default::default()
        })?;
        self.from_url(r, color, size)
    }
}
fn draw_pin<H: CanvasHost>(h: &H, c: &H::Image, color: Color, size: f64) -> Result<(), RuntimeError> {
    let fill = color.to_css_color_string();
    let stroke = color.brighten(0.6).to_css_color_string();
    for op in [
        Op::Save,
        Op::Scale(size / 24., size / 24.),
        Op::FillStyle(&fill),
        Op::StrokeStyle(&stroke),
        Op::LineWidth(0.846),
        Op::BeginPath,
        Op::MoveTo(6.72, 0.422),
        Op::LineTo(17.28, 0.422),
        Op::BezierCurveTo([18.553, 0.422, 19.577, 1.758, 19.577, 3.415]),
        Op::LineTo(19.577, 10.973),
        Op::BezierCurveTo([19.577, 12.63, 18.553, 13.966, 17.282, 13.966]),
        Op::LineTo(14.386, 14.008),
        Op::LineTo(11.826, 23.578),
        Op::LineTo(9.614, 14.008),
        Op::LineTo(6.719, 13.965),
        Op::BezierCurveTo([5.446, 13.983, 4.422, 12.629, 4.422, 10.972]),
        Op::LineTo(4.422, 3.416),
        Op::BezierCurveTo([4.423, 1.76, 5.447, 0.423, 6.718, 0.423]),
        Op::ClosePath,
        Op::Fill,
        Op::Stroke,
        Op::Restore,
    ] {
        h.command(c, op)?;
    }
    Ok(())
}
fn draw_icon<H: CanvasHost>(h: &H, c: &H::Image, image: &H::Image, size: f64) -> Result<(), RuntimeError> {
    let (w, height) = h.dimensions(image);
    let image_size = size / 2.5;
    let (mut sx, mut sy) = (image_size, image_size);
    if w > height {
        sy *= height as f64 / w as f64;
    } else if w < height {
        sx *= w as f64 / height as f64;
    }
    let x = super::math::js_round((size - sx) / 2.);
    let y = super::math::js_round(7. / 24. * size - sy / 2.);
    let black = Color::BLACK.to_css_color_string();
    let white = Color::WHITE.to_css_color_string();
    for op in [
        Op::Composite("destination-out"),
        Op::DrawImage(image, x - 1., y, sx, sy),
        Op::DrawImage(image, x, y - 1., sx, sy),
        Op::DrawImage(image, x + 1., y, sx, sy),
        Op::DrawImage(image, x, y + 1., sx, sy),
        Op::Composite("destination-over"),
        Op::FillStyle(&black),
        Op::FillRect(x - 1., y - 1., sx + 2., sy + 2.),
        Op::Composite("destination-out"),
        Op::DrawImage(image, x, y, sx, sy),
        Op::Composite("destination-over"),
        Op::FillStyle(&white),
        Op::FillRect(x - 1., y - 2., sx + 2., sy + 2.),
    ] {
        h.command(c, op)?;
    }
    Ok(())
}
