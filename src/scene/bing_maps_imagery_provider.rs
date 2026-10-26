//! CesiumJS `BingMapsImageryProvider`: Bing Maps REST imagery metadata,
//! quadkey tile URLs, coverage-area credits and empty-tile handling.

use super::{
    image_error, report_image_error, BingMapsStyle, ImageryErrorEvent, ImageryFuture, ImageryImage, ImageryLoader,
    ImageryProvider, TileDiscardPolicy,
};
use crate::core::{
    Credit, Ellipsoid, ImageHost, Rectangle, Resource, RuntimeError, TilingScheme, WebMercatorTilingScheme,
};
use serde_json::Value;
use std::{cell::RefCell, rc::Rc, sync::Arc};

pub const DEFAULT_BING_URL: &str = "https://dev.virtualearth.net";
/// `BingMapsImageryProvider.logoUrl` resolves this module-relative asset.
pub const DEFAULT_LOGO_URL: &str = "Assets/Images/bing_maps_credit.png";

#[derive(Clone, Debug, PartialEq)]
pub struct BingCoverageArea {
    pub bbox: Rectangle,
    pub zoom_min: u32,
    pub zoom_max: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct BingAttribution {
    pub attribution: String,
    pub coverage_areas: Vec<BingCoverageArea>,
    pub credit: Credit,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct BingMetadata {
    pub tile_width: f64,
    pub tile_height: f64,
    pub maximum_level: u32,
    pub image_url_subdomains: Vec<String>,
    pub image_url_template: String,
    pub attribution_list: Vec<BingAttribution>,
}

#[derive(Clone)]
pub struct BingMapsImageryProviderOptions {
    pub key: String,
    pub tile_protocol: Option<String>,
    pub map_style: BingMapsStyle,
    pub map_layer: Option<String>,
    pub culture: Option<String>,
    pub ellipsoid: Option<Ellipsoid>,
    pub tile_discard_policy: Option<Rc<dyn TileDiscardPolicy<ImageryImage>>>,
}

impl BingMapsImageryProviderOptions {
    pub fn new(key: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            tile_protocol: None,
            map_style: BingMapsStyle::Aerial,
            map_layer: None,
            culture: None,
            ellipsoid: None,
            tile_discard_policy: None,
        }
    }
}

pub struct BingMapsImageryProvider<H: ImageHost> {
    resource: Resource,
    loader: ImageryLoader<H>,
    key: String,
    map_style: BingMapsStyle,
    map_layer: Option<String>,
    culture: String,
    tiling_scheme: Arc<dyn TilingScheme>,
    tile_width: f64,
    tile_height: f64,
    maximum_level: Option<u32>,
    image_url_template: String,
    image_url_subdomains: Vec<String>,
    attribution_list: Vec<BingAttribution>,
    tile_discard_policy: Option<Rc<dyn TileDiscardPolicy<ImageryImage>>>,
    credit: Credit,
    error_event: ImageryErrorEvent,
}

impl<H: ImageHost + 'static> BingMapsImageryProvider<H> {
    /// Cesium's `fromUrl`. The metadata response must describe exactly one
    /// resource; any other shape raises the upstream `RuntimeError`.
    pub async fn from_url(
        loader: ImageryLoader<H>,
        url: Resource,
        mut options: BingMapsImageryProviderOptions,
    ) -> Result<Self, RuntimeError> {
        let tile_protocol = match options.tile_protocol.take() {
            Some(mut protocol) => {
                if protocol.ends_with(':') {
                    protocol.pop();
                }
                protocol
            }
            None => "https".into(),
        };
        let resource = url;
        resource.append_forward_slash();
        let mut query = vec![
            ("incl".to_owned(), Some(crate::core::query::QueryValue::Single("ImageryProviders".into()))),
            ("key".to_owned(), Some(crate::core::query::QueryValue::Single(options.key.clone()))),
            ("uriScheme".to_owned(), Some(crate::core::query::QueryValue::Single(tile_protocol))),
        ];
        if let Some(layer) = &options.map_layer {
            query.push(("mapLayer".into(), Some(crate::core::query::QueryValue::Single(layer.clone()))));
        }
        if let Some(culture) = &options.culture {
            query.push(("culture".into(), Some(crate::core::query::QueryValue::Single(culture.clone()))));
        }
        let metadata_resource = resource
            .get_derived_resource(crate::core::resource::DerivedResourceOptions {
                url: Some(format!("REST/v1/Imagery/Metadata/{}", options.map_style.as_str())),
                query_parameters: query,
                ..Default::default()
            })
            .ok_or_else(|| RuntimeError::new("Invalid Bing metadata resource."))?;

        let fetch = loader
            .client
            .fetch_json(&metadata_resource)
            .ok_or_else(|| RuntimeError::new("Bing metadata request was throttled."))?;
        let json = fetch.await.map_err(|e| {
            RuntimeError::new(format!("An error occurred while accessing {}: {e}", metadata_resource.url()))
        })?;
        let metadata = metadata_from_json(&json).map_err(|e| {
            RuntimeError::new(format!("An error occurred while accessing {}: {}", metadata_resource.url(), e.message))
        })?;

        // Cesium acquires the shared empty image at module load. Here the host
        // decodes it once per provider so the default discard policy can
        // compare tile images by identity.
        let empty_image = match loader
            .load_image(Resource::new(super::discard_empty_tile_image_policy::EMPTY_IMAGE_URI).unwrap(), false)
        {
            Some(fetch) => fetch.await.ok(),
            None => None,
        };
        let default_policy = empty_image
            .map(|image| Rc::new(BingEmptyImagePolicy { empty: image }) as Rc<dyn TileDiscardPolicy<ImageryImage>>);
        let policy = options.tile_discard_policy.take().or(default_policy);
        let ellipsoid = options.ellipsoid.unwrap_or(Ellipsoid::WGS84);
        Ok(Self {
            resource,
            loader,
            key: options.key,
            map_style: options.map_style,
            map_layer: options.map_layer,
            culture: options.culture.unwrap_or_default(),
            tiling_scheme: Arc::new(WebMercatorTilingScheme::new(ellipsoid, 2, 2, None)),
            tile_width: metadata.tile_width,
            tile_height: metadata.tile_height,
            maximum_level: Some(metadata.maximum_level),
            image_url_template: metadata.image_url_template,
            image_url_subdomains: metadata.image_url_subdomains,
            attribution_list: metadata.attribution_list,
            tile_discard_policy: policy,
            credit: bing_credit(),
            error_event: Rc::new(RefCell::new(Default::default())),
        })
    }

