//! CesiumJS `Azure2DImageryProvider` and the `AzureMapsImageryProvider`
//! name: Azure Maps raster tile URLs and per-zoom attribution.

use super::{
    ImageryErrorEvent, ImageryFeatureFuture, ImageryFuture, ImageryImage, ImageryLoader, ImageryProvider,
    TileDiscardPolicy, UrlTemplateImageryProvider, UrlTemplateImageryProviderOptions,
};
use crate::core::query::QueryValue;
use crate::core::{
    Credit, Ellipsoid, ImageHost, Rectangle, Resource, RuntimeError, TilingScheme, WebMercatorTilingScheme,
};
use serde_json::Value;
use std::{cell::RefCell, collections::HashMap, rc::Rc, sync::Arc};

pub const DEFAULT_AZURE_URL: &str = "https://atlas.microsoft.com/";
pub const DEFAULT_TILESET_ID: &str = "microsoft.imagery";
pub const AZURE_API_VERSION: &str = "2024-04-01";

type AttributionsByLevel = HashMap<u32, Vec<Credit>>;
type SharedAttributions = Rc<RefCell<Option<AttributionsByLevel>>>;

#[derive(Clone)]
pub struct Azure2DImageryProviderOptions {
    /// Also accepted under the alias `subscription-key`.
    pub subscription_key: String,
    pub url: Option<Resource>,
    pub tileset_id: Option<String>,
    pub ellipsoid: Option<Ellipsoid>,
    pub minimum_level: Option<u32>,
    pub maximum_level: Option<u32>,
    pub rectangle: Option<Rectangle>,
    pub tile_width: Option<f64>,
    pub tile_height: Option<f64>,
    pub credit: Option<Credit>,
    pub tile_discard_policy: Option<Rc<dyn TileDiscardPolicy<ImageryImage>>>,
}

impl Azure2DImageryProviderOptions {
    pub fn new(subscription_key: impl Into<String>) -> Self {
        Self {
            subscription_key: subscription_key.into(),
            url: None,
            tileset_id: None,
            ellipsoid: None,
            minimum_level: None,
            maximum_level: None,
            rectangle: None,
            tile_width: None,
            tile_height: None,
            credit: None,
            tile_discard_policy: None,
        }
    }
}

pub struct Azure2DImageryProvider<H: ImageHost> {
    resource: Resource,
    loader: ImageryLoader<H>,
    inner: UrlTemplateImageryProvider<H>,
    viewport_url: String,
    maximum_level: u32,
    subscription_key: String,
    tileset_id: String,
    tile_credits: Option<Vec<Credit>>,
    attributions_by_level: SharedAttributions,
    error_event: ImageryErrorEvent,
}

impl<H: ImageHost + 'static> Azure2DImageryProvider<H> {
    pub fn new(loader: ImageryLoader<H>, options: Azure2DImageryProviderOptions) -> Result<Self, RuntimeError> {
        let maximum_level = options.maximum_level.unwrap_or(22);
        let tileset_id = options.tileset_id.clone().unwrap_or_else(|| DEFAULT_TILESET_ID.into());
        let resource = options.url.clone().unwrap_or_else(|| Resource::new(DEFAULT_AZURE_URL).unwrap());
        let mut template_url = resource.get_url_component(true, false);
        if !template_url.ends_with('/') {
            template_url.push('/');
        }
        let viewport_url = format!("{template_url}map/attribution");
        resource.options_mut().url = format!("{template_url}map/tile");
        resource.set_query_parameters(
            &vec![
                ("api-version".into(), Some(QueryValue::Single(AZURE_API_VERSION.into()))),
                ("tilesetId".into(), Some(QueryValue::Single(tileset_id.clone()))),
                ("subscription-key".into(), Some(QueryValue::Single(options.subscription_key.clone()))),
                ("zoom".into(), Some(QueryValue::Single("{z}".into()))),
                ("x".into(), Some(QueryValue::Single("{x}".into()))),
                ("y".into(), Some(QueryValue::Single("{y}".into()))),
            ],
            false,
        );
        let tile_credits = resource.credits();
        let ellipsoid = options.ellipsoid.unwrap_or(Ellipsoid::WGS84);
        let mut inner_options = UrlTemplateImageryProviderOptions::new(resource.clone());
        inner_options.tile_width = options.tile_width;
        inner_options.tile_height = options.tile_height;
        inner_options.tiling_scheme = Some(Arc::new(WebMercatorTilingScheme::new(ellipsoid, 1, 1, None)));
        inner_options.rectangle = options.rectangle;
        inner_options.minimum_level = options.minimum_level;
        inner_options.maximum_level = Some(maximum_level);
        inner_options.credit = options.credit;
        inner_options.tile_discard_policy = options.tile_discard_policy;
        let inner = UrlTemplateImageryProvider::new(
            ImageryLoader::new(loader.client.clone(), loader.host.clone()),
            inner_options,
        )?;
        let error_event = inner.error_event().clone();
        Ok(Self {
            resource,
            loader,
            inner,
            viewport_url,
            maximum_level,
            subscription_key: options.subscription_key,
            tileset_id,
            tile_credits,
            attributions_by_level: Rc::new(RefCell::new(None)),
            error_event,
        })
    }

    pub fn url(&self) -> String {
        self.inner.url()
    }
    pub fn viewport_url(&self) -> &str {
        &self.viewport_url
    }
    pub fn subscription_key(&self) -> &str {
        &self.subscription_key
    }
    pub fn tileset_id(&self) -> &str {
        &self.tileset_id
    }
    pub fn attributions_by_level(&self) -> Option<HashMap<u32, Vec<Credit>>> {
        self.attributions_by_level.borrow().clone()
    }
    pub fn tile_url(&self, x: i32, y: i32, level: u32) -> String {
        self.inner.tile_url(x, y, level)
    }

    pub async fn get_viewport_credits(&self) -> Result<HashMap<u32, Vec<Credit>>, RuntimeError> {
        let mut futures = Vec::new();
        for level in 0..=self.maximum_level {
            let resource = self
                .resource
                .get_derived_resource(crate::core::resource::DerivedResourceOptions {
                    url: Some(self.viewport_url.clone()),
                    query_parameters: vec![
                        ("zoom".into(), Some(QueryValue::Single(level.to_string()))),
                        ("bounds".into(), Some(QueryValue::Single("-180,-90,180,90".into()))),
                    ],
                    ..Default::default()
                })
                .ok_or_else(|| RuntimeError::new("Invalid Azure attribution resource."))?;
            let client = self.loader.client.clone();
            futures.push(async move {
                client
                    .fetch_json(&resource)
                    .ok_or_else(|| RuntimeError::new("Azure attribution request was throttled."))?
                    .await
                    .map_err(|e| RuntimeError::new(e.to_string()))
            });
        }
        let results = futures_util::future::join_all(futures).await;
        let mut map = HashMap::new();
        for (level, result) in results.into_iter().enumerate() {
            let value: Value = result?;
            let text = value["copyrights"]
                .as_array()
                .map(|values| values.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(","))
                .unwrap_or_default();
            let mut credits = Vec::new();
            if !text.is_empty() {
                credits.push(Credit::new(text, false));
            }
            map.insert(level as u32, credits);
        }
        *self.attributions_by_level.borrow_mut() = Some(map.clone());
        Ok(map)
    }
}

