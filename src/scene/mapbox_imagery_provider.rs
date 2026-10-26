//! CesiumJS `MapboxImageryProvider`: tiled imagery hosted by Mapbox.

use super::{ImageryLoader, UrlTemplateImageryProvider, UrlTemplateImageryProviderOptions};
use crate::core::query::QueryValue;
use crate::core::{
    Credit, Ellipsoid, ImageHost, Rectangle, Resource, RuntimeError, TilingScheme, WebMercatorTilingScheme,
};
use std::ops::Deref;
use std::sync::Arc;

pub const DEFAULT_MAPBOX_URL: &str = "https://{s}.tiles.mapbox.com/v4/";
pub const DEFAULT_MAPBOX_CREDIT: &str =
    "&copy; <a href=\"https://www.mapbox.com/about/maps/\">Mapbox</a> &copy; <a href=\"http://www.openstreetmap.org/copyright\">OpenStreetMap</a> <strong><a href=\"https://www.mapbox.com/map-feedback/\">Improve this map</a></strong>";

#[derive(Clone)]
pub struct MapboxImageryProviderOptions {
    pub url: Option<Resource>,
    pub map_id: String,
    pub access_token: String,
    pub format: Option<String>,
    pub rectangle: Option<Rectangle>,
    pub minimum_level: Option<u32>,
    pub maximum_level: Option<u32>,
    pub ellipsoid: Option<Ellipsoid>,
    pub credit: Option<Credit>,
}

impl MapboxImageryProviderOptions {
    pub fn new(map_id: impl Into<String>, access_token: impl Into<String>) -> Self {
        Self {
            url: None,
            map_id: map_id.into(),
            access_token: access_token.into(),
            format: None,
            rectangle: None,
            minimum_level: None,
            maximum_level: None,
            ellipsoid: None,
            credit: None,
        }
    }
}

pub struct MapboxImageryProvider<H: ImageHost> {
    inner: UrlTemplateImageryProvider<H>,
    map_id: String,
    access_token: String,
    format: String,
}

impl<H: ImageHost + 'static> MapboxImageryProvider<H> {
    pub fn new(loader: ImageryLoader<H>, options: MapboxImageryProviderOptions) -> Result<Self, RuntimeError> {
        let resource = options.url.unwrap_or_else(|| Resource::new(DEFAULT_MAPBOX_URL).unwrap());

        let mut format = options.format.unwrap_or_else(|| "png".to_owned());
        if !format.starts_with('.') {
            format = format!(".{format}");
        }

        let mut template_url = resource.get_url_component(true, false);
        if !template_url.ends_with('/') {
            template_url.push('/');
        }
        let map_id = options.map_id;
        template_url = format!("{template_url}{map_id}/{{z}}/{{x}}/{{y}}{format}");
        resource.options_mut().url = template_url;
        resource.append_query_parameters(&vec![(
            "access_token".into(),
            Some(QueryValue::Single(options.access_token.clone())),
        )]);

        let credit = options.credit.unwrap_or_else(|| Credit::new(DEFAULT_MAPBOX_CREDIT, true));

        let ellipsoid = options.ellipsoid.unwrap_or(Ellipsoid::WGS84);
        let tiling_scheme = Arc::new(WebMercatorTilingScheme::new(ellipsoid, 1, 1, None));

        let mut url_options = UrlTemplateImageryProviderOptions::new(resource);
        url_options.credit = Some(credit);
        url_options.tiling_scheme = Some(tiling_scheme);
        url_options.minimum_level = options.minimum_level;
        url_options.maximum_level = options.maximum_level;
        url_options.rectangle = options.rectangle;

        let inner = UrlTemplateImageryProvider::new(loader, url_options)?;
        Ok(Self { inner, map_id, access_token: options.access_token, format })
    }

    pub fn map_id(&self) -> &str {
        &self.map_id
    }

    pub fn access_token(&self) -> &str {
        &self.access_token
    }

    pub fn format(&self) -> &str {
        &self.format
    }
}

impl<H: ImageHost> Deref for MapboxImageryProvider<H> {
    type Target = UrlTemplateImageryProvider<H>;
    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl<H: ImageHost + 'static> super::ImageryProvider for MapboxImageryProvider<H> {
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