    /// Builds a provider from metadata already in hand. Useful for ion
    /// endpoints and deterministic replay.
    pub fn from_metadata(
        loader: ImageryLoader<H>,
        resource: Resource,
        options: BingMapsImageryProviderOptions,
        metadata: BingMetadata,
        empty_image: Option<ImageryImage>,
    ) -> Self {
        let policy = options.tile_discard_policy.clone().or_else(|| {
            empty_image
                .map(|image| Rc::new(BingEmptyImagePolicy { empty: image }) as Rc<dyn TileDiscardPolicy<ImageryImage>>)
        });
        resource.append_forward_slash();
        Self {
            resource,
            loader,
            key: options.key,
            map_style: options.map_style,
            map_layer: options.map_layer,
            culture: options.culture.unwrap_or_default(),
            tiling_scheme: Arc::new(WebMercatorTilingScheme::new(
                options.ellipsoid.unwrap_or(Ellipsoid::WGS84),
                2,
                2,
                None,
            )),
            tile_width: metadata.tile_width,
            tile_height: metadata.tile_height,
            maximum_level: Some(metadata.maximum_level),
            image_url_template: metadata.image_url_template,
            image_url_subdomains: metadata.image_url_subdomains,
            attribution_list: metadata.attribution_list,
            tile_discard_policy: policy,
            credit: bing_credit(),
            error_event: Rc::new(RefCell::new(Default::default())),
        }
    }

    pub fn url(&self) -> String {
        self.resource.url()
    }
    pub fn key(&self) -> &str {
        &self.key
    }
    pub fn map_style(&self) -> BingMapsStyle {
        self.map_style
    }
    pub fn map_layer(&self) -> Option<&str> {
        self.map_layer.as_deref()
    }
    pub fn culture(&self) -> &str {
        &self.culture
    }
    pub fn metadata(&self) -> BingMetadata {
        BingMetadata {
            tile_width: self.tile_width,
            tile_height: self.tile_height,
            maximum_level: self.maximum_level.unwrap_or(0),
            image_url_subdomains: self.image_url_subdomains.clone(),
            image_url_template: self.image_url_template.clone(),
            attribution_list: self.attribution_list.clone(),
        }
    }

