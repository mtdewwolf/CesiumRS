//! CesiumJS `IonImageryProvider` and `IonImageryProviderFactory`: resolve a
//! Cesium ion imagery asset and build the provider named by its endpoint.

use super::arc_gis_map_server_imagery_provider::{
    ArcGisMapServerImageryProvider, ArcGisMapServerImageryProviderOptions,
};
use super::azure2_d_imagery_provider::{Azure2DImageryProvider, Azure2DImageryProviderOptions};
use super::bing_maps_imagery_provider::{BingMapsImageryProvider, BingMapsImageryProviderOptions};
use super::google2_d_imagery_provider::{Google2DImageryProvider, Google2DImageryProviderOptions};
use super::google_earth_enterprise_maps_provider::{
    GoogleEarthEnterpriseMapsProvider, GoogleEarthEnterpriseMapsProviderOptions,
};
use super::tile_map_service_imagery_provider::{TileMapServiceImageryProvider, TileMapServiceImageryProviderOptions};
use super::web_map_service_imagery_provider::{WebMapServiceImageryProvider, WebMapServiceImageryProviderOptions};
use super::web_map_tile_service_imagery_provider::{
    WebMapTileServiceImageryProvider, WebMapTileServiceImageryProviderOptions,
};
use super::{
    BingMapsStyle, ImageryErrorEvent, ImageryFeatureFuture, ImageryFuture, ImageryImage, ImageryLoader,
    ImageryProvider, MapboxImageryProvider, MapboxImageryProviderOptions, SingleTileImageryProvider, TileDiscardPolicy,
    UrlTemplateImageryProvider, UrlTemplateImageryProviderOptions,
};
use crate::core::{
    Credit, ImageHost, Ion, IonAssetOptions, IonResource, Rectangle, Resource, RuntimeError, TilingScheme,
};
use serde_json::Value;
use std::{cell::RefCell, rc::Rc, sync::Arc};

#[derive(Default)]
pub struct IonImageryProviderOptions {
    pub access_token: Option<String>,
    pub server: Option<Resource>,
    pub ion: Ion,
    pub tile_width: Option<f64>,
    pub tile_height: Option<f64>,
    pub minimum_level: Option<u32>,
    pub maximum_level: Option<u32>,
    pub rectangle: Option<Rectangle>,
    pub credit: Option<Credit>,
}

pub struct IonImageryProvider {
    inner: Box<dyn ImageryProvider>,
    tile_credits: Vec<Credit>,
    error_event: ImageryErrorEvent,
}

