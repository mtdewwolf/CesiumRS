//! Host-independent billboard images and their optional canvas renderer.
use crate::core::{CanvasHost, Color, PinBuilder, Resource, RuntimeError};
use serde_json::{json, Value};

#[derive(Clone, Debug, PartialEq)]
pub enum PinSymbol {
    Color,
    Text(String),
    Maki(String),
}

#[derive(Clone, Debug, PartialEq)]
pub struct GeoJsonPin {
    pub symbol: PinSymbol,
    pub color: Color,
    pub size: f64,
}
impl GeoJsonPin {
    pub(crate) fn to_json(&self) -> Value {
        let (kind, symbol) = match &self.symbol {
            PinSymbol::Color => ("color", None),
            PinSymbol::Text(s) => ("text", Some(s)),
            PinSymbol::Maki(s) => ("maki", Some(s)),
        };
        let c = self.color;
        json!({"kind":kind,"symbol":symbol,"size":self.size,"color":[c.red,c.green,c.blue,c.alpha]})
    }
    /// Rasterize on the host, falling back to a solid pin if the Maki image is unavailable.
    pub async fn image<H: CanvasHost + 'static>(
        &self,
        builder: &PinBuilder<H>,
        base: &Resource,
    ) -> Result<H::Image, RuntimeError>
    where
        H::Image: Clone + 'static,
    {
        match &self.symbol {
            PinSymbol::Color => builder.from_color(self.color, self.size),
            PinSymbol::Text(text) => builder.from_text(text, self.color, self.size),
            PinSymbol::Maki(id) => {
                if let Some(image) = builder.from_maki_icon_id(id, self.color, self.size, base) {
                    if let Ok(image) = image.await {
                        return Ok(image);
                    }
                }
                builder.from_color(self.color, self.size)
            }
        }
    }
}
