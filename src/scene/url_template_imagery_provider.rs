//! CesiumJS `UrlTemplateImageryProvider`: imagery from a URL template.
//!
//! Template expansion follows `Resource.getDerivedResource`, so values are
//! percent-encoded exactly as Cesium encodes them. The built-in tags are
//! implemented here; custom tags are closures supplied by the caller.

use super::get_feature_info_format::{FeatureInfoResponse, GetFeatureInfoFormat};
use super::{ImageryErrorEvent, ImageryImage, ImageryLoader, ImageryProvider};
use crate::core::resource::DerivedResourceOptions;
use crate::core::{
    math::{js_number_to_string, to_degrees, to_int32},
    Credit, ImageHost, MapProjection, Rectangle, Resource, ResourceClient, RuntimeError, TilingScheme,
    WebMercatorTilingScheme,
};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;

/// Arguments a custom tag receives. `longitude`, `latitude` and `format` are
/// present only while building a feature-pick URL.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct CustomTagArgs {
    pub x: i32,
    pub y: i32,
    pub level: u32,
    pub longitude: Option<f64>,
    pub latitude: Option<f64>,
    pub format: Option<String>,
}

/// `customTags` values. Cesium calls them with the provider as well; the
/// provider is available through the closure's own captures.
pub type CustomTag = Rc<dyn Fn(&CustomTagArgs) -> String>;

#[derive(Clone)]
pub struct UrlTemplateImageryProviderOptions {
    pub url: Resource,
    pub pick_features_url: Option<Resource>,
    pub url_scheme_zero_padding: HashMap<String, String>,
    /// `None` is Cesium's default `["a", "b", "c"]`.
    pub subdomains: Option<Vec<String>>,
    pub credit: Option<Credit>,
    pub minimum_level: Option<u32>,
    pub maximum_level: Option<u32>,
    pub rectangle: Option<Rectangle>,
    pub tiling_scheme: Option<Arc<dyn TilingScheme>>,
    pub tile_width: Option<f64>,
    pub tile_height: Option<f64>,
    pub has_alpha_channel: Option<bool>,
    pub get_feature_info_formats: Vec<GetFeatureInfoFormat>,
    pub enable_pick_features: Option<bool>,
    pub tile_discard_policy: Option<Rc<dyn super::TileDiscardPolicy<ImageryImage>>>,
    pub custom_tags: HashMap<String, CustomTag>,
}

impl UrlTemplateImageryProviderOptions {
    pub fn new(url: Resource) -> Self {
        Self {
            url,
            pick_features_url: None,
            url_scheme_zero_padding: HashMap::new(),
            subdomains: None,
            credit: None,
            minimum_level: None,
            maximum_level: None,
            rectangle: None,
            tiling_scheme: None,
            tile_width: None,
            tile_height: None,
            has_alpha_channel: None,
            get_feature_info_formats: Vec::new(),
            enable_pick_features: None,
            tile_discard_policy: None,
            custom_tags: HashMap::new(),
        }
    }
}

pub struct UrlTemplateImageryProvider<H: ImageHost> {
    resource: Resource,
    pick_features_resource: Option<Resource>,
    loader: ImageryLoader<H>,
    url_scheme_zero_padding: HashMap<String, String>,
    subdomains: Vec<String>,
    tile_width: f64,
    tile_height: f64,
    minimum_level: u32,
    maximum_level: Option<u32>,
    tiling_scheme: Arc<dyn TilingScheme>,
    rectangle: Rectangle,
    tile_discard_policy: Option<Rc<dyn super::TileDiscardPolicy<ImageryImage>>>,
    credit: Option<Credit>,
    has_alpha_channel: bool,
    get_feature_info_formats: Vec<GetFeatureInfoFormat>,
    enable_pick_features: Cell<bool>,
    custom_tags: HashMap<String, CustomTag>,
    error_event: ImageryErrorEvent,
}