/// Task-facing alias; upstream's class is `Azure2DImageryProvider`.
pub type AzureMapsImageryProvider<H> = Azure2DImageryProvider<H>;

impl<H: ImageHost + 'static> ImageryProvider for Azure2DImageryProvider<H> {
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
    fn tile_discard_policy(&self) -> Option<Rc<dyn TileDiscardPolicy<ImageryImage>>> {
        self.inner.tile_discard_policy()
    }
    fn error_event(&self) -> &ImageryErrorEvent {
        &self.error_event
    }
    fn credit(&self) -> Option<&Credit> {
        self.inner.credit()
    }
    fn proxy(&self) -> Option<Arc<dyn crate::core::Proxy>> {
        self.inner.proxy()
    }
    fn has_alpha_channel(&self) -> bool {
        self.inner.has_alpha_channel()
    }
    fn get_tile_credits(&self, _x: i32, _y: i32, level: u32) -> Option<Vec<Credit>> {
        let attributions = self.attributions_by_level.borrow();
        let attributions = attributions.as_ref()?;
        let Some(tile_credits) = &self.tile_credits else {
            return None;
        };
        let inner = attributions.get(&level).cloned().unwrap_or_default();
        Some(tile_credits.iter().chain(inner.iter()).cloned().collect())
    }
    fn request_image(&self, x: i32, y: i32, level: u32) -> Option<ImageryFuture> {
        let image = self.inner.request_image(x, y, level)?;
        if self.attributions_by_level.borrow().is_some() {
            return Some(image);
        }
        let loader = ImageryLoader::new(self.loader.client.clone(), self.loader.host.clone());
        let resource = self.resource.clone();
        let viewport_url = self.viewport_url.clone();
        let maximum_level = self.maximum_level;
        let attributions = self.attributions_by_level.clone();
        Some(Box::pin(async move {
            let image = image.await?;
            let mut futures = Vec::new();
            for level in 0..=maximum_level {
                let viewport = resource
                    .get_derived_resource(crate::core::resource::DerivedResourceOptions {
                        url: Some(viewport_url.clone()),
                        query_parameters: vec![
                            ("zoom".into(), Some(QueryValue::Single(level.to_string()))),
                            ("bounds".into(), Some(QueryValue::Single("-180,-90,180,90".into()))),
                        ],
                        ..Default::default()
                    })
                    .ok_or_else(|| RuntimeError::new("Invalid Azure attribution resource."))?;
                let client = loader.client.clone();
                futures.push(async move {
                    client
                        .fetch_json(&viewport)
                        .ok_or_else(|| RuntimeError::new("Azure attribution request was throttled."))?
                        .await
                        .map_err(|e| RuntimeError::new(e.to_string()))
                });
            }
            let results = futures_util::future::join_all(futures).await;
            let mut map = HashMap::new();
            for (level, result) in results.into_iter().enumerate() {
                let value: Value = result?;
                let text = value["copyrights"]
                    .as_array()
                    .map(|values| values.iter().filter_map(Value::as_str).collect::<Vec<_>>().join(","))
                    .unwrap_or_default();
                let mut credits = Vec::new();
                if !text.is_empty() {
                    credits.push(Credit::new(text, false));
                }
                map.insert(level as u32, credits);
            }
            *attributions.borrow_mut() = Some(map);
            Ok(image)
        }))
    }
    fn pick_features(
        &self,
        _x: i32,
        _y: i32,
        _level: u32,
        _longitude: f64,
        _latitude: f64,
    ) -> Option<ImageryFeatureFuture> {
        None
    }
}
