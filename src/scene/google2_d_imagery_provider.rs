//! CesiumJS `Google2DImageryProvider`: Google Map Tiles 2D sessions, tile
//! URLs and per-zoom viewport attributions.

use super::{
    ImageryErrorEvent, ImageryFeatureFuture, ImageryFuture, ImageryImage, ImageryLoader, ImageryProvider,
    TileDiscardPolicy, UrlTemplateImageryProvider, UrlTemplateImageryProviderOptions,
};
use crate::core::query::QueryValue;
use crate::core::{
    uri::encode_uri_component, Credit, Ellipsoid, GoogleMaps, ImageHost, Rectangle, Resource, RuntimeError,
    TilingScheme, WebMercatorTilingScheme,
};
use serde_json::{json, Value};
use std::{cell::RefCell, collections::HashMap, rc::Rc, sync::Arc};

type AttributionsByLevel = HashMap<u32, Vec<Credit>>;
type SharedAttributions = Rc<RefCell<Option<AttributionsByLevel>>>;

#[derive(Clone)]
pub struct Google2DImageryProviderOptions {
    pub key: String,
    pub session: String,
    pub url: Option<Resource>,
    pub tile_width: f64,
    pub tile_height: f64,
    pub ellipsoid: Option<Ellipsoid>,
    pub minimum_level: Option<u32>,
    pub maximum_level: Option<u32>,
    pub rectangle: Option<Rectangle>,
    pub credit: Option<Credit>,
    pub tile_discard_policy: Option<Rc<dyn TileDiscardPolicy<ImageryImage>>>,
}

impl Google2DImageryProviderOptions {
    pub fn new(key: impl Into<String>, session: impl Into<String>, tile_width: f64, tile_height: f64) -> Self {
        Self {
            key: key.into(),
            session: session.into(),
            url: None,
            tile_width,
            tile_height,
            ellipsoid: None,
            minimum_level: None,
            maximum_level: None,
            rectangle: None,
            credit: None,
            tile_discard_policy: None,
        }
    }
}

/// Options for Cesium's `fromUrl`, which creates a Google tile session.
#[derive(Clone)]
pub struct Google2DFromUrlOptions {
    pub key: Option<String>,
    pub map_type: String,
    pub language: String,
    pub region: String,
    pub overlay_layer_type: Option<String>,
    pub styles: Option<Value>,
    pub url: Option<Resource>,
    pub ellipsoid: Option<Ellipsoid>,
    pub minimum_level: Option<u32>,
    pub maximum_level: Option<u32>,
    pub rectangle: Option<Rectangle>,
    pub credit: Option<Credit>,
}

impl Default for Google2DFromUrlOptions {
    fn default() -> Self {
        Self {
            key: None,
            map_type: "satellite".into(),
            language: "en_US".into(),
            region: "US".into(),
            overlay_layer_type: None,
            styles: None,
            url: None,
            ellipsoid: None,
            minimum_level: None,
            maximum_level: None,
            rectangle: None,
            credit: None,
        }
    }
}

pub struct Google2DImageryProvider<H: ImageHost> {
    resource: Resource,
    loader: ImageryLoader<H>,
    inner: UrlTemplateImageryProvider<H>,
    viewport_url: String,
    maximum_level: u32,
    tile_credits: Option<Vec<Credit>>,
    attributions_by_level: SharedAttributions,
    error_event: ImageryErrorEvent,
}