impl IonImageryProvider {
    /// Cesium's `IonImageryProvider.fromAssetId`. The endpoint must have type
    /// `IMAGERY`; unsupported external types are reported by name.
    pub async fn from_asset_id<H: ImageHost + 'static>(
        loader: ImageryLoader<H>,
        asset_id: u64,
        options: IonImageryProviderOptions,
    ) -> Result<Self, RuntimeError> {
        let endpoint_resource = IonResource::create_endpoint_resource(
            asset_id,
            &options.ion,
            &IonAssetOptions {
                access_token: options.access_token.clone(),
                server: options.server.clone(),
                query_parameters: Vec::new(),
            },
        );
        let endpoint = loader
            .client
            .fetch_json(&endpoint_resource)
            .ok_or_else(|| RuntimeError::new("Cesium ion endpoint request was throttled."))?
            .await
            .map_err(|e| RuntimeError::new(e.to_string()))?;
        if endpoint["type"].as_str() != Some("IMAGERY") {
            return Err(RuntimeError::new(format!("Cesium ion asset {asset_id} is not an imagery asset.")));
        }
        let tile_credits = IonResource::get_credits_from_endpoint(&endpoint, &endpoint_resource);
        let endpoint_options = endpoint.get("options").cloned().unwrap_or(Value::Null);
        let external_type = endpoint["externalType"].as_str();
        // External assets carry their service URL in `options.url`; native
        // TMS imagery assets carry it at the endpoint's top level.
        let url = endpoint_options["url"].as_str().or_else(|| endpoint["url"].as_str()).unwrap_or("").to_owned();
        let resource = ion_resource(&url, &endpoint_resource);

        let inner: Box<dyn ImageryProvider> = match external_type {
            Some("BING") => {
                let mut bing_options =
                    BingMapsImageryProviderOptions::new(endpoint_options["key"].as_str().unwrap_or(""));
                bing_options.map_style = endpoint_options["mapStyle"]
                    .as_str()
                    .and_then(BingMapsStyle::from_str_value)
                    .unwrap_or(BingMapsStyle::Aerial);
                bing_options.map_layer = endpoint_options["mapLayer"].as_str().map(str::to_owned);
                bing_options.culture = endpoint_options["culture"].as_str().map(str::to_owned);
                Box::new(
                    BingMapsImageryProvider::from_url(
                        ImageryLoader::new(loader.client.clone(), loader.host.clone()),
                        resource,
                        bing_options,
                    )
                    .await?,
                )
            }
            Some("ARCGIS_MAPSERVER") => Box::new(
                ArcGisMapServerImageryProvider::from_url(
                    ImageryLoader::new(loader.client.clone(), loader.host.clone()),
                    resource,
                    arcgis_options(&endpoint_options),
                )
                .await?,
            ),
            Some("WMS") => Box::new(WebMapServiceImageryProvider::new(
                ImageryLoader::new(loader.client.clone(), loader.host.clone()),
                wms_options(resource, &endpoint_options),
            )?),
            Some("WMTS") => Box::new(WebMapTileServiceImageryProvider::new(
                ImageryLoader::new(loader.client.clone(), loader.host.clone()),
                wmts_options(resource, &endpoint_options),
            )?),
            Some("SINGLE_TILE") => Box::new(
                SingleTileImageryProvider::from_url(
                    ImageryLoader::new(loader.client.clone(), loader.host.clone()),
                    resource,
                    None,
                    None,
                    None,
                )
                .await?,
            ),
            Some("MAPBOX") => {
                let mut mapbox = MapboxImageryProviderOptions::new(
                    endpoint_options["mapId"].as_str().unwrap_or(""),
                    endpoint_options["accessToken"].as_str().unwrap_or(""),
                );
                mapbox.url = Some(resource.clone());
                Box::new(MapboxImageryProvider::new(
                    ImageryLoader::new(loader.client.clone(), loader.host.clone()),
                    mapbox,
                )?)
            }
            Some("GOOGLE_2D_MAPS") => {
                let session = endpoint_options["session"].as_str().unwrap_or("");
                let key = endpoint_options["key"].as_str().unwrap_or("");
                let mut google = Google2DImageryProviderOptions::new(
                    key,
                    session,
                    endpoint_options["tileWidth"].as_f64().unwrap_or(256.0),
                    endpoint_options["tileHeight"].as_f64().unwrap_or(256.0),
                );
                google.url = Some(resource.clone());
                google.minimum_level = endpoint_options["minimumLevel"].as_u64().map(|v| v as u32);
                google.maximum_level = endpoint_options["maximumLevel"].as_u64().map(|v| v as u32);
                google.credit = endpoint_options["credit"].as_str().map(|v| Credit::new(v, true));
                Box::new(Google2DImageryProvider::new(
                    ImageryLoader::new(loader.client.clone(), loader.host.clone()),
                    google,
                )?)
            }
            Some("AZURE_MAPS") => {
                let key = endpoint_options["subscription-key"]
                    .as_str()
                    .or_else(|| endpoint_options["subscriptionKey"].as_str())
                    .unwrap_or("");
                let mut azure = Azure2DImageryProviderOptions::new(key);
                azure.url = Some(resource.clone());
                azure.tileset_id = endpoint_options["tilesetId"].as_str().map(str::to_owned);
                azure.minimum_level = endpoint_options["minimumLevel"].as_u64().map(|v| v as u32);
                azure.maximum_level = endpoint_options["maximumLevel"].as_u64().map(|v| v as u32);
                Box::new(Azure2DImageryProvider::new(
                    ImageryLoader::new(loader.client.clone(), loader.host.clone()),
                    azure,
                )?)
            }
            Some("GOOGLE_EARTH") => {
                let channel = endpoint_options["channel"].as_u64().unwrap_or(0) as u32;
                Box::new(
                    GoogleEarthEnterpriseMapsProvider::from_url(
                        ImageryLoader::new(loader.client.clone(), loader.host.clone()),
                        resource,
                        channel,
                        GoogleEarthEnterpriseMapsProviderOptions {
                            maximum_level: endpoint_options["maximumLevel"].as_u64().map(|v| v as u32),
                            ..Default::default()
                        },
                    )
                    .await?,
                )
            }
            Some("URL_TEMPLATE") => {
                let mut url_options = UrlTemplateImageryProviderOptions::new(resource);
                url_options.tile_width = options.tile_width;
                url_options.tile_height = options.tile_height;
                url_options.minimum_level = options.minimum_level;
                url_options.maximum_level = options.maximum_level;
                url_options.rectangle = options.rectangle;
                url_options.credit = options.credit.clone();
                Box::new(UrlTemplateImageryProvider::new(
                    ImageryLoader::new(loader.client.clone(), loader.host.clone()),
                    url_options,
                )?)
            }
            Some("TMS") | None => {
                let mut tms = TileMapServiceImageryProviderOptions {
                    minimum_level: options.minimum_level,
                    maximum_level: options.maximum_level,
                    rectangle: options.rectangle,
                    credit: options.credit.clone(),
                    ..Default::default()
                };
                if let Some(width) = options.tile_width {
                    tms.tile_width = Some(width);
                }
                if let Some(height) = options.tile_height {
                    tms.tile_height = Some(height);
                }
                Box::new(
                    TileMapServiceImageryProvider::from_url(
                        ImageryLoader::new(loader.client.clone(), loader.host.clone()),
                        resource,
                        tms,
                    )
                    .await?,
                )
            }
            Some(other) => {
                return Err(RuntimeError::new(format!("Unrecognized Cesium ion imagery type: {other}")));
            }
        };
        let error_event = Rc::new(RefCell::new(Default::default()));
        Ok(Self { inner, tile_credits, error_event })
    }

    pub fn inner(&self) -> &dyn ImageryProvider {
        self.inner.as_ref()
    }
}

