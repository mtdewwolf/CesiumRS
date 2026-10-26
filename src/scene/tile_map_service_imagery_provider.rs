//! CesiumJS `TileMapServiceImageryProvider`: tiles in the OSGeo Tile Map
//! Service layout, configured from `tilemapresource.xml` when available.
//!
//! As upstream, missing metadata is not fatal: the provider falls back to
//! option values and a Web Mercator `{z}/{x}/{reverseY}` template.

use super::{
    ImageryErrorEvent, ImageryFuture, ImageryImage, ImageryLoader, ImageryProvider, TileDiscardPolicy,
    UrlTemplateImageryProvider, UrlTemplateImageryProviderOptions,
};
use crate::core::{
    Cartographic, Credit, Ellipsoid, GeographicTilingScheme, ImageHost, Rectangle, Resource, ResourceClient,
    RuntimeError, TilingScheme, WebMercatorTilingScheme, XmlDocument,
};
use std::ops::Deref;
use std::rc::Rc;
use std::sync::Arc;

#[derive(Clone, Default)]
pub struct TileMapServiceImageryProviderOptions {
    pub file_extension: Option<String>,
    pub credit: Option<Credit>,
    pub minimum_level: Option<u32>,
    pub maximum_level: Option<u32>,
    pub rectangle: Option<Rectangle>,
    pub tiling_scheme: Option<Arc<dyn TilingScheme>>,
    pub ellipsoid: Option<Ellipsoid>,
    pub tile_width: Option<f64>,
    pub tile_height: Option<f64>,
    pub flip_xy: bool,
    pub tile_discard_policy: Option<Rc<dyn TileDiscardPolicy<ImageryImage>>>,
}

pub struct TileMapServiceImageryProvider<H: ImageHost> {
    inner: UrlTemplateImageryProvider<H>,
}

fn child<'a>(node: roxmltree::Node<'a, 'a>, name: &str) -> Option<roxmltree::Node<'a, 'a>> {
    node.children().find(|child| child.is_element() && child.tag_name().name().eq_ignore_ascii_case(name))
}

fn parse_i32(value: Option<&str>) -> Option<i32> {
    value?.trim().parse().ok()
}

fn parse_f64(value: Option<&str>) -> Option<f64> {
    value?.trim().parse().ok()
}

fn confine_rectangle_to_tiling_scheme(mut rectangle: Rectangle, tiling_scheme: &dyn TilingScheme) -> Rectangle {
    let scheme = tiling_scheme.rectangle();
    if rectangle.west < scheme.west {
        rectangle.west = scheme.west;
    }
    if rectangle.east > scheme.east {
        rectangle.east = scheme.east;
    }
    if rectangle.south < scheme.south {
        rectangle.south = scheme.south;
    }
    if rectangle.north > scheme.north {
        rectangle.north = scheme.north;
    }
    rectangle
}

fn calculate_safe_minimum_detail_level(
    tiling_scheme: &dyn TilingScheme,
    rectangle: &Rectangle,
    minimum_level: u32,
) -> u32 {
    let southwest = rectangle.southwest();
    let northeast = rectangle.northeast();
    if let (Some(sw), Some(ne)) = (
        tiling_scheme.position_to_tile_xy(&southwest, minimum_level),
        tiling_scheme.position_to_tile_xy(&northeast, minimum_level),
    ) {
        let tiles = ((ne.x - sw.x).abs() + 1.0) * ((ne.y - sw.y).abs() + 1.0);
        if tiles > 4.0 {
            return 0;
        }
    }
    minimum_level
}

fn absolute_base(resource: &Resource) -> String {
    let base = resource.get_url_component(false, false);
    if base.ends_with('/') {
        base
    } else {
        format!("{base}/")
    }
}

fn clone_with_url(resource: &Resource, url: String) -> Resource {
    let result = resource.clone();
    result.options_mut().url = url;
    result
}

fn template_resource(tms_resource: &Resource, file_extension: &str) -> Resource {
    let url = format!("{}{{z}}/{{x}}/{{reverseY}}.{file_extension}", absolute_base(tms_resource));
    clone_with_url(tms_resource, url)
}