    /// `BingMapsImageryProvider.tileXYToQuadKey`.
    pub fn tile_xy_to_quad_key(x: i32, y: i32, level: u32) -> String {
        let mut key = String::new();
        for i in (0..=level).rev() {
            let mask = 1i32.wrapping_shl(i);
            let mut digit = 0;
            if x & mask != 0 {
                digit |= 1;
            }
            if y & mask != 0 {
                digit |= 2;
            }
            key.push(char::from(b'0' + digit as u8));
        }
        key
    }

    /// `BingMapsImageryProvider.quadKeyToTileXY`.
    pub fn quad_key_to_tile_xy(quadkey: &str) -> (i32, i32, i32) {
        let level = quadkey.len() as i32 - 1;
        let (mut x, mut y) = (0, 0);
        for i in (0..=level).rev() {
            let mask = 1i32.wrapping_shl(i as u32);
            let digit = quadkey.as_bytes()[level as usize - i as usize].wrapping_sub(b'0');
            if digit & 1 != 0 {
                x |= mask;
            }
            if digit & 2 != 0 {
                y |= mask;
            }
        }
        (x, y, level)
    }

    pub fn image_resource(&self, x: i32, y: i32, level: u32) -> Resource {
        let subdomain_index = (x + y + level as i32) % self.image_url_subdomains.len().max(1) as i32;
        let subdomain = usize::try_from(subdomain_index)
            .ok()
            .and_then(|i| self.image_url_subdomains.get(i))
            .cloned()
            .unwrap_or_else(|| "undefined".into());
        self.resource
            .get_derived_resource(crate::core::resource::DerivedResourceOptions {
                url: Some(self.image_url_template.clone()),
                template_values: vec![
                    ("quadkey".into(), Self::tile_xy_to_quad_key(x, y, level)),
                    ("subdomain".into(), subdomain),
                    ("culture".into(), self.culture.clone()),
                ],
                query_parameters: vec![("n".into(), Some(crate::core::query::QueryValue::Single("z".into())))],
                ..Default::default()
            })
            .unwrap_or_else(|| self.resource.clone())
    }

    pub fn tile_url(&self, x: i32, y: i32, level: u32) -> String {
        self.image_resource(x, y, level).get_url_component(true, false)
    }

    /// Credits for a tile: attributions whose coverage includes the tile
    /// rectangle at Bing's one-based zoom.
    pub fn credits_for_tile(&self, x: i32, y: i32, level: u32) -> Vec<Credit> {
        let rectangle = self.tiling_scheme.tile_xy_to_rectangle(f64::from(x), f64::from(y), level);
        self.attribution_list
            .iter()
            .filter(|attribution| {
                attribution.coverage_areas.iter().any(|area| {
                    let zoom = level + 1;
                    zoom >= area.zoom_min && zoom <= area.zoom_max && rectangle.intersection(&area.bbox).is_some()
                })
            })
            .map(|attribution| attribution.credit.clone())
            .collect()
    }
}

fn bing_credit() -> Credit {
    Credit::new(
        format!(
            "<a href=\"https://www.microsoft.com/en-us/maps/bing-maps/product\"><img src=\"{DEFAULT_LOGO_URL}\" title=\"Bing Imagery\"/></a>"
        ),
        false,
    )
}

/// Compares the `ImageryImage` returned for an empty response with the shared
/// empty frame decoded by the host.
struct BingEmptyImagePolicy {
    empty: ImageryImage,
}

impl TileDiscardPolicy<ImageryImage> for BingEmptyImagePolicy {
    fn is_ready(&self) -> bool {
        true
    }
    fn should_discard_image(&self, image: &ImageryImage) -> bool {
        match (image, &self.empty) {
            (ImageryImage::Image(a), ImageryImage::Image(b)) => a.ptr_eq(b),
            _ => false,
        }
    }
}

