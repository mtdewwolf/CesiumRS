//! CesiumJS `WebMapTileServiceImageryProvider`: WMTS 1.0.0 KVP and RESTful
//! GetTile requests, static dimensions and time-dynamic dimensions.

use super::time_dynamic_imagery::{TimeDynamicImagery, TimeDynamicImageryOptions};
use super::{
    ImageryErrorEvent, ImageryFeatureFuture, ImageryFuture, ImageryImage, ImageryLoader, ImageryProvider,
    TileDiscardPolicy,
};
use crate::core::query::QueryValue;
use crate::core::{
    Clock, Credit, Ellipsoid, ImageHost, Rectangle, Resource, RuntimeError, TilingScheme, TimeInterval,
    TimeIntervalCollection, WebMercatorTilingScheme,
};
use std::{cell::RefCell, rc::Rc, sync::Arc};

pub type DynamicData = Vec<(String, String)>;
type TimeDynamicHandle = Rc<RefCell<TimeDynamicImagery<DynamicData>>>;
type Reload = Rc<dyn Fn()>;
type SharedReload = Rc<RefCell<Option<Reload>>>;

pub const DEFAULT_PARAMETERS: [(&str, &str); 3] = [("service", "WMTS"), ("version", "1.0.0"), ("request", "GetTile")];

#[derive()]
pub struct WebMapTileServiceImageryProviderOptions {
    pub url: Resource,
    pub format: Option<String>,
    pub layer: String,
    pub style: String,
    pub tile_matrix_set_id: String,
    pub tile_matrix_labels: Option<Vec<String>>,
    pub clock: Option<Rc<RefCell<Clock>>>,
    pub times: Option<TimeIntervalCollection<DynamicData>>,
    pub dimensions: Option<Vec<(String, String)>>,
    pub tile_width: Option<f64>,
    pub tile_height: Option<f64>,
    pub tiling_scheme: Option<Arc<dyn TilingScheme>>,
    pub rectangle: Option<Rectangle>,
    pub minimum_level: Option<u32>,
    pub maximum_level: Option<u32>,
    pub ellipsoid: Option<Ellipsoid>,
    pub credit: Option<Credit>,
    pub subdomains: Option<Vec<String>>,
    pub tile_discard_policy: Option<Rc<dyn TileDiscardPolicy<ImageryImage>>>,
}

impl WebMapTileServiceImageryProviderOptions {
    pub fn new(
        url: Resource,
        layer: impl Into<String>,
        style: impl Into<String>,
        tile_matrix_set_id: impl Into<String>,
    ) -> Self {
        Self {
            url,
            format: None,
            layer: layer.into(),
            style: style.into(),
            tile_matrix_set_id: tile_matrix_set_id.into(),
            tile_matrix_labels: None,
            clock: None,
            times: None,
            dimensions: None,
            tile_width: None,
            tile_height: None,
            tiling_scheme: None,
            rectangle: None,
            minimum_level: None,
            maximum_level: None,
            ellipsoid: None,
            credit: None,
            subdomains: None,
            tile_discard_policy: None,
        }
    }
}

struct WmtsCore<H: ImageHost> {
    resource: Resource,
    loader: ImageryLoader<H>,
    use_kvp: bool,
    layer: String,
    style: String,
    tile_matrix_set_id: String,
    tile_matrix_labels: Option<Vec<String>>,
    format: String,
    subdomains: Vec<String>,
    dimensions: RefCell<Option<Vec<(String, String)>>>,
    discard_policy: Option<Rc<dyn TileDiscardPolicy<ImageryImage>>>,
}

