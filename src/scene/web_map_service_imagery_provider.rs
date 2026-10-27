//! CesiumJS `WebMapServiceImageryProvider`: tiled WMS GetMap imagery with
//! GetFeatureInfo picking and time-dynamic dimensions.
//!
//! URL building is delegated to [`UrlTemplateImageryProvider`], as upstream
//! does. A time-dynamic request clones the configured resource with the
//! interval's parameters so the shared template provider never leaks one
//! interval's dimensions into another.

use super::time_dynamic_imagery::{TimeDynamicImagery, TimeDynamicImageryOptions};
use super::{
    GetFeatureInfoFormat, ImageryErrorEvent, ImageryFeatureFuture, ImageryFuture, ImageryImage, ImageryLoader,
    ImageryProvider, TileDiscardPolicy, UrlTemplateImageryProvider, UrlTemplateImageryProviderOptions,
};
use crate::core::query::QueryValue;
use crate::core::{
    Clock, Credit, Ellipsoid, GeographicTilingScheme, ImageHost, Rectangle, Resource, RuntimeError, TilingScheme,
    TimeIntervalCollection,
};
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    sync::Arc,
};

/// EPSG codes whose axis order is north/east despite being outside 4000–4999.
pub const INCLUDES_REVERSE_AXIS: [u32; 5] = [3034, 3035, 3042, 3043, 3044];
/// EPSG codes in 4000–4999 that use the usual east/north order.
pub const EXCLUDES_REVERSE_AXIS: [u32; 2] = [4471, 4559];

pub const DEFAULT_PARAMETERS: [(&str, &str); 5] =
    [("service", "WMS"), ("version", "1.1.1"), ("request", "GetMap"), ("styles", ""), ("format", "image/jpeg")];

pub type DynamicData = Vec<(String, String)>;
type TimeDynamicHandle = Rc<RefCell<TimeDynamicImagery<DynamicData>>>;

pub const GET_FEATURE_INFO_DEFAULT_PARAMETERS: [(&str, &str); 3] =
    [("service", "WMS"), ("version", "1.1.1"), ("request", "GetFeatureInfo")];

pub fn default_get_feature_info_formats() -> Vec<GetFeatureInfoFormat> {
    vec![GetFeatureInfoFormat::json(), GetFeatureInfoFormat::xml(), GetFeatureInfoFormat::text()]
}

#[derive()]
pub struct WebMapServiceImageryProviderOptions {
    pub url: Resource,
    pub layers: String,
    pub get_feature_info_url: Option<Resource>,
    /// Case-insensitive keys; stored lowercased like upstream.
    pub parameters: Vec<(String, String)>,
    pub get_feature_info_parameters: Vec<(String, String)>,
    pub enable_pick_features: Option<bool>,
    pub get_feature_info_formats: Option<Vec<GetFeatureInfoFormat>>,
    pub rectangle: Option<Rectangle>,
    pub tiling_scheme: Option<Arc<dyn TilingScheme>>,
    pub ellipsoid: Option<Ellipsoid>,
    pub tile_width: Option<f64>,
    pub tile_height: Option<f64>,
    pub minimum_level: Option<u32>,
    pub maximum_level: Option<u32>,
    pub crs: Option<String>,
    pub srs: Option<String>,
    pub credit: Option<Credit>,
    pub subdomains: Option<Vec<String>>,
    pub clock: Option<Rc<RefCell<Clock>>>,
    pub times: Option<TimeIntervalCollection<DynamicData>>,
    pub tile_discard_policy: Option<Rc<dyn TileDiscardPolicy<ImageryImage>>>,
}

impl WebMapServiceImageryProviderOptions {
    pub fn new(url: Resource, layers: impl Into<String>) -> Self {
        Self {
            url,
            layers: layers.into(),
            get_feature_info_url: None,
            parameters: Vec::new(),
            get_feature_info_parameters: Vec::new(),
            enable_pick_features: None,
            get_feature_info_formats: None,
            rectangle: None,
            tiling_scheme: None,
            ellipsoid: None,
            tile_width: None,
            tile_height: None,
            minimum_level: None,
            maximum_level: None,
            crs: None,
            srs: None,
            credit: None,
            subdomains: None,
            clock: None,
            times: None,
            tile_discard_policy: None,
        }
    }
}

struct WmsCore<H: ImageHost> {
    loader: ImageryLoader<H>,
    resource: Resource,
    pick_resource: Resource,
    layers: String,
    tiling_scheme: Arc<dyn TilingScheme>,
    rectangle: Rectangle,
    tile_width: f64,
    tile_height: f64,
    minimum_level: u32,
    maximum_level: Option<u32>,
    discard_policy: Option<Rc<dyn TileDiscardPolicy<ImageryImage>>>,
    credit: Option<Credit>,
    formats: Vec<GetFeatureInfoFormat>,
    enable_pick_features: Cell<bool>,
    subdomains: Option<Vec<String>>,
}