impl<H: ImageHost + 'static> TileMapServiceImageryProvider<H> {
    pub fn new(loader: ImageryLoader<H>, options: UrlTemplateImageryProviderOptions) -> Result<Self, RuntimeError> {
        Ok(Self { inner: UrlTemplateImageryProvider::new(loader, options)? })
    }

    /// Cesium's `fromUrl`. A failed `tilemapresource.xml` request falls back to
    /// options, exactly as the upstream `_metadataFailure` path.
    pub async fn from_url(
        loader: ImageryLoader<H>,
        resource: Resource,
        options: TileMapServiceImageryProviderOptions,
    ) -> Result<Self, RuntimeError> {
        resource.append_forward_slash();
        let tms_resource = resource.clone();
        let xml_resource = clone_with_url(&resource, format!("{}tilemapresource.xml", absolute_base(&resource)));
        let metadata = match load_metadata(&loader.client, &xml_resource, &tms_resource, &options).await {
            Ok(options) => options,
            Err(_) => metadata_failure(&tms_resource, &options),
        };
        Self::new(loader, metadata)
    }

    pub fn url(&self) -> String {
        self.inner.url()
    }
}

async fn load_metadata(
    client: &Rc<ResourceClient>,
    xml_resource: &Resource,
    tms_resource: &Resource,
    options: &TileMapServiceImageryProviderOptions,
) -> Result<UrlTemplateImageryProviderOptions, RuntimeError> {
    let fetch =
        client.fetch_xml(xml_resource).ok_or_else(|| RuntimeError::new("TMS metadata request was throttled."))?;
    let xml = fetch.await.map_err(|e| RuntimeError::new(e.to_string()))?;
    metadata_success(&xml, options, tms_resource)
}

fn metadata_success(
    xml: &XmlDocument,
    options: &TileMapServiceImageryProviderOptions,
    tms_resource: &Resource,
) -> Result<UrlTemplateImageryProviderOptions, RuntimeError> {
    let document = xml.document();
    let root = document.root_element();
    let format = child(root, "tileformat");
    let tilesets = child(root, "tilesets");
    let bbox = child(root, "boundingbox");
    let (Some(tilesets), Some(bbox)) = (tilesets, bbox) else {
        return Err(RuntimeError::new(format!(
            "Unable to find expected tilesets or bbox attributes in {}.",
            xml_url(tms_resource)
        )));
    };
    let tilesets_list: Vec<_> = tilesets
        .children()
        .filter(|node| node.is_element() && node.tag_name().name().eq_ignore_ascii_case("tileset"))
        .collect();
    let format = format.ok_or_else(|| {
        RuntimeError::new(format!("Unable to find expected tilesets or bbox attributes in {}.", xml_url(tms_resource)))
    })?;

    let file_extension = options
        .file_extension
        .clone()
        .or_else(|| format.attribute("extension").map(str::to_owned))
        .ok_or_else(|| RuntimeError::new("TMS tileformat has no extension."))?;
    let tile_width =
        options.tile_width.or_else(|| parse_i32(format.attribute("width")).map(f64::from)).unwrap_or(256.0);
    let tile_height =
        options.tile_height.or_else(|| parse_i32(format.attribute("height")).map(f64::from)).unwrap_or(256.0);
    let mut minimum_level = options
        .minimum_level
        .or_else(|| tilesets_list.first().and_then(|n| parse_i32(n.attribute("order"))).map(|v| v.max(0) as u32))
        .unwrap_or(0);
    let maximum_level = options
        .maximum_level
        .or_else(|| tilesets_list.last().and_then(|n| parse_i32(n.attribute("order"))).map(|v| v.max(0) as u32));

    let tiling_scheme_name = tilesets.attribute("profile").unwrap_or("");
    let tiling_scheme: Arc<dyn TilingScheme> = match &options.tiling_scheme {
        Some(scheme) => scheme.clone(),
        None => match tiling_scheme_name {
            "geodetic" | "global-geodetic" => Arc::new(GeographicTilingScheme::new(
                options.ellipsoid.unwrap_or(Ellipsoid::WGS84),
                Rectangle::MAX_VALUE,
                2,
                1,
            )),
            "mercator" | "global-mercator" => {
                Arc::new(WebMercatorTilingScheme::new(options.ellipsoid.unwrap_or(Ellipsoid::WGS84), 1, 1, None))
            }
            other => {
                return Err(RuntimeError::new(format!(
                    "{} specifies an unsupported profile attribute, {other}.",
                    xml_url(tms_resource)
                )))
            }
        },
    };

    let mut rectangle = match options.rectangle {
        Some(rectangle) => rectangle,
        None => {
            let (sw_x, sw_y, ne_x, ne_y) = if options.flip_xy {
                (
                    parse_f64(bbox.attribute("miny")),
                    parse_f64(bbox.attribute("minx")),
                    parse_f64(bbox.attribute("maxy")),
                    parse_f64(bbox.attribute("maxx")),
                )
            } else {
                (
                    parse_f64(bbox.attribute("minx")),
                    parse_f64(bbox.attribute("miny")),
                    parse_f64(bbox.attribute("maxx")),
                    parse_f64(bbox.attribute("maxy")),
                )
            };
            let (Some(sw_x), Some(sw_y), Some(ne_x), Some(ne_y)) = (sw_x, sw_y, ne_x, ne_y) else {
                return Err(RuntimeError::new(format!(
                    "Unable to find expected tilesets or bbox attributes in {}.",
                    xml_url(tms_resource)
                )));
            };
            let is_gdal2tiles = tiling_scheme_name == "geodetic" || tiling_scheme_name == "mercator";
            let (sw, ne) = if tiling_scheme.projection().is_geographic() || is_gdal2tiles {
                (Cartographic::from_degrees(sw_x, sw_y, 0.0), Cartographic::from_degrees(ne_x, ne_y, 0.0))
            } else {
                let projection = tiling_scheme.projection();
                (
                    projection.unproject(crate::core::Cartesian3::new(sw_x, sw_y, 0.0)),
                    projection.unproject(crate::core::Cartesian3::new(ne_x, ne_y, 0.0)),
                )
            };
            Rectangle::new(sw.longitude, sw.latitude, ne.longitude, ne.latitude)
        }
    };
    rectangle = confine_rectangle_to_tiling_scheme(rectangle, tiling_scheme.as_ref());
    minimum_level = calculate_safe_minimum_detail_level(tiling_scheme.as_ref(), &rectangle, minimum_level);

    let template = template_resource(tms_resource, &file_extension);
    Ok(make_options(template, tiling_scheme, rectangle, tile_width, tile_height, minimum_level, maximum_level, options))
}