impl<H: ImageHost + 'static> Google2DImageryProvider<H> {
    pub fn new(loader: ImageryLoader<H>, options: Google2DImageryProviderOptions) -> Result<Self, RuntimeError> {
        let maximum_level = options.maximum_level.unwrap_or(22);
        let resource = options.url.clone().unwrap_or_else(|| GoogleMaps::default().map_tiles_api_endpoint.clone());
        let mut template_url = resource.get_url_component(true, false);
        if !template_url.ends_with('/') {
            template_url.push('/');
        }
        let viewport_url = format!("{template_url}tile/v1/viewport");
        resource.options_mut().url = format!("{template_url}v1/2dtiles/{{z}}/{{x}}/{{y}}");
        resource.set_query_parameters(
            &vec![
                ("session".into(), Some(QueryValue::Single(encode_uri_component(&options.session)))),
                ("key".into(), Some(QueryValue::Single(encode_uri_component(&options.key)))),
            ],
            false,
        );
        let tile_credits = resource.credits();
        let ellipsoid = options.ellipsoid.unwrap_or(Ellipsoid::WGS84);
        let mut inner_options = UrlTemplateImageryProviderOptions::new(resource.clone());
        inner_options.credit = options.credit.clone();
        inner_options.tile_width = Some(options.tile_width);
        inner_options.tile_height = Some(options.tile_height);
        inner_options.tiling_scheme = Some(Arc::new(WebMercatorTilingScheme::new(ellipsoid, 1, 1, None)));
        inner_options.rectangle = options.rectangle;
        inner_options.minimum_level = options.minimum_level;
        inner_options.maximum_level = Some(maximum_level);
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
            tile_credits,
            attributions_by_level: Rc::new(RefCell::new(None)),
            error_event,
        })
    }

    pub async fn from_url(loader: ImageryLoader<H>, options: Google2DFromUrlOptions) -> Result<Self, RuntimeError> {
        let key = options
            .key
            .clone()
            .or_else(|| GoogleMaps::default().default_api_key)
            .ok_or_else(|| RuntimeError::new("options.key or GoogleMaps.defaultApiKey is required."))?;
        let base = options.url.clone().unwrap_or_else(|| GoogleMaps::default().map_tiles_api_endpoint.clone());
        let mut base_url = base.get_url_component(true, false);
        if !base_url.ends_with('/') {
            base_url.push('/');
        }
        let mut body = json!({
            "mapType": options.map_type,
            "overlay": false,
            "language": options.language,
            "region": options.region,
        });
        if options.map_type == "terrain" && options.overlay_layer_type.is_none() {
            body["layerTypes"] = json!(["layerRoadmap"]);
        }
        if let Some(layer_type) = &options.overlay_layer_type {
            body["mapType"] = json!("satellite");
            body["overlay"] = json!(true);
            body["layerTypes"] = json!([layer_type]);
        }
        if let Some(styles) = &options.styles {
            body["styles"] = styles.clone();
        }
        let session_resource = Resource::new(format!("{base_url}v1/createSession")).unwrap();
        session_resource.set_query_parameters(&vec![("key".into(), Some(QueryValue::Single(key.clone())))], false);
        let response = session_resource
            .post(&loader.client, body.to_string().into_bytes())
            .ok_or_else(|| RuntimeError::new("Google createSession request was throttled."))?
            .await
            .map_err(|e| RuntimeError::new(e.to_string()))?
            .ok_or_else(|| RuntimeError::new("Google createSession returned no response."))?;
        let text = response.text().map_err(|e| RuntimeError::new(e.to_string()))?;
        let session: Value = serde_json::from_str(text).map_err(|e| RuntimeError::new(e.to_string()))?;
        let session_id = session["session"]
            .as_str()
            .ok_or_else(|| RuntimeError::new("Google createSession returned no session."))?;
        let tile_width = session["tileWidth"].as_f64().unwrap_or(256.0);
        let tile_height = session["tileHeight"].as_f64().unwrap_or(256.0);
        Self::new(
            loader,
            Google2DImageryProviderOptions {
                key,
                session: session_id.into(),
                url: Some(base),
                tile_width,
                tile_height,
                ellipsoid: options.ellipsoid,
                minimum_level: options.minimum_level,
                maximum_level: options.maximum_level,
                rectangle: options.rectangle,
                credit: options.credit.or_else(|| Some(GoogleMaps::get_default_credit())),
                tile_discard_policy: None,
            },
        )
    }

    pub fn url(&self) -> String {
        self.inner.url()
    }
    pub fn viewport_url(&self) -> &str {
        &self.viewport_url
    }
    pub fn attributions_by_level(&self) -> Option<HashMap<u32, Vec<Credit>>> {
        self.attributions_by_level.borrow().clone()
    }
    pub fn tile_url(&self, x: i32, y: i32, level: u32) -> String {
        self.inner.tile_url(x, y, level)
    }

    /// Fetches the copyright string for every zoom, as Cesium's
    /// `getViewportCredits` does. The level map is cached after success.
    pub async fn get_viewport_credits(&self) -> Result<HashMap<u32, Vec<Credit>>, RuntimeError> {
        let mut futures = Vec::new();
        for level in 0..=self.maximum_level {
            let resource = self
                .resource
                .get_derived_resource(crate::core::resource::DerivedResourceOptions {
                    url: Some(self.viewport_url.clone()),
                    query_parameters: vec![
                        ("zoom".into(), Some(QueryValue::Single(level.to_string()))),
                        ("north".into(), Some(QueryValue::Single("90".into()))),
                        ("south".into(), Some(QueryValue::Single("-90".into()))),
                        ("east".into(), Some(QueryValue::Single("180".into()))),
                        ("west".into(), Some(QueryValue::Single("-180".into()))),
                    ],
                    ..Default::default()
                })
                .ok_or_else(|| RuntimeError::new("Invalid Google viewport resource."))?;
            let client = self.loader.client.clone();
            futures.push(async move {
                let value = client
                    .fetch_json(&resource)
                    .ok_or_else(|| RuntimeError::new("Google viewport request was throttled."))?
                    .await
                    .map_err(|e| RuntimeError::new(e.to_string()))?;
                Ok::<_, RuntimeError>(value)
            });
        }
        let results = futures_util::future::join_all(futures).await;
        let mut attributions = HashMap::new();
        for (level, result) in results.into_iter().enumerate() {
            let value = result?;
            let mut credits = Vec::new();
            if let Some(text) = value["copyright"].as_str().filter(|text| !text.is_empty()) {
                credits.push(Credit::new(text, false));
            }
            attributions.insert(level as u32, credits);
        }
        *self.attributions_by_level.borrow_mut() = Some(attributions.clone());
        Ok(attributions)
    }
}

impl<H: ImageHost + 'static> ImageryProvider for Google2DImageryProvider<H> {
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
                            ("north".into(), Some(QueryValue::Single("90".into()))),
                            ("south".into(), Some(QueryValue::Single("-90".into()))),
                            ("east".into(), Some(QueryValue::Single("180".into()))),
                            ("west".into(), Some(QueryValue::Single("-180".into()))),
                        ],
                        ..Default::default()
                    })
                    .ok_or_else(|| RuntimeError::new("Invalid Google viewport resource."))?;
                let client = loader.client.clone();
                futures.push(async move {
                    let value = client
                        .fetch_json(&viewport)
                        .ok_or_else(|| RuntimeError::new("Google viewport request was throttled."))?
                        .await
                        .map_err(|e| RuntimeError::new(e.to_string()))?;
                    Ok::<_, RuntimeError>(value)
                });
            }
            let results = futures_util::future::join_all(futures).await;
            let mut map = HashMap::new();
            for (level, result) in results.into_iter().enumerate() {
                let value = result?;
                let mut credits = Vec::new();
                if let Some(text) = value["copyright"].as_str().filter(|text| !text.is_empty()) {
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