impl<H: ImageHost + 'static> WmsCore<H> {
    fn provider(&self, dynamic: Option<&[(String, String)]>) -> Result<UrlTemplateImageryProvider<H>, RuntimeError> {
        let resource = self.resource.clone();
        if let Some(data) = dynamic {
            resource.set_query_parameters(&query_parameters(data), false);
        }
        let pick_resource = self.pick_resource.clone();
        if let Some(data) = dynamic {
            pick_resource.set_query_parameters(&query_parameters(data), false);
        }
        let mut options = UrlTemplateImageryProviderOptions::new(resource);
        options.pick_features_url = Some(pick_resource);
        options.tiling_scheme = Some(self.tiling_scheme.clone());
        options.rectangle = Some(self.rectangle);
        options.tile_width = Some(self.tile_width);
        options.tile_height = Some(self.tile_height);
        options.minimum_level = Some(self.minimum_level);
        options.maximum_level = self.maximum_level;
        options.subdomains = self.subdomains.clone();
        options.tile_discard_policy = self.discard_policy.clone();
        options.credit = self.credit.clone();
        options.get_feature_info_formats = self.formats.clone();
        options.enable_pick_features = Some(self.enable_pick_features.get());
        UrlTemplateImageryProvider::new(
            ImageryLoader::new(self.loader.client.clone(), self.loader.host.clone()),
            options,
        )
    }
}

pub struct WebMapServiceImageryProvider<H: ImageHost> {
    core: Rc<WmsCore<H>>,
    inner: UrlTemplateImageryProvider<H>,
    time_dynamic: Option<TimeDynamicHandle>,
    clock: Option<Rc<RefCell<Clock>>>,
    get_feature_info_url: Resource,
    error_event: ImageryErrorEvent,
}

