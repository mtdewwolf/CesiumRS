//! CesiumJS `ArcGisMapServerImageryProvider`: pre-cached MapServer tiles or
//! dynamic Export requests, copyright credits and Identify feature picking.

use super::{
    DiscardMissingTileImagePolicy, DiscardMissingTileImagePolicyOptions, ImageryErrorEvent, ImageryFeatureFuture,
    ImageryFuture, ImageryImage, ImageryLayerFeatureInfo, ImageryLoader, ImageryProvider, TileDiscardPolicy,
};
use crate::core::query::QueryValue;
use crate::core::MapProjection;
use crate::core::{
    Cartesian2, Cartesian3, Cartographic, Credit, Ellipsoid, GeographicTilingScheme, ImageHost, Rectangle, Resource,
    RuntimeError, TilingScheme, WebMercatorProjection, WebMercatorTilingScheme,
};
use serde_json::Value;
use std::{
    cell::{Cell, RefCell},
    rc::Rc,
    sync::Arc,
};

pub const DEFAULT_TILE_WIDTH: f64 = 256.0;
pub const DEFAULT_TILE_HEIGHT: f64 = 256.0;

#[derive(Clone)]
pub struct ArcGisMapServerImageryProviderOptions {
    pub token: Option<String>,
    pub use_pre_cached_tiles_if_available: bool,
    pub layers: Option<String>,
    pub enable_pick_features: Option<bool>,
    pub rectangle: Option<Rectangle>,
    pub tiling_scheme: Option<Arc<dyn TilingScheme>>,
    pub ellipsoid: Option<Ellipsoid>,
    pub credit: Option<Credit>,
    pub tile_width: Option<f64>,
    pub tile_height: Option<f64>,
    pub maximum_level: Option<u32>,
    pub tile_discard_policy: Option<Rc<dyn TileDiscardPolicy<ImageryImage>>>,
}

impl Default for ArcGisMapServerImageryProviderOptions {
    fn default() -> Self {
        Self {
            token: None,
            use_pre_cached_tiles_if_available: true,
            layers: None,
            enable_pick_features: None,
            rectangle: None,
            tiling_scheme: None,
            ellipsoid: None,
            credit: None,
            tile_width: None,
            tile_height: None,
            maximum_level: None,
            tile_discard_policy: None,
        }
    }
}

/// The metadata-derived settings of a MapServer.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ArcGisMetadata {
    pub use_tiles: bool,
    pub tile_width: f64,
    pub tile_height: f64,
    pub maximum_level: Option<u32>,
    pub rectangle: Option<Rectangle>,
    pub wkid: Option<i32>,
    pub copyright_text: Option<String>,
    pub lod_count: usize,
}

pub struct ArcGisMapServerImageryProvider<H: ImageHost> {
    resource: Resource,
    loader: ImageryLoader<H>,
    tiling_scheme: Arc<dyn TilingScheme>,
    use_tiles: bool,
    rectangle: Rectangle,
    layers: Option<String>,
    credit: Option<Credit>,
    tile_credits: Option<Vec<Credit>>,
    tile_width: f64,
    tile_height: f64,
    maximum_level: Option<u32>,
    tile_discard_policy: Option<Rc<dyn TileDiscardPolicy<ImageryImage>>>,
    enable_pick_features: Cell<bool>,
    error_event: ImageryErrorEvent,
}

