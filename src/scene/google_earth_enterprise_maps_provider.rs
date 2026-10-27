//! CesiumJS `GoogleEarthEnterpriseMapsProvider`: the Google Earth
//! Enterprise 2D Maps `ImageryMaps` endpoint, including its historically
//! malformed JSON metadata.

use super::{ImageryErrorEvent, ImageryFuture, ImageryImage, ImageryLoader, ImageryProvider, TileDiscardPolicy};
use crate::core::query::QueryValue;
use crate::core::{
    Credit, Ellipsoid, GeographicTilingScheme, ImageHost, Rectangle, Resource, RuntimeError, TilingScheme,
    WebMercatorTilingScheme,
};
use regex::Regex;
use serde_json::Value;
use std::{cell::RefCell, rc::Rc, sync::Arc};

pub const DEFAULT_PATH: &str = "/default_map";
pub const DEFAULT_LOGO_URL: &str = "Assets/Images/google_earth_credit.png";
pub const REQUEST_TYPE: &str = "ImageryMaps";

#[derive(Clone, Default)]
pub struct GoogleEarthEnterpriseMapsProviderOptions {
    pub path: Option<String>,
    pub maximum_level: Option<u32>,
    pub tile_discard_policy: Option<Rc<dyn TileDiscardPolicy<ImageryImage>>>,
    pub ellipsoid: Option<Ellipsoid>,
    pub credit: Option<Credit>,
}

pub struct GoogleEarthEnterpriseMapsProvider<H: ImageHost> {
    resource: Resource,
    loader: ImageryLoader<H>,
    url: String,
    path: String,
    channel: u32,
    version: u32,
    tiling_scheme: Arc<dyn TilingScheme>,
    maximum_level: Option<u32>,
    tile_discard_policy: Option<Rc<dyn TileDiscardPolicy<ImageryImage>>>,
    credit: Credit,
    error_event: ImageryErrorEvent,
}

impl<H: ImageHost + 'static> GoogleEarthEnterpriseMapsProvider<H> {
    pub async fn from_url(
        loader: ImageryLoader<H>,
        url: Resource,
        channel: u32,
        options: GoogleEarthEnterpriseMapsProviderOptions,
    ) -> Result<Self, RuntimeError> {
        let path = options.path.clone().unwrap_or_else(|| DEFAULT_PATH.into());
        let path_without_slash = path.strip_prefix('/').unwrap_or(&path);
        let resource = url
            .get_derived_resource(crate::core::resource::DerivedResourceOptions {
                url: Some(path_without_slash.into()),
                ..Default::default()
            })
            .ok_or_else(|| RuntimeError::new("Invalid Google Earth Maps resource."))?;
        resource.append_forward_slash();
        let metadata_resource = resource
            .get_derived_resource(crate::core::resource::DerivedResourceOptions {
                url: Some("query".into()),
                query_parameters: vec![
                    ("request".into(), Some(QueryValue::Single("Json".into()))),
                    ("vars".into(), Some(QueryValue::Single("geeServerDefs".into()))),
                    ("is2d".into(), Some(QueryValue::Single("t".into()))),
                ],
                ..Default::default()
            })
            .ok_or_else(|| RuntimeError::new("Invalid Google Earth Maps metadata resource."))?;
        let text = loader
            .client
            .fetch_text(&metadata_resource)
            .ok_or_else(|| RuntimeError::new("Google Earth Maps metadata request was throttled."))?
            .await
            .map_err(|e| {
                RuntimeError::new(format!("An error occurred while accessing {}. {e}", metadata_resource.url()))
            })?;
        let (version, tiling_scheme) = parse_metadata(&text, channel, options.ellipsoid)?;
        Ok(Self {
            resource,
            loader,
            url: url.url(),
            path,
            channel,
            version,
            tiling_scheme,
            maximum_level: options.maximum_level,
            tile_discard_policy: options.tile_discard_policy,
            credit: options.credit.unwrap_or_else(gee_maps_credit),
            error_event: Rc::new(RefCell::new(Default::default())),
        })
    }

    pub fn url(&self) -> &str {
        &self.url
    }
    pub fn path(&self) -> &str {
        &self.path
    }
    pub fn channel(&self) -> u32 {
        self.channel
    }
    pub fn version(&self) -> u32 {
        self.version
    }
    pub fn request_type(&self) -> &'static str {
        REQUEST_TYPE
    }
    pub fn tile_url(&self, x: i32, y: i32, level: u32) -> String {
        self.image_resource(x, y, level).get_url_component(true, false)
    }

    fn image_resource(&self, x: i32, y: i32, level: u32) -> Resource {
        self.resource
            .get_derived_resource(crate::core::resource::DerivedResourceOptions {
                url: Some("query".into()),
                query_parameters: vec![
                    ("request".into(), Some(QueryValue::Single(REQUEST_TYPE.into()))),
                    ("channel".into(), Some(QueryValue::Single(self.channel.to_string()))),
                    ("version".into(), Some(QueryValue::Single(self.version.to_string()))),
                    ("x".into(), Some(QueryValue::Single(x.to_string()))),
                    ("y".into(), Some(QueryValue::Single(y.to_string()))),
                    // Google Earth starts with zoom level 1, not 0.
                    ("z".into(), Some(QueryValue::Single((level + 1).to_string()))),
                ],
                ..Default::default()
            })
            .unwrap_or_else(|| self.resource.clone())
    }
}