impl<H: ImageHost + 'static> UrlTemplateImageryProvider<H> {
    pub fn new(loader: ImageryLoader<H>, options: UrlTemplateImageryProviderOptions) -> Result<Self, RuntimeError> {
        let tiling_scheme = options.tiling_scheme.unwrap_or_else(|| Arc::new(WebMercatorTilingScheme::default()));
        let rectangle = match options.rectangle {
            Some(rectangle) => rectangle
                .intersection(tiling_scheme.rectangle())
                .ok_or_else(|| RuntimeError::new("The rectangle does not intersect the tiling scheme."))?,
            None => *tiling_scheme.rectangle(),
        };
        let subdomains = match options.subdomains {
            Some(values) if !values.is_empty() => values,
            _ => vec!["a".to_owned(), "b".to_owned(), "c".to_owned()],
        };
        Ok(Self {
            resource: options.url,
            pick_features_resource: options.pick_features_url,
            loader,
            url_scheme_zero_padding: options.url_scheme_zero_padding,
            subdomains,
            tile_width: options.tile_width.unwrap_or(256.0),
            tile_height: options.tile_height.unwrap_or(256.0),
            minimum_level: options.minimum_level.unwrap_or(0),
            maximum_level: options.maximum_level,
            tiling_scheme,
            rectangle,
            tile_discard_policy: options.tile_discard_policy,
            credit: options.credit,
            has_alpha_channel: options.has_alpha_channel.unwrap_or(true),
            get_feature_info_formats: options.get_feature_info_formats,
            enable_pick_features: Cell::new(options.enable_pick_features.unwrap_or(true)),
            custom_tags: options.custom_tags,
            error_event: Rc::new(RefCell::new(Default::default())),
        })
    }

    pub fn url(&self) -> String {
        self.resource.url()
    }
    pub fn pick_features_url(&self) -> Option<String> {
        self.pick_features_resource.as_ref().map(Resource::url)
    }
    pub fn url_scheme_zero_padding(&self) -> &HashMap<String, String> {
        &self.url_scheme_zero_padding
    }
    pub fn enable_pick_features(&self) -> bool {
        self.enable_pick_features.get()
    }
    pub fn set_enable_pick_features(&self, enabled: bool) {
        self.enable_pick_features.set(enabled);
    }
    pub fn loader(&self) -> &ImageryLoader<H> {
        &self.loader
    }
    pub fn get_feature_info_formats(&self) -> &[GetFeatureInfoFormat] {
        &self.get_feature_info_formats
    }
    pub fn client(&self) -> Rc<ResourceClient> {
        self.loader.client.clone()
    }

    fn pad_with_zeros_if_necessary(&self, key: &str, value: String) -> String {
        if let Some(template) = self.url_scheme_zero_padding.get(key) {
            if template.len() > 1 && value.len() < template.len() {
                return format!("{}{value}", "0".repeat(template.len() - value.len()));
            }
        }
        value
    }

    fn number_tag(&self, key: &str, value: i64) -> String {
        self.pad_with_zeros_if_necessary(key, value.to_string())
    }

    fn image_tag_value(&self, key: &str, x: i32, y: i32, level: u32) -> Option<String> {
        if let Some(custom) = self.custom_tags.get(key) {
            return Some(custom(&CustomTagArgs { x, y, level, ..Default::default() }));
        }
        if key == "s" {
            // Cesium indexes the array with JavaScript's `%`, which keeps the
            // sign, so a negative index leaves the placeholder in place.
            let index = (x + y + level as i32) % self.subdomains.len() as i32;
            return usize::try_from(index).ok().map(|index| self.subdomains[index].clone());
        }
        Some(match key {
            "x" => self.number_tag("{x}", i64::from(x)),
            "y" => self.number_tag("{y}", i64::from(y)),
            "z" => self.number_tag("{z}", i64::from(level)),
            "reverseX" => self.number_tag(
                "{reverseX}",
                i64::from(self.tiling_scheme.get_number_of_x_tiles_at_level(level)) - i64::from(x) - 1,
            ),
            "reverseY" => self.number_tag(
                "{reverseY}",
                i64::from(self.tiling_scheme.get_number_of_y_tiles_at_level(level)) - i64::from(y) - 1,
            ),
            "reverseZ" => {
                let reverse = match self.maximum_level {
                    Some(maximum) if level < maximum => maximum - level - 1,
                    _ => level,
                };
                self.number_tag("{reverseZ}", i64::from(reverse))
            }
            "s" => return None,
            "westDegrees" => js_number_to_string(to_degrees(self.tile_rectangle(x, y, level).west)),
            "southDegrees" => js_number_to_string(to_degrees(self.tile_rectangle(x, y, level).south)),
            "eastDegrees" => js_number_to_string(to_degrees(self.tile_rectangle(x, y, level).east)),
            "northDegrees" => js_number_to_string(to_degrees(self.tile_rectangle(x, y, level).north)),
            "westProjected" => js_number_to_string(self.native_rectangle(x, y, level).west),
            "southProjected" => js_number_to_string(self.native_rectangle(x, y, level).south),
            "eastProjected" => js_number_to_string(self.native_rectangle(x, y, level).east),
            "northProjected" => js_number_to_string(self.native_rectangle(x, y, level).north),
            "width" => js_number_to_string(self.tile_width),
            "height" => js_number_to_string(self.tile_height),
            _ => return None,
        })
    }

    fn pick_tag_value(&self, key: &str, args: &CustomTagArgs) -> Option<String> {
        if let Some(custom) = self.custom_tags.get(key) {
            return Some(custom(args));
        }
        let longitude = args.longitude.unwrap_or(0.0);
        let latitude = args.latitude.unwrap_or(0.0);
        let projected = self.project(longitude, latitude);
        let native = self.native_rectangle(args.x, args.y, args.level);
        let i = to_int32(self.tile_width * (projected.x - native.west) / native.width());
        let j = to_int32(self.tile_height * (native.north - projected.y) / native.height());
        Some(match key {
            "i" => i.to_string(),
            "j" => j.to_string(),
            "reverseI" => js_number_to_string(self.tile_width - f64::from(i) - 1.0),
            "reverseJ" => js_number_to_string(self.tile_height - f64::from(j) - 1.0),
            "longitudeDegrees" => js_number_to_string(to_degrees(longitude)),
            "latitudeDegrees" => js_number_to_string(to_degrees(latitude)),
            "longitudeProjected" => js_number_to_string(projected.x),
            "latitudeProjected" => js_number_to_string(projected.y),
            "format" => args.format.clone().unwrap_or_default(),
            _ => return self.image_tag_value(key, args.x, args.y, args.level),
        })
    }

    fn tile_rectangle(&self, x: i32, y: i32, level: u32) -> Rectangle {
        self.tiling_scheme.tile_xy_to_rectangle(f64::from(x), f64::from(y), level)
    }
    fn native_rectangle(&self, x: i32, y: i32, level: u32) -> Rectangle {
        self.tiling_scheme.tile_xy_to_native_rectangle(f64::from(x), f64::from(y), level)
    }
    /// Degrees for a geographic projection, projected coordinates otherwise.
    fn project(&self, longitude: f64, latitude: f64) -> crate::core::Cartesian3 {
        let projection: &dyn MapProjection = self.tiling_scheme.projection();
        if projection.is_geographic() {
            crate::core::Cartesian3::new(to_degrees(longitude), to_degrees(latitude), 0.0)
        } else {
            projection.project(&crate::core::Cartographic::new(longitude, latitude, 0.0))
        }
    }

    /// A tag's value, as `Resource` template expansion receives it.
    pub fn image_tag(&self, key: &str, x: i32, y: i32, level: u32) -> Option<String> {
        self.image_tag_value(key, x, y, level)
    }

    /// The tile resource Cesium's `buildImageResource` creates.
    pub fn tile_resource(&self, x: i32, y: i32, level: u32) -> Resource {
        self.derive(&self.resource, x, y, level, None)
    }

    /// The URL Cesium requests for a tile.
    pub fn tile_url(&self, x: i32, y: i32, level: u32) -> String {
        self.tile_resource(x, y, level).get_url_component(true, false)
    }

    /// The pick-features URL Cesium requests for a position.
    pub fn pick_features_tile_url(
        &self,
        x: i32,
        y: i32,
        level: u32,
        longitude: f64,
        latitude: f64,
        format: &str,
    ) -> Option<String> {
        let resource = self.derive_pick(x, y, level, longitude, latitude, format)?;
        Some(resource.get_url_component(true, false))
    }

    fn derive(&self, base: &Resource, x: i32, y: i32, level: u32, _request: Option<()>) -> Resource {
        let url = base.get_url_component(true, false);
        let values = template_values(&url)
            .into_iter()
            .filter_map(|key| self.image_tag_value(&key, x, y, level).map(|value| (key, value)))
            .collect();
        base.get_derived_resource(DerivedResourceOptions { template_values: values, ..Default::default() })
            .unwrap_or_else(|| base.clone())
    }

    fn derive_pick(&self, x: i32, y: i32, level: u32, longitude: f64, latitude: f64, format: &str) -> Option<Resource> {
        let base = self.pick_features_resource.as_ref()?;
        let url = base.get_url_component(true, false);
        let args = CustomTagArgs {
            x,
            y,
            level,
            longitude: Some(longitude),
            latitude: Some(latitude),
            format: Some(format.to_owned()),
        };
        let values = template_values(&url)
            .into_iter()
            .filter_map(|key| self.pick_tag_value(&key, &args).map(|value| (key, value)))
            .collect();
        base.get_derived_resource(DerivedResourceOptions { template_values: values, ..Default::default() })
    }
}