impl<H: ImageHost + 'static> ArcGisMapServerImageryProvider<H> {
    pub async fn from_url(
        loader: ImageryLoader<H>,
        url: Resource,
        options: ArcGisMapServerImageryProviderOptions,
    ) -> Result<Self, RuntimeError> {
        let resource = url;
        resource.append_forward_slash();
        if let Some(token) = &options.token {
            resource.set_query_parameters(&vec![("token".into(), Some(QueryValue::Single(token.clone())))], false);
        }
        let ellipsoid = options.ellipsoid.unwrap_or(Ellipsoid::WGS84);
        let base_scheme: Arc<dyn TilingScheme> = options
            .tiling_scheme
            .clone()
            .unwrap_or_else(|| Arc::new(GeographicTilingScheme::new(ellipsoid, Rectangle::MAX_VALUE, 2, 1)));

        let mut builder = ArcGisBuilder {
            use_tiles: options.use_pre_cached_tiles_if_available,
            tiling_scheme: base_scheme.clone(),
            rectangle: options.rectangle.unwrap_or(*base_scheme.rectangle()),
            credit: options.credit.clone(),
            tile_credits: None,
            tile_width: options.tile_width.unwrap_or(DEFAULT_TILE_WIDTH),
            tile_height: options.tile_height.unwrap_or(DEFAULT_TILE_HEIGHT),
            maximum_level: options.maximum_level,
            ellipsoid,
            layers: options.layers.clone(),
        };

        if options.use_pre_cached_tiles_if_available {
            let json_resource = resource
                .get_derived_resource(crate::core::resource::DerivedResourceOptions {
                    query_parameters: vec![("f".into(), Some(QueryValue::Single("json".into())))],
                    ..Default::default()
                })
                .ok_or_else(|| RuntimeError::new("Invalid ArcGIS metadata resource."))?;
            let fetch = loader
                .client
                .fetch_json(&json_resource)
                .ok_or_else(|| RuntimeError::new("ArcGIS metadata request was throttled."))?;
            let json = fetch
                .await
                .map_err(|e| RuntimeError::new(format!("An error occurred while accessing {}: {e}", resource.url())))?;
            metadata_success(&json, &mut builder)?;
        }

        let mut policy = options.tile_discard_policy.clone();
        if policy.is_none() && builder.use_tiles {
            let max_level = builder.maximum_level.unwrap_or(0);
            let missing_url = build_image_resource_with(
                &resource,
                builder.use_tiles,
                builder.tiling_scheme.as_ref(),
                builder.layers.as_deref(),
                builder.tile_width,
                builder.tile_height,
                0,
                0,
                max_level,
            );
            let missing = match loader.load_image(missing_url, true) {
                Some(fetch) => fetch.await.ok(),
                None => None,
            };
            if let Some(ImageryImage::Image(frame)) = &missing {
                if let Some(image) = frame.get::<H::Image>() {
                    let inner = DiscardMissingTileImagePolicy::new(
                        loader.host.clone(),
                        DiscardMissingTileImagePolicyOptions {
                            pixels_to_check: vec![
                                Cartesian2::new(0.0, 0.0),
                                Cartesian2::new(200.0, 20.0),
                                Cartesian2::new(20.0, 200.0),
                                Cartesian2::new(80.0, 110.0),
                                Cartesian2::new(160.0, 130.0),
                            ],
                            disable_check_if_all_pixels_are_transparent: true,
                        },
                        Some(image),
                    );
                    policy = Some(Rc::new(ArcGisDiscardPolicy { inner }));
                }
            }
        }

        Ok(Self {
            resource,
            loader,
            tiling_scheme: builder.tiling_scheme,
            use_tiles: builder.use_tiles,
            rectangle: builder.rectangle,
            layers: options.layers.clone(),
            credit: builder.credit,
            tile_credits: builder.tile_credits,
            tile_width: builder.tile_width,
            tile_height: builder.tile_height,
            maximum_level: builder.maximum_level,
            tile_discard_policy: policy,
            enable_pick_features: Cell::new(options.enable_pick_features.unwrap_or(true)),
            error_event: Rc::new(RefCell::new(Default::default())),
        })
    }

    pub fn url(&self) -> String {
        self.resource.options().url.clone()
    }
    pub fn token(&self) -> Option<String> {
        self.resource.options().query_parameters.iter().find(|(k, _)| k == "token").and_then(|(_, v)| match v {
            Some(QueryValue::Single(v)) => Some(v.clone()),
            _ => None,
        })
    }
    pub fn layers(&self) -> Option<&str> {
        self.layers.as_deref()
    }
    pub fn using_precached_tiles(&self) -> bool {
        self.use_tiles
    }
    pub fn enable_pick_features(&self) -> bool {
        self.enable_pick_features.get()
    }
    pub fn set_enable_pick_features(&self, value: bool) {
        self.enable_pick_features.set(value);
    }
    pub fn tile_url(&self, x: i32, y: i32, level: u32) -> String {
        build_image_resource_with(
            &self.resource,
            self.use_tiles,
            self.tiling_scheme.as_ref(),
            self.layers.as_deref(),
            self.tile_width,
            self.tile_height,
            x,
            y,
            level,
        )
        .get_url_component(true, false)
    }
}

