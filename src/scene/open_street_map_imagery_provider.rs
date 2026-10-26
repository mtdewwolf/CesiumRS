//! CesiumJS `OpenStreetMapImageryProvider`: tiled imagery hosted by OpenStreetMap or Slippy tile servers.

use super::{ImageryLoader, UrlTemplateImageryProvider, UrlTemplateImageryProviderOptions};
use crate::core::{
    Credit, Ellipsoid, ImageHost, Rectangle, Resource, RuntimeError, TilingScheme, WebMercatorTilingScheme,
};
use std::ops::Deref;
use std::sync::Arc;

pub const DEFAULT_OSM_URL: &str = "https://tile.openstreetmap.org/";
pub const DEFAULT_OSM_CREDIT: &str = "MapQuest, Open Street Map and contributors, CC-BY-SA";

#[derive(Clone, Default)]
pub struct OpenStreetMapImageryProviderOptions {
    pub url: Option<Resource>,
    pub file_extension: Option<String>,
    pub retina_tiles: bool,
    pub rectangle: Option<Rectangle>,
    pub minimum_level: Option<u32>,
    pub maximum_level: Option<u32>,
    pub ellipsoid: Option<Ellipsoid>,
    pub credit: Option<Credit>,
}

pub struct OpenStreetMapImageryProvider<H: ImageHost> {
    inner: UrlTemplateImageryProvider<H>,
}

impl<H: ImageHost + 'static> OpenStreetMapImageryProvider<H> {
    pub fn new(loader: ImageryLoader<H>, options: OpenStreetMapImageryProviderOptions) -> Result<Self, RuntimeError> {
        let resource = options.url.unwrap_or_else(|| Resource::new(DEFAULT_OSM_URL).unwrap());
        resource.append_forward_slash();

        let ext = options.file_extension.as_deref().unwrap_or("png");
        let retina = if options.retina_tiles { "@2x" } else { "" };
        let template_suffix = format!("{{z}}/{{x}}/{{y}}{retina}.{ext}");
        let current_url = resource.url();
        resource.options_mut().url = format!("{current_url}{template_suffix}");

        let ellipsoid = options.ellipsoid.unwrap_or(Ellipsoid::WGS84);
        let tiling_scheme = Arc::new(WebMercatorTilingScheme::new(ellipsoid, 1, 1, None));
        let rectangle = options.rectangle.unwrap_or(*tiling_scheme.rectangle());
        let minimum_level = options.minimum_level.unwrap_or(0);

        let sw_tile = tiling_scheme.position_to_tile_xy(&rectangle.southwest(), minimum_level);
        let ne_tile = tiling_scheme.position_to_tile_xy(&rectangle.northeast(), minimum_level);
        if let (Some(sw), Some(ne)) = (sw_tile, ne_tile) {
            let tile_count = ((ne.x - sw.x).abs() + 1.0) * ((ne.y - sw.y).abs() + 1.0);
            if tile_count > 4.0 {
                return Err(RuntimeError::new(format!(
                    "The rectangle and minimumLevel indicate that there are {tile_count} tiles at the minimum level. Imagery providers with more than four tiles at the minimum level are not supported."
                )));
            }
        }

        let credit = options.credit.unwrap_or_else(|| Credit::new(DEFAULT_OSM_CREDIT, true));

        let mut url_options = UrlTemplateImageryProviderOptions::new(resource);
        url_options.credit = Some(credit);
        url_options.tiling_scheme = Some(tiling_scheme);
        url_options.tile_width = Some(256.0);
        url_options.tile_height = Some(256.0);
        url_options.minimum_level = Some(minimum_level);
        url_options.maximum_level = options.maximum_level;
        url_options.rectangle = Some(rectangle);

        let inner = UrlTemplateImageryProvider::new(loader, url_options)?;
        Ok(Self { inner })
    }
}

impl<H: ImageHost> Deref for OpenStreetMapImageryProvider<H> {
    type Target = UrlTemplateImageryProvider<H>;
    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl<H: ImageHost + 'static> super::ImageryProvider for OpenStreetMapImageryProvider<H> {
    fn rectangle(&self) -> &Rectangle {
        self.inner.rectangle()
    }
    fn tile_width(&self) -> f64 {
        self.inner.tile_width()
    }
    fn tile_height(&self) -> f64 {
        self.inner.tile_height()
    }
    fn maximum_level(&self) -> Option<u32> {
        self.inner.maximum_level()
    }
    fn minimum_level(&self) -> u32 {
        self.inner.minimum_level()
    }
    fn tiling_scheme(&self) -> &dyn TilingScheme {
        self.inner.tiling_scheme()
    }
    fn tile_discard_policy(&self) -> Option<std::rc::Rc<dyn super::TileDiscardPolicy<super::ImageryImage>>> {
        self.inner.tile_discard_policy()
    }
    fn error_event(&self) -> &super::ImageryErrorEvent {
        self.inner.error_event()
    }
    fn credit(&self) -> Option<&Credit> {
        self.inner.credit()
    }
    fn proxy(&self) -> Option<std::sync::Arc<dyn crate::core::Proxy>> {
        self.inner.proxy()
    }
    fn has_alpha_channel(&self) -> bool {
        self.inner.has_alpha_channel()
    }
    fn get_tile_credits(&self, x: i32, y: i32, level: u32) -> Option<Vec<Credit>> {
        self.inner.get_tile_credits(x, y, level)
    }
    fn request_image(&self, x: i32, y: i32, level: u32) -> Option<super::ImageryFuture> {
        self.inner.request_image(x, y, level)
    }
    fn pick_features(
        &self,
        x: i32,
        y: i32,
        level: u32,
        longitude: f64,
        latitude: f64,
    ) -> Option<super::ImageryFeatureFuture> {
        self.inner.pick_features(x, y, level, longitude, latitude)
    }
}