impl<H: ImageHost + 'static> ImageryProvider for UrlTemplateImageryProvider<H> {
    fn rectangle(&self) -> &Rectangle {
        &self.rectangle
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
        self.minimum_level
    }
    fn tiling_scheme(&self) -> &dyn TilingScheme {
        self.tiling_scheme.as_ref()
    }
    fn tile_discard_policy(&self) -> Option<Rc<dyn super::TileDiscardPolicy<ImageryImage>>> {
        self.tile_discard_policy.clone()
    }
    fn error_event(&self) -> &ImageryErrorEvent {
        &self.error_event
    }
    fn credit(&self) -> Option<&Credit> {
        self.credit.as_ref()
    }
    fn proxy(&self) -> Option<Arc<dyn crate::core::Proxy>> {
        self.resource.options().proxy.clone()
    }
    fn has_alpha_channel(&self) -> bool {
        self.has_alpha_channel
    }

    fn request_image(&self, x: i32, y: i32, level: u32) -> Option<super::ImageryFuture> {
        let resource = self.tile_resource(x, y, level);
        let has_discard_policy = self.tile_discard_policy.is_some();
        self.loader.load_image(resource, has_discard_policy)
    }

    fn pick_features(
        &self,
        x: i32,
        y: i32,
        level: u32,
        longitude: f64,
        latitude: f64,
    ) -> Option<super::ImageryFeatureFuture> {
        if !self.enable_pick_features.get()
            || self.pick_features_resource.is_none()
            || self.get_feature_info_formats.is_empty()
        {
            return None;
        }
        let formats = self.get_feature_info_formats.clone();
        let requests: Vec<(Resource, GetFeatureInfoFormat, String)> = formats
            .into_iter()
            .filter_map(|format| {
                self.derive_pick(x, y, level, longitude, latitude, &format.format)
                    .map(|resource| (resource, format.clone(), format.format.clone()))
            })
            .collect();
        let client = self.loader.client.clone();
        Some(Box::pin(async move {
            for (resource, format, _) in requests {
                let response = fetch_feature_response(&client, &resource, &format.response_type).await;
                let Ok(response) = response else {
                    continue;
                };
                match format.callback(response) {
                    Ok(features) => return Ok(features),
                    // Cesium's `.then(callback).catch(doRequest)` also retries
                    // when the parser rejects the response.
                    Err(_) => continue,
                }
            }
            Ok(Some(Vec::new()))
        }))
    }
}