struct ArcGisBuilder {
    use_tiles: bool,
    tiling_scheme: Arc<dyn TilingScheme>,
    rectangle: Rectangle,
    credit: Option<Credit>,
    tile_credits: Option<Vec<Credit>>,
    tile_width: f64,
    tile_height: f64,
    maximum_level: Option<u32>,
    ellipsoid: Ellipsoid,
    // Kept separately because `buildImageResource` reads `layers` from the
    // provider, not the builder.
    layers: Option<String>,
}

impl Default for ArcGisBuilder {
    fn default() -> Self {
        Self {
            use_tiles: true,
            tiling_scheme: Arc::new(GeographicTilingScheme::default()),
            rectangle: Rectangle::MAX_VALUE,
            credit: None,
            tile_credits: None,
            tile_width: DEFAULT_TILE_WIDTH,
            tile_height: DEFAULT_TILE_HEIGHT,
            maximum_level: None,
            ellipsoid: Ellipsoid::WGS84,
            layers: None,
        }
    }
}

fn metadata_success(data: &Value, builder: &mut ArcGisBuilder) -> Result<(), RuntimeError> {
    let tile_info = &data["tileInfo"];
    if tile_info.is_null() {
        builder.use_tiles = false;
    } else {
        builder.tile_width = tile_info["rows"].as_f64().ok_or_else(|| RuntimeError::new("tileInfo has no rows"))?;
        builder.tile_height = tile_info["cols"].as_f64().ok_or_else(|| RuntimeError::new("tileInfo has no cols"))?;
        let wkid = tile_info["spatialReference"]["wkid"].as_i64().unwrap_or(0) as i32;
        if wkid == 102100 || wkid == 102113 {
            builder.tiling_scheme = Arc::new(WebMercatorTilingScheme::new(builder.ellipsoid, 1, 1, None));
        } else if wkid == 4326 {
            builder.tiling_scheme =
                Arc::new(GeographicTilingScheme::new(builder.ellipsoid, Rectangle::MAX_VALUE, 2, 1));
        } else {
            return Err(RuntimeError::new(format!("Tile spatial reference WKID {wkid} is not supported.")));
        }
        let lods = tile_info["lods"].as_array().map(Vec::len).unwrap_or(0);
        builder.maximum_level = Some(lods.saturating_sub(1) as u32);
        builder.rectangle = match full_extent_rectangle(data, builder)? {
            Some(rectangle) => rectangle,
            None => *builder.tiling_scheme.rectangle(),
        };
        builder.use_tiles = true;
    }

    if let Some(text) = data["copyrightText"].as_str().filter(|text| !text.is_empty()) {
        if builder.credit.is_some() {
            builder.tile_credits = Some(vec![Credit::new(text, false)]);
        } else {
            builder.credit = Some(Credit::new(text, false));
        }
    }
    Ok(())
}

fn full_extent_rectangle(data: &Value, builder: &ArcGisBuilder) -> Result<Option<Rectangle>, RuntimeError> {
    let extent = &data["fullExtent"];
    if extent.is_null() {
        return Ok(None);
    }
    let wkid = extent["spatialReference"]["wkid"].as_i64().unwrap_or(0) as i32;
    if wkid == 102100 || wkid == 102113 {
        let max = builder.tiling_scheme.ellipsoid().maximum_radius * std::f64::consts::PI;
        let clamp = |value: Option<f64>| value.unwrap_or(f64::NAN);
        let xmin = clamp(extent["xmin"].as_f64()).max(-max);
        let ymin = clamp(extent["ymin"].as_f64()).max(-max);
        let xmax = clamp(extent["xmax"].as_f64()).min(max);
        let ymax = clamp(extent["ymax"].as_f64()).min(max);
        let projection = WebMercatorProjection::new(*builder.tiling_scheme.ellipsoid());
        let sw = projection.unproject(Cartesian3::new(xmin, ymin, 0.0));
        let ne = projection.unproject(Cartesian3::new(xmax, ymax, 0.0));
        Ok(Some(Rectangle::new(sw.longitude, sw.latitude, ne.longitude, ne.latitude)))
    } else if wkid == 4326 {
        Ok(Some(Rectangle::from_degrees(
            extent["xmin"].as_f64().unwrap_or(f64::NAN),
            extent["ymin"].as_f64().unwrap_or(f64::NAN),
            extent["xmax"].as_f64().unwrap_or(f64::NAN),
            extent["ymax"].as_f64().unwrap_or(f64::NAN),
        )))
    } else {
        Err(RuntimeError::new(format!("fullExtent.spatialReference WKID {wkid} is not supported.")))
    }
}