fn metadata_from_json(data: &Value) -> Result<BingMetadata, RuntimeError> {
    let sets = data["resourceSets"].as_array().ok_or_else(|| RuntimeError::new("metadata has no resourceSets"))?;
    if sets.len() != 1 {
        return Err(RuntimeError::new("metadata does not specify one resource in resourceSets"));
    }
    let resources = sets[0]["resources"].as_array().ok_or_else(|| RuntimeError::new("metadata has no resources"))?;
    let resource = resources.first().ok_or_else(|| RuntimeError::new("metadata has no resources"))?;
    let tile_width = resource["imageWidth"].as_f64().ok_or_else(|| RuntimeError::new("metadata has no imageWidth"))?;
    let tile_height =
        resource["imageHeight"].as_f64().ok_or_else(|| RuntimeError::new("metadata has no imageHeight"))?;
    let zoom_max = resource["zoomMax"].as_f64().ok_or_else(|| RuntimeError::new("metadata has no zoomMax"))?;
    let image_url_template =
        resource["imageUrl"].as_str().ok_or_else(|| RuntimeError::new("metadata has no imageUrl"))?.to_owned();
    let image_url_subdomains = resource["imageUrlSubdomains"]
        .as_array()
        .map(|values| values.iter().filter_map(Value::as_str).map(str::to_owned).collect())
        .unwrap_or_default();

    let attribution_list = resource["imageryProviders"]
        .as_array()
        .map(|providers| {
            providers
                .iter()
                .filter(|provider| {
                    provider["coverageAreas"]
                        .as_array()
                        .is_some_and(|areas| areas.iter().any(|area| !area["bbox"].is_null()))
                })
                .map(|provider| {
                    let attribution = provider["attribution"].as_str().unwrap_or("").to_owned();
                    let coverage_areas = provider["coverageAreas"]
                        .as_array()
                        .map(|areas| {
                            areas
                                .iter()
                                .filter_map(|area| {
                                    let bbox = area["bbox"].as_array()?;
                                    if bbox.len() < 4 {
                                        return None;
                                    }
                                    let value = |i: usize| bbox[i].as_f64();
                                    Some(BingCoverageArea {
                                        bbox: Rectangle::from_degrees(value(1)?, value(0)?, value(3)?, value(2)?),
                                        zoom_min: area["zoomMin"].as_f64().unwrap_or(0.0) as u32,
                                        zoom_max: area["zoomMax"].as_f64().unwrap_or(0.0) as u32,
                                    })
                                })
                                .collect()
                        })
                        .unwrap_or_default();
                    BingAttribution { credit: Credit::new(attribution.clone(), false), attribution, coverage_areas }
                })
                .collect()
        })
        .unwrap_or_default();

    Ok(BingMetadata {
        tile_width,
        tile_height,
        maximum_level: (zoom_max as i64 - 1).max(0) as u32,
        image_url_subdomains,
        image_url_template,
        attribution_list,
    })
}

impl<H: ImageHost + 'static> ImageryProvider for BingMapsImageryProvider<H> {
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
        self.maximum_level
    }
    fn minimum_level(&self) -> u32 {
        0
    }
    fn tiling_scheme(&self) -> &dyn TilingScheme {
        self.tiling_scheme.as_ref()
    }
    fn tile_discard_policy(&self) -> Option<Rc<dyn TileDiscardPolicy<ImageryImage>>> {
        self.tile_discard_policy.clone()
    }
    fn error_event(&self) -> &ImageryErrorEvent {
        &self.error_event
    }
    fn credit(&self) -> Option<&Credit> {
        Some(&self.credit)
    }
    fn proxy(&self) -> Option<Arc<dyn crate::core::Proxy>> {
        self.resource.options().proxy.clone()
    }
    fn has_alpha_channel(&self) -> bool {
        self.map_layer.is_some()
    }
    fn get_tile_credits(&self, x: i32, y: i32, level: u32) -> Option<Vec<Credit>> {
        Some(self.credits_for_tile(x, y, level))
    }
    fn request_image(&self, x: i32, y: i32, level: u32) -> Option<ImageryFuture> {
        let has_policy = self.tile_discard_policy.is_some();
        let fetch = self.loader.load_image(self.image_resource(x, y, level), has_policy)?;
        let error_event = self.error_event.clone();
        let url = self.image_resource(x, y, level).url();
        Some(Box::pin(async move {
            match fetch.await {
                Ok(image) => Ok(image),
                Err(error) => {
                    let mut provider_error = image_error(
                        None,
                        format!("Failed to load image {url}: {error}"),
                        Some(x),
                        Some(y),
                        Some(level),
                    );
                    report_image_error(&error_event, &mut provider_error);
                    Err(RuntimeError::new(format!("Failed to load image {url}: {error}")))
                }
            }
        }))
    }
}

// Alias imported for documentation parity; the actual URI lives with the
// discard policy it belongs to.