impl<H: ImageHost + 'static> WebMapServiceImageryProvider<H> {
    pub fn new(loader: ImageryLoader<H>, options: WebMapServiceImageryProviderOptions) -> Result<Self, RuntimeError> {
        let mut options = options;
        if options.times.is_some() && options.clock.is_none() {
            return Err(RuntimeError::new("options.times was specified, so options.clock is required."));
        }
        let get_feature_info_url = options.get_feature_info_url.clone().unwrap_or_else(|| options.url.clone());
        let resource = options.url.clone();
        let pick_resource = get_feature_info_url.clone();
        resource.set_query_parameters(&default_parameters(), true);
        pick_resource.set_query_parameters(&feature_info_default_parameters(), true);
        if !options.parameters.is_empty() {
            resource.set_query_parameters(&lowercase_parameters(&options.parameters), false);
        }
        if !options.get_feature_info_parameters.is_empty() {
            pick_resource.set_query_parameters(&lowercase_parameters(&options.get_feature_info_parameters), false);
        }

        let tiling_scheme = options.tiling_scheme.clone().unwrap_or_else(|| {
            Arc::new(GeographicTilingScheme::new(
                options.ellipsoid.unwrap_or(Ellipsoid::WGS84),
                Rectangle::MAX_VALUE,
                2,
                1,
            ))
        });
        let version = query_value(&resource, "version").unwrap_or_default();
        let version = parse_float_prefix(&version);
        let mut parameters = vec![
            ("layers".to_owned(), options.layers.clone()),
            ("bbox".to_owned(), "{westProjected},{southProjected},{eastProjected},{northProjected}".into()),
            ("width".to_owned(), "{width}".into()),
            ("height".to_owned(), "{height}".into()),
        ];
        if version >= 1.3 {
            let crs = options.crs.clone().unwrap_or_else(|| {
                if tiling_scheme.projection().is_geographic() {
                    "CRS:84".into()
                } else {
                    "EPSG:3857".into()
                }
            });
            parameters.push(("crs".into(), crs.clone()));
            let parts: Vec<&str> = crs.split(':').collect();
            if parts.len() == 2 && parts[0] == "EPSG" {
                if let Ok(code) = parts[1].parse::<u32>() {
                    if (4000..5000).contains(&code) && !EXCLUDES_REVERSE_AXIS.contains(&code)
                        || INCLUDES_REVERSE_AXIS.contains(&code)
                    {
                        parameters[1].1 = "{southProjected},{westProjected},{northProjected},{eastProjected}".into();
                    }
                }
            }
        } else {
            let srs = options.srs.clone().unwrap_or_else(|| {
                if tiling_scheme.projection().is_geographic() {
                    "EPSG:4326".into()
                } else {
                    "EPSG:3857".into()
                }
            });
            parameters.push(("srs".into(), srs));
        }
        resource.set_query_parameters(&string_parameters(&parameters), true);
        pick_resource.set_query_parameters(&string_parameters(&parameters), true);

        let mut pick_parameters =
            vec![("query_layers".to_owned(), options.layers.clone()), ("info_format".to_owned(), "{format}".into())];
        if version >= 1.3 {
            pick_parameters.push(("i".into(), "{i}".into()));
            pick_parameters.push(("j".into(), "{j}".into()));
        } else {
            pick_parameters.push(("x".into(), "{i}".into()));
            pick_parameters.push(("y".into(), "{j}".into()));
        }
        pick_resource.set_query_parameters(&string_parameters(&pick_parameters), true);

        let formats = options.get_feature_info_formats.clone().unwrap_or_else(default_get_feature_info_formats);
        let proposed = options.rectangle.unwrap_or(*tiling_scheme.rectangle());
        let rectangle = proposed
            .intersection(tiling_scheme.rectangle())
            .ok_or_else(|| RuntimeError::new("The rectangle does not intersect the tiling scheme."))?;
        let core = Rc::new(WmsCore {
            loader,
            resource,
            pick_resource,
            layers: options.layers.clone(),
            tiling_scheme,
            rectangle,
            tile_width: options.tile_width.unwrap_or(256.0),
            tile_height: options.tile_height.unwrap_or(256.0),
            minimum_level: options.minimum_level.unwrap_or(0),
            maximum_level: options.maximum_level,
            discard_policy: options.tile_discard_policy.clone(),
            credit: options.credit.clone(),
            formats,
            enable_pick_features: Cell::new(options.enable_pick_features.unwrap_or(true)),
            subdomains: options.subdomains.clone(),
        });
        let inner = core.provider(None)?;

        let time_dynamic = options.times.take().map(|times| {
            let weak = Rc::downgrade(&core);
            let request: super::time_dynamic_imagery::RequestImageFunction<DynamicData> =
                Rc::new(move |x, y, level, interval| {
                    let core = weak.upgrade()?;
                    let provider = core.provider(interval.and_then(|i| i.data.as_deref())).ok()?;
                    provider.request_image(x, y, level)
                });
            Rc::new(RefCell::new(TimeDynamicImagery::new(TimeDynamicImageryOptions {
                times,
                request_image_function: request,
                reload_function: Rc::new(|| {}),
            })))
        });
        let error_event = inner.error_event().clone();
        Ok(Self { core, inner, time_dynamic, clock: options.clock, get_feature_info_url, error_event })
    }

    pub fn url(&self) -> String {
        self.core.resource.options().url.clone()
    }
    pub fn layers(&self) -> &str {
        &self.core.layers
    }
    pub fn get_feature_info_url(&self) -> &Resource {
        &self.get_feature_info_url
    }
    pub fn clock(&self) -> Option<Rc<RefCell<Clock>>> {
        self.clock.clone()
    }
    pub fn set_clock(&mut self, clock: Rc<RefCell<Clock>>) {
        self.clock = Some(clock);
    }
    pub fn current_interval_data(&self) -> Option<DynamicData> {
        self.time_dynamic.as_ref()?.borrow().current_interval().and_then(|interval| interval.data.clone())
    }

    /// The URL a tile request uses, after the current time interval.
    pub fn tile_url(&self, x: i32, y: i32, level: u32) -> Option<String> {
        let provider = self.core.provider(self.current_interval_data().as_deref()).ok()?;
        Some(provider.tile_url(x, y, level))
    }
    pub fn enable_pick_features(&self) -> bool {
        self.core.enable_pick_features.get()
    }
    pub fn set_enable_pick_features(&self, value: bool) {
        self.core.enable_pick_features.set(value);
        self.inner.set_enable_pick_features(value);
    }

    /// Advances time-dynamic state from the configured clock. Providers call
    /// this before serving a tile; hosts may also call it on their tick.
    pub fn update_time(&self) -> Option<Vec<(String, String)>> {
        let dynamic = self.time_dynamic.as_ref()?;
        let clock = self.clock.as_ref()?;
        let clock = clock.borrow();
        dynamic.borrow_mut().update(
            &clock.current_time(),
            clock.can_animate,
            clock.should_animate(),
            clock.multiplier(),
        );
        dynamic.borrow().current_interval().and_then(|interval| interval.data.clone())
    }
}

fn query_parameters(data: &[(String, String)]) -> Vec<(String, Option<QueryValue>)> {
    data.iter().map(|(k, v)| (k.clone(), Some(QueryValue::Single(v.clone())))).collect()
}

fn string_parameters(data: &[(String, String)]) -> Vec<(String, Option<QueryValue>)> {
    data.iter().map(|(k, v)| (k.clone(), Some(QueryValue::Single(v.clone())))).collect()
}