#[allow(clippy::too_many_arguments)]
fn build_image_resource_with(
    service: &Resource,
    use_tiles: bool,
    tiling_scheme: &dyn TilingScheme,
    layers: Option<&str>,
    tile_width: f64,
    tile_height: f64,
    x: i32,
    y: i32,
    level: u32,
) -> Resource {
    let base = service.get_url_component(false, false);
    let base = if base.ends_with('/') { base } else { format!("{base}/") };
    if use_tiles {
        return service
            .get_derived_resource(crate::core::resource::DerivedResourceOptions {
                url: Some(format!("{base}tile/{level}/{y}/{x}")),
                ..Default::default()
            })
            .unwrap_or_else(|| service.clone());
    }
    let native = tiling_scheme.tile_xy_to_native_rectangle(f64::from(x), f64::from(y), level);
    let bbox = format!("{},{},{},{}", native.west, native.south, native.east, native.north);
    let mut query = vec![
        ("bbox".to_owned(), Some(QueryValue::Single(bbox))),
        ("size".to_owned(), Some(QueryValue::Single(format!("{tile_width},{tile_height}")))),
        ("format".to_owned(), Some(QueryValue::Single("png32".into()))),
        ("transparent".to_owned(), Some(QueryValue::Single("true".into()))),
        ("f".to_owned(), Some(QueryValue::Single("image".into()))),
    ];
    let sr = if tiling_scheme.projection().is_geographic() { "4326" } else { "3857" };
    query.push(("bboxSR".into(), Some(QueryValue::Single(sr.into()))));
    query.push(("imageSR".into(), Some(QueryValue::Single(sr.into()))));
    if let Some(layers) = layers {
        query.push(("layers".into(), Some(QueryValue::Single(format!("show:{layers}")))));
    }
    service
        .get_derived_resource(crate::core::resource::DerivedResourceOptions {
            url: Some(format!("{base}export")),
            query_parameters: query,
            ..Default::default()
        })
        .unwrap_or_else(|| service.clone())
}

struct ArcGisDiscardPolicy<H: ImageHost> {
    inner: DiscardMissingTileImagePolicy<H>,
}

impl<H: ImageHost + 'static> TileDiscardPolicy<ImageryImage> for ArcGisDiscardPolicy<H> {
    fn is_ready(&self) -> bool {
        self.inner.is_ready()
    }
    fn should_discard_image(&self, image: &ImageryImage) -> bool {
        match image {
            ImageryImage::Image(frame) => {
                frame.get::<H::Image>().is_some_and(|image| self.inner.should_discard_image(image))
            }
            _ => false,
        }
    }
}