async fn fetch_feature_response(
    client: &Rc<ResourceClient>,
    resource: &Resource,
    response_type: &super::get_feature_info_format::FeatureInfoType,
) -> Result<FeatureInfoResponse, RuntimeError> {
    use super::get_feature_info_format::FeatureInfoType;
    match response_type {
        FeatureInfoType::Json => {
            let fetch = client.fetch_json(resource).ok_or_else(|| RuntimeError::new("throttled"))?;
            Ok(FeatureInfoResponse::Json(fetch.await.map_err(|e| RuntimeError::new(e.to_string()))?))
        }
        FeatureInfoType::Xml => {
            let fetch = client.fetch_xml(resource).ok_or_else(|| RuntimeError::new("throttled"))?;
            Ok(FeatureInfoResponse::Xml(fetch.await.map_err(|e| RuntimeError::new(e.to_string()))?))
        }
        FeatureInfoType::Html | FeatureInfoType::Text => {
            let fetch = client.fetch_text(resource).ok_or_else(|| RuntimeError::new("throttled"))?;
            Ok(FeatureInfoResponse::Text(fetch.await.map_err(|e| RuntimeError::new(e.to_string()))?))
        }
        FeatureInfoType::Other(_) => {
            let fetch = client.fetch_array_buffer(resource).ok_or_else(|| RuntimeError::new("throttled"))?;
            Ok(FeatureInfoResponse::Bytes(fetch.await.map_err(|e| RuntimeError::new(e.to_string()))?))
        }
    }
}

/// `{[^}]+}` placeholders in document order, without braces.
fn template_values(url: &str) -> Vec<String> {
    let mut values = Vec::new();
    let bytes = url.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if bytes[index] == b'{' {
            if let Some(end) = url[index + 1..].find('}') {
                values.push(url[index + 1..index + 1 + end].to_owned());
                index += end + 2;
                continue;
            }
        }
        index += 1;
    }
    values
}