impl<H: ImageHost + 'static> WmtsCore<H> {
    fn tile_matrix(&self, level: u32) -> Option<String> {
        match &self.tile_matrix_labels {
            Some(labels) => labels.get(level as usize).cloned(),
            None => Some(level.to_string()),
        }
    }

    fn image_resource(
        &self,
        x: i32,
        y: i32,
        level: u32,
        dynamic: Option<&[(String, String)]>,
    ) -> Result<Resource, RuntimeError> {
        let subdomain = &self.subdomains[(x + y + level as i32).rem_euclid(self.subdomains.len() as i32) as usize];
        if !self.use_kvp {
            let resource = self
                .resource
                .get_derived_resource(Default::default())
                .ok_or_else(|| RuntimeError::new("The WMTS resource could not be derived."))?;
            resource.set_template_values(
                &[
                    ("TileRow".into(), y.to_string()),
                    ("TileCol".into(), x.to_string()),
                    ("s".into(), subdomain.clone()),
                ],
                false,
            );
            if let Some(tile_matrix) = self.tile_matrix(level) {
                resource.set_template_values(&[("TileMatrix".into(), tile_matrix)], false);
            }
            if let Some(dimensions) = &*self.dimensions.borrow() {
                resource.set_template_values(dimensions, false);
            }
            if let Some(dimensions) = dynamic {
                resource.set_template_values(dimensions, false);
            }
            Ok(resource)
        } else {
            let mut query: Vec<(String, Option<QueryValue>)> = vec![
                (
                    "tilematrix".into(),
                    Some(QueryValue::Single(self.tile_matrix(level).unwrap_or_else(|| "undefined".into()))),
                ),
                ("layer".into(), Some(QueryValue::Single(self.layer.clone()))),
                ("style".into(), Some(QueryValue::Single(self.style.clone()))),
                ("tilerow".into(), Some(QueryValue::Single(y.to_string()))),
                ("tilecol".into(), Some(QueryValue::Single(x.to_string()))),
                ("tilematrixset".into(), Some(QueryValue::Single(self.tile_matrix_set_id.clone()))),
                ("format".into(), Some(QueryValue::Single(self.format.clone()))),
            ];
            // Cesium's `combine(query, dimensions)` lets dimensions override.
            let borrowed_dims = self.dimensions.borrow();
            for (key, value) in borrowed_dims.iter().flatten().chain(dynamic.into_iter().flatten()) {
                set_query(&mut query, key, value);
            }
            let resource = self
                .resource
                .get_derived_resource(crate::core::resource::DerivedResourceOptions {
                    template_values: vec![("s".into(), subdomain.clone())],
                    ..Default::default()
                })
                .ok_or_else(|| RuntimeError::new("The WMTS resource could not be derived."))?;
            resource.set_query_parameters(&query, false);
            Ok(resource)
        }
    }

    fn request_image(
        &self,
        x: i32,
        y: i32,
        level: u32,
        dynamic: Option<&[(String, String)]>,
    ) -> Option<ImageryFuture> {
        let resource = self.image_resource(x, y, level, dynamic).ok()?;
        let has_policy = self.discard_policy.is_some();
        self.loader.load_image(resource, has_policy)
    }
}

pub struct WebMapTileServiceImageryProvider<H: ImageHost> {
    core: Rc<WmtsCore<H>>,
    dimensions: Option<Vec<(String, String)>>,
    tiling_scheme: Arc<dyn TilingScheme>,
    tile_width: f64,
    tile_height: f64,
    minimum_level: u32,
    maximum_level: Option<u32>,
    rectangle: Rectangle,
    credit: Option<Credit>,
    error_event: ImageryErrorEvent,
    time_dynamic: Option<TimeDynamicHandle>,
    clock: Option<Rc<RefCell<Clock>>>,
    reload: SharedReload,
}