fn parse_metadata(
    text: &str,
    channel: u32,
    ellipsoid: Option<Ellipsoid>,
) -> Result<(u32, Arc<dyn TilingScheme>), RuntimeError> {
    let data = parse_malformed_json(text)?;
    let layers = data["layers"].as_array().ok_or_else(|| RuntimeError::new("Google Earth JSON has no layers."))?;
    let layer = layers
        .iter()
        .find(|layer| layer["id"].as_u64().is_some_and(|id| id == u64::from(channel)))
        .ok_or_else(|| RuntimeError::new(format!("Could not find layer with channel (id) of {channel}.")))?;
    let version = layer["version"]
        .as_u64()
        .ok_or_else(|| RuntimeError::new(format!("Could not find a version in channel (id) {channel}.")))?
        as u32;
    let ellipsoid = ellipsoid.unwrap_or(Ellipsoid::WGS84);
    let projection = data["projection"].as_str();
    let scheme: Arc<dyn TilingScheme> = match projection {
        Some("flat") => Arc::new(GeographicTilingScheme::new(
            ellipsoid,
            Rectangle::new(-std::f64::consts::PI, -std::f64::consts::PI, std::f64::consts::PI, std::f64::consts::PI),
            2,
            2,
        )),
        None | Some("mercator") => Arc::new(WebMercatorTilingScheme::new(ellipsoid, 2, 2, None)),
        Some(other) => return Err(RuntimeError::new(format!("Unsupported projection {other}."))),
    };
    Ok((version, scheme))
}

/// First parse normally; on failure quote the bare object keys the GEE server
/// emits. This is the exact repair Cesium applies.
pub fn parse_malformed_json(text: &str) -> Result<Value, RuntimeError> {
    match serde_json::from_str(text) {
        Ok(value) => Ok(value),
        Err(_) => {
            let regex = Regex::new(r"([\[\{,])[\n\r ]*([A-Za-z0-9]+)[\n\r ]*:").expect("valid regex");
            let repaired = regex.replace_all(text, "$1\"$2\":");
            serde_json::from_str(&repaired).map_err(|e| RuntimeError::new(e.to_string()))
        }
    }
}

fn gee_maps_credit() -> Credit {
    Credit::new(
        format!(
            "<a href=\"http://www.google.com/enterprise/mapsearth/products/earthenterprise.html\"><img src=\"{DEFAULT_LOGO_URL}\" title=\"Google Imagery\"/></a>"
        ),
        false,
    )
}

impl<H: ImageHost + 'static> ImageryProvider for GoogleEarthEnterpriseMapsProvider<H> {
    fn rectangle(&self) -> &Rectangle {
        self.tiling_scheme.rectangle()
    }
    fn tile_width(&self) -> f64 {
        256.0
    }
    fn tile_height(&self) -> f64 {
        256.0
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
        true
    }
    fn request_image(&self, x: i32, y: i32, level: u32) -> Option<ImageryFuture> {
        let resource = self.image_resource(x, y, level);
        let has_policy = self.tile_discard_policy.is_some();
        let fetch = self.loader.load_image(resource, has_policy)?;
        Some(fetch)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repairs_bare_keys() {
        let value = parse_malformed_json("{layers: [{id: 1002, version: 7, requestType: \"ImageryMaps\"}]}").unwrap();
        assert_eq!(value["layers"][0]["id"], 1002);
    }
}