fn ion_resource(url: &str, endpoint_resource: &Resource) -> Resource {
    let resource = Resource::new(url).unwrap_or_else(|| endpoint_resource.clone());
    let query = endpoint_resource.options().query_parameters.clone();
    if !query.is_empty() {
        resource.set_query_parameters(&query, false);
    }
    resource
}

fn arcgis_options(options: &Value) -> ArcGisMapServerImageryProviderOptions {
    ArcGisMapServerImageryProviderOptions {
        token: options["token"].as_str().map(str::to_owned),
        use_pre_cached_tiles_if_available: options["usePreCachedTilesIfAvailable"].as_bool().unwrap_or(true),
        layers: options["layers"].as_str().map(str::to_owned),
        enable_pick_features: options["enablePickFeatures"].as_bool(),
        rectangle: None,
        tiling_scheme: None,
        ellipsoid: None,
        credit: options["credit"].as_str().map(|v| Credit::new(v, true)),
        tile_width: options["tileWidth"].as_f64(),
        tile_height: options["tileHeight"].as_f64(),
        maximum_level: options["maximumLevel"].as_u64().map(|v| v as u32),
        tile_discard_policy: None,
    }
}

fn wms_options(url: Resource, options: &Value) -> WebMapServiceImageryProviderOptions {
    let mut result = WebMapServiceImageryProviderOptions::new(url, options["layers"].as_str().unwrap_or(""));
    result.minimum_level = options["minimumLevel"].as_u64().map(|v| v as u32);
    result.maximum_level = options["maximumLevel"].as_u64().map(|v| v as u32);
    result.tile_width = options["tileWidth"].as_f64();
    result.tile_height = options["tileHeight"].as_f64();
    result.credit = options["credit"].as_str().map(|v| Credit::new(v, true));
    result.enable_pick_features = options["enablePickFeatures"].as_bool();
    result
}

fn wmts_options(url: Resource, options: &Value) -> WebMapTileServiceImageryProviderOptions {
    let mut result = WebMapTileServiceImageryProviderOptions::new(
        url,
        options["layer"].as_str().unwrap_or(""),
        options["style"].as_str().unwrap_or(""),
        options["tileMatrixSetID"].as_str().unwrap_or(""),
    );
    result.format = options["format"].as_str().map(str::to_owned);
    result.tile_width = options["tileWidth"].as_f64();
    result.tile_height = options["tileHeight"].as_f64();
    result.minimum_level = options["minimumLevel"].as_u64().map(|v| v as u32);
    result.maximum_level = options["maximumLevel"].as_u64().map(|v| v as u32);
    result.credit = options["credit"].as_str().map(|v| Credit::new(v, true));
    result
}

impl ImageryProvider for IonImageryProvider {
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
    fn get_tile_credits(&self, x: i32, y: i32, level: u32) -> Option<Vec<Credit>> {
        let inner = self.inner.get_tile_credits(x, y, level);
        if let Some(inner) = inner {
            return Some(self.tile_credits.iter().chain(inner.iter()).cloned().collect());
        }
        Some(self.tile_credits.clone())
    }
    fn request_image(&self, x: i32, y: i32, level: u32) -> Option<ImageryFuture> {
        self.inner.request_image(x, y, level)
    }
    fn pick_features(&self, x: i32, y: i32, level: u32, longitude: f64, latitude: f64) -> Option<ImageryFeatureFuture> {
        self.inner.pick_features(x, y, level, longitude, latitude)
    }
}

/// Builds a world-imagery provider for an [`crate::scene::IonWorldImageryStyle`].
pub async fn create_world_imagery_async<H: ImageHost + 'static>(
    loader: ImageryLoader<H>,
    style: crate::scene::IonWorldImageryStyle,
    options: IonImageryProviderOptions,
) -> Result<IonImageryProvider, RuntimeError> {
    IonImageryProvider::from_asset_id(loader, style as u64, options).await
}