impl<H: ImageHost + 'static> ImageryProvider for ArcGisMapServerImageryProvider<H> {
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
        self.credit.as_ref()
    }
    fn proxy(&self) -> Option<Arc<dyn crate::core::Proxy>> {
        self.resource.options().proxy.clone()
    }
    fn has_alpha_channel(&self) -> bool {
        true
    }
    fn get_tile_credits(&self, _x: i32, _y: i32, _level: u32) -> Option<Vec<Credit>> {
        self.tile_credits.clone()
    }
    fn request_image(&self, x: i32, y: i32, level: u32) -> Option<ImageryFuture> {
        let resource = build_image_resource_with(
            &self.resource,
            self.use_tiles,
            self.tiling_scheme.as_ref(),
            self.layers.as_deref(),
            self.tile_width,
            self.tile_height,
            x,
            y,
            level,
        );
        let has_policy = self.tile_discard_policy.is_some();
        let fetch = self.loader.load_image(resource, has_policy)?;
        Some(fetch)
    }
    fn pick_features(&self, x: i32, y: i32, level: u32, longitude: f64, latitude: f64) -> Option<ImageryFeatureFuture> {
        if !self.enable_pick_features.get() {
            return None;
        }
        let rectangle = self.tiling_scheme.tile_xy_to_native_rectangle(f64::from(x), f64::from(y), level);
        let geographic = self.tiling_scheme.projection().is_geographic();
        let (horizontal, vertical, sr) = if geographic {
            (crate::core::math::to_degrees(longitude), crate::core::math::to_degrees(latitude), "4326")
        } else {
            let projected = self.tiling_scheme.projection().project(&Cartographic::new(longitude, latitude, 0.0));
            (projected.x, projected.y, "3857")
        };
        let layers = match &self.layers {
            Some(layers) => format!("visible:{layers}"),
            None => "visible".into(),
        };
        let query = vec![
            ("f".to_owned(), Some(QueryValue::Single("json".into()))),
            ("tolerance".to_owned(), Some(QueryValue::Single("2".into()))),
            ("geometryType".to_owned(), Some(QueryValue::Single("esriGeometryPoint".into()))),
            ("geometry".to_owned(), Some(QueryValue::Single(format!("{horizontal},{vertical}")))),
            (
                "mapExtent".to_owned(),
                Some(QueryValue::Single(format!(
                    "{},{},{},{}",
                    rectangle.west, rectangle.south, rectangle.east, rectangle.north
                ))),
            ),
            (
                "imageDisplay".to_owned(),
                Some(QueryValue::Single(format!("{},{},96", self.tile_width, self.tile_height))),
            ),
            ("sr".to_owned(), Some(QueryValue::Single(sr.into()))),
            ("layers".to_owned(), Some(QueryValue::Single(layers))),
        ];
        let base = self.resource.get_url_component(false, false);
        let base = if base.ends_with('/') { base } else { format!("{base}/") };
        let resource = self.resource.get_derived_resource(crate::core::resource::DerivedResourceOptions {
            url: Some(format!("{base}identify")),
            query_parameters: query,
            ..Default::default()
        })?;
        let client = self.loader.client.clone();
        Some(Box::pin(async move {
            let fetch = client
                .fetch_json(&resource)
                .ok_or_else(|| RuntimeError::new("ArcGIS identify request was throttled."))?;
            let json = fetch.await.map_err(|e| RuntimeError::new(e.to_string()))?;
            Ok(Some(parse_features(&json)))
        }))
    }
}

/// Cesium's ArcGIS Identify result parsing, including point coordinates.
pub fn parse_features(json: &Value) -> Vec<ImageryLayerFeatureInfo> {
    let mut result = Vec::new();
    let Some(features) = json["results"].as_array() else {
        return result;
    };
    for feature in features {
        let mut info = ImageryLayerFeatureInfo::new();
        info.data = Some(feature.clone());
        info.name = feature.get("value").cloned();
        info.properties = feature.get("attributes").cloned();
        if let Some(properties) = info.properties.clone() {
            info.configure_description_from_properties(&properties);
        }
        if feature["geometryType"] == "esriGeometryPoint" && !feature["geometry"].is_null() {
            let geometry = &feature["geometry"];
            let wkid = geometry["spatialReference"]["wkid"].as_i64().map(|v| v as i32).unwrap_or(4326);
            let x = geometry["x"].as_f64();
            let y = geometry["y"].as_f64();
            let z = geometry["z"].as_f64().unwrap_or(0.0);
            if let (Some(x), Some(y)) = (x, y) {
                if wkid == 4326 || wkid == 4283 {
                    info.position = Some(Cartographic::from_degrees(x, y, z));
                } else if wkid == 102100 || wkid == 900913 || wkid == 3857 {
                    info.position =
                        Some(WebMercatorProjection::new(Ellipsoid::WGS84).unproject(Cartesian3::new(x, y, z)));
                }
            }
        }
        result.push(info);
    }
    result
}