fn xml_url(resource: &Resource) -> String {
    format!("{}tilemapresource.xml", absolute_base(resource))
}

fn metadata_failure(
    tms_resource: &Resource,
    options: &TileMapServiceImageryProviderOptions,
) -> UrlTemplateImageryProviderOptions {
    let file_extension = options.file_extension.clone().unwrap_or_else(|| "png".into());
    let tile_width = options.tile_width.unwrap_or(256.0);
    let tile_height = options.tile_height.unwrap_or(256.0);
    let tiling_scheme = options.tiling_scheme.clone().unwrap_or_else(|| {
        Arc::new(WebMercatorTilingScheme::new(options.ellipsoid.unwrap_or(Ellipsoid::WGS84), 1, 1, None))
    });
    let rectangle = confine_rectangle_to_tiling_scheme(
        options.rectangle.unwrap_or(*tiling_scheme.rectangle()),
        tiling_scheme.as_ref(),
    );
    let minimum_level =
        calculate_safe_minimum_detail_level(tiling_scheme.as_ref(), &rectangle, options.minimum_level.unwrap_or(0));
    let template = template_resource(tms_resource, &file_extension);
    make_options(
        template,
        tiling_scheme,
        rectangle,
        tile_width,
        tile_height,
        minimum_level,
        options.maximum_level,
        options,
    )
}

#[allow(clippy::too_many_arguments)]
fn make_options(
    url: Resource,
    tiling_scheme: Arc<dyn TilingScheme>,
    rectangle: Rectangle,
    tile_width: f64,
    tile_height: f64,
    minimum_level: u32,
    maximum_level: Option<u32>,
    options: &TileMapServiceImageryProviderOptions,
) -> UrlTemplateImageryProviderOptions {
    let mut result = UrlTemplateImageryProviderOptions::new(url);
    result.tiling_scheme = Some(tiling_scheme);
    result.rectangle = Some(rectangle);
    result.tile_width = Some(tile_width);
    result.tile_height = Some(tile_height);
    result.minimum_level = Some(minimum_level);
    result.maximum_level = maximum_level;
    result.tile_discard_policy = options.tile_discard_policy.clone();
    result.credit = options.credit.clone();
    result
}

impl<H: ImageHost> Deref for TileMapServiceImageryProvider<H> {
    type Target = UrlTemplateImageryProvider<H>;
    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}

impl<H: ImageHost + 'static> ImageryProvider for TileMapServiceImageryProvider<H> {
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
        self.inner.error_event()
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
        self.inner.get_tile_credits(x, y, level)
    }
    fn request_image(&self, x: i32, y: i32, level: u32) -> Option<ImageryFuture> {
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

// Re-exported so the parent module can name the builder without pulling in
// upstream's JavaScript prototype machinery.