impl<H: ImageHost + 'static> WebMapTileServiceImageryProvider<H> {
    pub fn new(
        loader: ImageryLoader<H>,
        options: WebMapTileServiceImageryProviderOptions,
    ) -> Result<Self, RuntimeError> {
        if options.times.is_some() && options.clock.is_none() {
            return Err(RuntimeError::new("options.times was specified, so options.clock is required."));
        }
        let resource = options.url.clone();
        let url = resource.url();
        let braces = url.matches('{').count();
        let use_kvp = braces == 0 || (braces == 1 && url.contains("{s}"));
        if use_kvp {
            resource.set_query_parameters(
                &DEFAULT_PARAMETERS
                    .iter()
                    .map(|(k, v)| ((*k).to_owned(), Some(QueryValue::Single((*v).to_owned()))))
                    .collect::<Vec<_>>(),
                false,
            );
        } else {
            resource.set_template_values(
                &[
                    ("style".into(), options.style.clone()),
                    ("Style".into(), options.style.clone()),
                    ("TileMatrixSet".into(), options.tile_matrix_set_id.clone()),
                ],
                false,
            );
        }
        let tiling_scheme = options.tiling_scheme.clone().unwrap_or_else(|| {
            Arc::new(WebMercatorTilingScheme::new(options.ellipsoid.unwrap_or(Ellipsoid::WGS84), 1, 1, None))
        });
        let rectangle = options.rectangle.unwrap_or(*tiling_scheme.rectangle());
        let subdomains = match options.subdomains {
            Some(list) if !list.is_empty() => list,
            _ => vec!["a".into(), "b".into(), "c".into()],
        };
        let core = Rc::new(WmtsCore {
            resource,
            loader,
            use_kvp,
            layer: options.layer,
            style: options.style,
            tile_matrix_set_id: options.tile_matrix_set_id,
            tile_matrix_labels: options.tile_matrix_labels,
            format: options.format.unwrap_or_else(|| "image/jpeg".into()),
            subdomains,
            dimensions: RefCell::new(options.dimensions.clone()),
            discard_policy: options.tile_discard_policy,
        });
        let reload: SharedReload = Rc::new(RefCell::new(None));
        let time_dynamic = options.times.map(|times| {
            let weak = Rc::downgrade(&core);
            let request: super::time_dynamic_imagery::RequestImageFunction<DynamicData> =
                Rc::new(move |x, y, level, interval| {
                    let core = weak.upgrade()?;
                    core.request_image(x, y, level, interval.and_then(|i| i.data.as_deref()))
                });
            Rc::new(RefCell::new(TimeDynamicImagery::new(TimeDynamicImageryOptions {
                times,
                request_image_function: request,
                reload_function: {
                    let reload = reload.clone();
                    Rc::new(move || {
                        if let Some(f) = reload.borrow().as_ref() {
                            f();
                        }
                    })
                },
            })))
        });
        Ok(Self {
            core,
            dimensions: options.dimensions,
            tiling_scheme,
            tile_width: options.tile_width.unwrap_or(256.0),
            tile_height: options.tile_height.unwrap_or(256.0),
            minimum_level: options.minimum_level.unwrap_or(0),
            maximum_level: options.maximum_level,
            rectangle,
            credit: options.credit,
            error_event: Rc::new(RefCell::new(Default::default())),
            time_dynamic,
            clock: options.clock,
            reload,
        })
    }

    pub fn url(&self) -> String {
        self.core.resource.url()
    }
    pub fn layer(&self) -> &str {
        &self.core.layer
    }
    pub fn style(&self) -> &str {
        &self.core.style
    }
    pub fn tile_matrix_set_id(&self) -> &str {
        &self.core.tile_matrix_set_id
    }
    pub fn format(&self) -> &str {
        &self.core.format
    }
    pub fn use_kvp(&self) -> bool {
        self.core.use_kvp
    }
    pub fn dimensions(&self) -> Option<&[(String, String)]> {
        self.dimensions.as_deref()
    }
    pub fn set_dimensions(&mut self, dimensions: Option<Vec<(String, String)>>) {
        if self.dimensions != dimensions {
            self.dimensions = dimensions.clone();
            *self.core.dimensions.borrow_mut() = dimensions;
            if let Some(reload) = self.reload.borrow().as_ref() {
                reload();
            }
        }
    }
    pub fn set_reload(&self, reload: Option<Reload>) {
        *self.reload.borrow_mut() = reload;
    }
    pub fn clock(&self) -> Option<Rc<RefCell<Clock>>> {
        self.clock.clone()
    }
    pub fn set_clock(&mut self, clock: Rc<RefCell<Clock>>) {
        self.clock = Some(clock);
    }
    pub fn current_interval_data(&self) -> Option<DynamicData> {
        self.time_dynamic.as_ref()?.borrow().current_interval().and_then(|i| i.data.clone())
    }
    pub fn current_interval(&self) -> Option<TimeInterval<DynamicData>> {
        self.time_dynamic.as_ref()?.borrow().current_interval().cloned()
    }
    pub fn current_interval_index(&self) -> isize {
        self.time_dynamic.as_ref().map_or(-1, |d| d.borrow().current_interval_index())
    }
    pub fn cached_interval_count(&self) -> usize {
        self.time_dynamic.as_ref().map_or(0, |d| d.borrow().cached_interval_count())
    }
    pub fn pending_preload_count(&self) -> usize {
        self.time_dynamic.as_ref().map_or(0, |d| d.borrow().pending_preload_count())
    }
    pub fn set_times(&mut self, times: TimeIntervalCollection<DynamicData>) {
        if let Some(dynamic) = &self.time_dynamic {
            dynamic.borrow_mut().set_times(times);
            self.update_time();
        }
    }

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
        self.current_interval_data()
    }

    /// The URL a tile request would use. Kept public for deterministic tests.
    pub fn tile_url(&self, x: i32, y: i32, level: u32) -> Result<String, RuntimeError> {
        let dynamic = self.current_interval_data();
        Ok(self.core.image_resource(x, y, level, dynamic.as_deref())?.get_url_component(true, false))
    }
}

fn set_query(query: &mut Vec<(String, Option<QueryValue>)>, key: &str, value: &str) {
    if let Some(slot) = query.iter_mut().find(|(k, _)| k == key) {
        slot.1 = Some(QueryValue::Single(value.to_owned()));
    } else {
        query.push((key.to_owned(), Some(QueryValue::Single(value.to_owned()))));
    }
}

impl<H: ImageHost + 'static> ImageryProvider for WebMapTileServiceImageryProvider<H> {
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
    fn tile_discard_policy(&self) -> Option<Rc<dyn TileDiscardPolicy<ImageryImage>>> {
        self.core.discard_policy.clone()
    }
    fn error_event(&self) -> &ImageryErrorEvent {
        &self.error_event
    }
    fn credit(&self) -> Option<&Credit> {
        self.credit.as_ref()
    }
    fn proxy(&self) -> Option<Arc<dyn crate::core::Proxy>> {
        self.core.resource.options().proxy.clone()
    }
    fn has_alpha_channel(&self) -> bool {
        true
    }
    fn request_image(&self, x: i32, y: i32, level: u32) -> Option<ImageryFuture> {
        if let Some(time_dynamic) = &self.time_dynamic {
            let dynamic = self.update_time();
            let clock = self.clock.as_ref()?;
            let state = clock.borrow();
            let result = if let Some(cached) = time_dynamic.borrow().get_from_cache(x, y, level) {
                Some(cached)
            } else {
                self.core.request_image(x, y, level, dynamic.as_deref())
            };
            if result.is_some() {
                time_dynamic.borrow_mut().check_approaching_interval(
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
        self.core.request_image(x, y, level, None)
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn default_parameters_match_upstream() {
        assert_eq!(DEFAULT_PARAMETERS, [("service", "WMTS"), ("version", "1.0.0"), ("request", "GetTile")]);
    }
}