fn default_parameters() -> Vec<(String, Option<QueryValue>)> {
    DEFAULT_PARAMETERS.iter().map(|(k, v)| ((*k).to_owned(), Some(QueryValue::Single((*v).to_owned())))).collect()
}

fn feature_info_default_parameters() -> Vec<(String, Option<QueryValue>)> {
    GET_FEATURE_INFO_DEFAULT_PARAMETERS
        .iter()
        .map(|(k, v)| ((*k).to_owned(), Some(QueryValue::Single((*v).to_owned()))))
        .collect()
}

fn lowercase_parameters(data: &[(String, String)]) -> Vec<(String, Option<QueryValue>)> {
    data.iter().map(|(k, v)| (k.to_lowercase(), Some(QueryValue::Single(v.clone())))).collect()
}

/// JavaScript `parseFloat`'s leading numeric prefix ("1.3.0" -> 1.3).
fn parse_float_prefix(value: &str) -> f64 {
    let value = value.trim_start();
    let mut end = 0;
    let mut seen_dot = false;
    let mut seen_exp = false;
    for (i, ch) in value.char_indices() {
        let valid = match ch {
            '0'..='9' => true,
            '.' if !seen_dot && !seen_exp => {
                seen_dot = true;
                true
            }
            'e' | 'E' if !seen_exp && i > 0 => {
                seen_exp = true;
                true
            }
            '+' | '-' if i > 0 && value[..i].ends_with(['e', 'E']) => true,
            _ => false,
        };
        if !valid {
            break;
        }
        end = i + ch.len_utf8();
    }
    value[..end].parse().unwrap_or(f64::NAN)
}

fn query_value(resource: &Resource, key: &str) -> Option<String> {
    resource.options().query_parameters.iter().find(|(k, _)| k == key).and_then(|(_, v)| match v {
        Some(QueryValue::Single(v)) => Some(v.clone()),
        _ => None,
    })
}

impl<H: ImageHost + 'static> ImageryProvider for WebMapServiceImageryProvider<H> {
    fn rectangle(&self) -> &Rectangle {
        &self.core.rectangle
    }
    fn tile_width(&self) -> f64 {
        self.core.tile_width
    }
    fn tile_height(&self) -> f64 {
        self.core.tile_height
    }
    fn maximum_level(&self) -> Option<u32> {
        self.core.maximum_level
    }
    fn minimum_level(&self) -> u32 {
        self.core.minimum_level
    }
    fn tiling_scheme(&self) -> &dyn TilingScheme {
        self.core.tiling_scheme.as_ref()
    }
    fn tile_discard_policy(&self) -> Option<Rc<dyn TileDiscardPolicy<ImageryImage>>> {
        self.core.discard_policy.clone()
    }
    fn error_event(&self) -> &ImageryErrorEvent {
        &self.error_event
    }
    fn credit(&self) -> Option<&Credit> {
        self.core.credit.as_ref()
    }
    fn proxy(&self) -> Option<Arc<dyn crate::core::Proxy>> {
        self.core.resource.options().proxy.clone()
    }
    fn has_alpha_channel(&self) -> bool {
        self.inner.has_alpha_channel()
    }
    fn get_tile_credits(&self, x: i32, y: i32, level: u32) -> Option<Vec<Credit>> {
        self.inner.get_tile_credits(x, y, level)
    }
    fn request_image(&self, x: i32, y: i32, level: u32) -> Option<ImageryFuture> {
        let data = self.update_time();
        if let Some(dynamic) = &self.time_dynamic {
            let clock = self.clock.as_ref()?;
            let state = clock.borrow();
            if let Some(cached) = dynamic.borrow().get_from_cache(x, y, level) {
                return Some(cached);
            }
            let result = self.core.provider(data.as_deref()).ok()?.request_image(x, y, level);
            if result.is_some() {
                dynamic.borrow_mut().check_approaching_interval(
                    x,
                    y,
                    level,
                    &state.current_time(),
                    state.can_animate,
                    state.should_animate(),
                    state.multiplier(),
                    None,
                );
            }
            return result;
        }
        self.inner.request_image(x, y, level)
    }
    fn pick_features(&self, x: i32, y: i32, level: u32, longitude: f64, latitude: f64) -> Option<ImageryFeatureFuture> {
        if !self.core.enable_pick_features.get() {
            return None;
        }
        let data = self.update_time();
        if data.is_some() {
            return self.core.provider(data.as_deref()).ok()?.pick_features(x, y, level, longitude, latitude);
        }
        self.inner.pick_features(x, y, level, longitude, latitude)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_parameters_match_upstream() {
        assert_eq!(
            DEFAULT_PARAMETERS,
            [("service", "WMS"), ("version", "1.1.1"), ("request", "GetMap"), ("styles", ""), ("format", "image/jpeg")]
        );
        assert_eq!(default_get_feature_info_formats().len(), 3);
    }
}
