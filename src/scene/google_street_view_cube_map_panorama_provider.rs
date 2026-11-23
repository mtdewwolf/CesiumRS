//! CesiumJS `GoogleStreetViewCubeMapPanoramaProvider`: Google Street View
//! static-API cube faces for a panorama primitive.
//!
//! Network access is host-owned: `load_panorama` returns a future resolved by
//! a [`ResourceClient`]. The provider itself is transport free, so URL and
//! transform construction are deterministic and testable.

use super::{
    cube_map_panorama::{CubeMapPanorama, CubeMapPanoramaOptions, CubeMapSources},
    panorama_provider::PanoramaFuture,
};
use crate::core::{
    Cartesian3, Cartographic, Credit, Ellipsoid, GoogleMaps, LocalAxis, LocalFrame, Matrix3, Matrix4, Resource,
    ResourceClient, RuntimeError,
};
use serde_json::Value;
use std::future::Future;
use std::pin::Pin;

const DEFAULT_TILE_SIZE: u32 = 600;

/// Options for [`GoogleStreetViewCubeMapPanoramaProvider::new`].
#[derive(Clone, Debug, Default)]
pub struct GoogleStreetViewCubeMapPanoramaProviderOptions {
    pub key: Option<String>,
    pub url: Option<String>,
    pub tile_size: Option<u32>,
    pub credit: Option<Credit>,
}

/// Metadata returned by `getNearestPanoId`.
#[derive(Clone, Debug, PartialEq)]
pub struct NearestPanoId {
    pub pano_id: String,
    pub latitude: f64,
    pub longitude: f64,
}

/// Options for `loadPanorama`.
#[derive(Clone, Debug)]
pub struct GoogleStreetViewLoadPanoramaOptions {
    pub cartographic: Cartographic,
    pub pano_id: Option<String>,
    pub tile_size: Option<u32>,
    pub signature: Option<String>,
    pub credit: Option<Credit>,
}

/// A provider that supplies Google Street View cube-face imagery.
pub struct GoogleStreetViewCubeMapPanoramaProvider {
    key: Option<String>,
    base_resource: Resource,
    metadata_resource: Resource,
    tile_size: u32,
    credit: Option<Credit>,
}

impl GoogleStreetViewCubeMapPanoramaProvider {
    /// Constructs the provider directly. Prefer `from_url`.
    pub fn new(options: GoogleStreetViewCubeMapPanoramaProviderOptions) -> Result<Self, RuntimeError> {
        let google_maps = GoogleMaps::default();
        let key = options.key.or(google_maps.default_street_view_static_api_key).or(google_maps.default_api_key);
        let base_resource = match options.url {
            Some(url) => {
                Resource::new(url).ok_or_else(|| RuntimeError::new("options.url must be a valid URL or Resource."))?
            }
            None => google_maps.street_view_static_api_endpoint,
        };
        let metadata_resource = base_resource
            .get_derived_resource(crate::core::resource::DerivedResourceOptions {
                url: Some("streetview/metadata".into()),
                ..Default::default()
            })
            .ok_or_else(|| RuntimeError::new("Unable to derive the Google Street View metadata resource."))?;
        Ok(Self {
            key,
            base_resource,
            metadata_resource,
            tile_size: options.tile_size.unwrap_or(DEFAULT_TILE_SIZE),
            credit: options.credit,
        })
    }

    /// `GoogleStreetViewCubeMapPanoramaProvider.fromUrl`. Requires an API key.
    pub fn from_url(options: GoogleStreetViewCubeMapPanoramaProviderOptions) -> PanoramaFuture<'static, Self> {
        Box::pin(async move {
            let google_maps = GoogleMaps::default();
            let key =
                options.key.clone().or(google_maps.default_street_view_static_api_key).or(google_maps.default_api_key);
            if key.is_none() {
                return Err(RuntimeError::new(
                    "options.key, GoogleMaps.defaultStreetViewStaticApiKey or GoogleMaps.defaultApiKey is required.",
                ));
            }
            let mut provider = Self::new(options)?;
            provider.key = key;
            Ok(provider)
        })
    }

    pub fn key(&self) -> Option<&str> {
        self.key.as_deref()
    }

    pub fn tile_size(&self) -> u32 {
        self.tile_size
    }

    pub fn credit(&self) -> Option<&Credit> {
        self.credit.as_ref()
    }

    /// The metadata resource `streetview/metadata` derived from the base URL.
    pub fn metadata_resource(&self) -> &Resource {
        &self.metadata_resource
    }

    /// `_buildFaceUrl`.
    pub fn build_face_url(
        &self,
        heading: i32,
        pitch: i32,
        tile_size: u32,
        pano_id: &str,
        signature: Option<&str>,
    ) -> String {
        let mut query = vec![
            ("size".to_string(), Some(crate::core::query::QueryValue::Single(format!("{tile_size}x{tile_size}")))),
            ("pano".to_string(), Some(crate::core::query::QueryValue::Single(pano_id.to_string()))),
            ("heading".to_string(), Some(crate::core::query::QueryValue::Single(heading.to_string()))),
            ("pitch".to_string(), Some(crate::core::query::QueryValue::Single(pitch.to_string()))),
        ];
        if let Some(key) = &self.key {
            query.push(("key".to_string(), Some(crate::core::query::QueryValue::Single(key.clone()))));
        }
        if let Some(signature) = signature {
            query.push(("signature".to_string(), Some(crate::core::query::QueryValue::Single(signature.to_string()))));
        }
        self.base_resource
            .get_derived_resource(crate::core::resource::DerivedResourceOptions {
                query_parameters: query,
                ..Default::default()
            })
            .map_or_else(String::new, |resource| resource.url())
    }

    /// The six face URLs in positiveX, negativeX, positiveY, negativeY,
    /// positiveZ, negativeZ order, matching Cesium's heading/pitch layout.
    pub fn face_urls(&self, pano_id: &str, tile_size: u32, signature: Option<&str>) -> [String; 6] {
        [
            self.build_face_url(0, 0, tile_size, pano_id, signature),
            self.build_face_url(180, 0, tile_size, pano_id, signature),
            self.build_face_url(-90, -90, tile_size, pano_id, signature),
            self.build_face_url(-90, 90, tile_size, pano_id, signature),
            self.build_face_url(270, 0, tile_size, pano_id, signature),
            self.build_face_url(90, 0, tile_size, pano_id, signature),
        ]
    }

    fn metadata_error(status: &str) -> RuntimeError {
        RuntimeError::new(format!("GoogleStreetViewCubeMapPanoramaProvider metadata error: {status}"))
    }

    /// `getNearestPanoId`; returns a future requiring the host's resource client.
    pub fn get_nearest_pano_id<'a>(
        &'a self,
        client: &'a ResourceClient,
        position: Cartographic,
        radius: Option<f64>,
    ) -> Pin<Box<dyn Future<Output = Result<NearestPanoId, RuntimeError>> + 'a>> {
        Box::pin(async move {
            let resource = self
                .metadata_resource
                .get_derived_resource(crate::core::resource::DerivedResourceOptions {
                    query_parameters: vec![
                        ("key".into(), self.key.clone().map(crate::core::query::QueryValue::Single)),
                        (
                            "location".into(),
                            Some(crate::core::query::QueryValue::Single(format!(
                                "{},{}",
                                crate::core::math::to_degrees(position.latitude),
                                crate::core::math::to_degrees(position.longitude)
                            ))),
                        ),
                        (
                            "radius".into(),
                            Some(crate::core::query::QueryValue::Single(radius.unwrap_or(50.0).to_string())),
                        ),
                    ],
                    ..Default::default()
                })
                .ok_or_else(|| RuntimeError::new("Unable to derive the Google Street View metadata URL."))?;
            let value = client
                .fetch_json(&resource)
                .ok_or_else(|| RuntimeError::new("Google Street View metadata request was throttled."))?
                .await
                .map_err(|error| RuntimeError::new(error.to_string()))?;
            let status = value.get("status").and_then(Value::as_str).unwrap_or("");
            if status != "OK" {
                return Err(Self::metadata_error(status));
            }
            let location = value
                .get("location")
                .ok_or_else(|| RuntimeError::new("Google Street View metadata is missing location."))?;
            Ok(NearestPanoId {
                pano_id: value
                    .get("pano_id")
                    .and_then(Value::as_str)
                    .ok_or_else(|| RuntimeError::new("Google Street View metadata is missing pano_id."))?
                    .to_string(),
                latitude: location
                    .get("lat")
                    .and_then(Value::as_f64)
                    .ok_or_else(|| RuntimeError::new("Google Street View metadata is missing location.lat."))?,
                longitude: location
                    .get("lng")
                    .and_then(Value::as_f64)
                    .ok_or_else(|| RuntimeError::new("Google Street View metadata is missing location.lng."))?,
            })
        })
    }

    /// `getPanoIdMetadata`.
    pub fn get_pano_id_metadata<'a>(
        &'a self,
        client: &'a ResourceClient,
        pano_id: &'a str,
    ) -> Pin<Box<dyn Future<Output = Result<Value, RuntimeError>> + 'a>> {
        Box::pin(async move {
            let resource = self
                .metadata_resource
                .get_derived_resource(crate::core::resource::DerivedResourceOptions {
                    query_parameters: vec![
                        ("key".into(), self.key.clone().map(crate::core::query::QueryValue::Single)),
                        ("pano".into(), Some(crate::core::query::QueryValue::Single(pano_id.to_string()))),
                    ],
                    ..Default::default()
                })
                .ok_or_else(|| RuntimeError::new("Unable to derive the Google Street View metadata URL."))?;
            let value = client
                .fetch_json(&resource)
                .ok_or_else(|| RuntimeError::new("Google Street View metadata request was throttled."))?
                .await
                .map_err(|error| RuntimeError::new(error.to_string()))?;
            let status = value.get("status").and_then(Value::as_str).unwrap_or("");
            if status != "OK" {
                return Err(Self::metadata_error(status));
            }
            Ok(value)
        })
    }

    fn north_down_east_to_fixed_frame(cartographic: &Cartographic) -> Matrix4 {
        let origin = Cartesian3::from_degrees(
            crate::core::math::to_degrees(cartographic.longitude),
            crate::core::math::to_degrees(cartographic.latitude),
            0.0,
            &Ellipsoid::WGS84,
        );
        LocalFrame::new(LocalAxis::North, LocalAxis::Down)
            .expect("north and down are perpendicular")
            .to_fixed_frame(origin, &Ellipsoid::WGS84)
    }

    /// Resolves a panorama's face URLs and orientation. `pano_id` is looked up
    /// when absent, exactly as Cesium does.
    pub async fn load_panorama_data(
        &self,
        client: &ResourceClient,
        options: GoogleStreetViewLoadPanoramaOptions,
    ) -> Result<GoogleStreetViewPanoramaData, RuntimeError> {
        let pano_id = match options.pano_id {
            Some(pano_id) => pano_id,
            None => self.get_nearest_pano_id(client, options.cartographic, None).await?.pano_id,
        };
        let tile_size = options.tile_size.unwrap_or(self.tile_size);
        let faces = self.face_urls(&pano_id, tile_size, options.signature.as_deref());
        let transform = Matrix4::get_matrix3(&Self::north_down_east_to_fixed_frame(&options.cartographic));
        Ok(GoogleStreetViewPanoramaData {
            pano_id,
            faces,
            transform,
            credit: options.credit.or_else(|| self.credit.clone()).or_else(|| Some(GoogleMaps::get_default_credit())),
        })
    }

    /// Creates the panorama primitive from resolved face URLs.
    pub async fn load_panorama<G: crate::renderer::Gl + 'static>(
        &self,
        context: crate::renderer::Context<G>,
        client: &ResourceClient,
        options: GoogleStreetViewLoadPanoramaOptions,
    ) -> Result<CubeMapPanorama<G>, RuntimeError> {
        let data = self.load_panorama_data(client, options).await?;
        Ok(data.into_panorama(context))
    }
}

/// Face URLs, orientation and credit for one Street View panorama.
#[derive(Clone, Debug)]
pub struct GoogleStreetViewPanoramaData {
    pub pano_id: String,
    pub faces: [String; 6],
    pub transform: Matrix3,
    pub credit: Option<Credit>,
}

impl GoogleStreetViewPanoramaData {
    /// Builds the renderable panorama for a renderer context.
    pub fn into_panorama<G: crate::renderer::Gl + 'static>(
        self,
        context: crate::renderer::Context<G>,
    ) -> CubeMapPanorama<G> {
        CubeMapPanorama::new(
            context,
            CubeMapPanoramaOptions {
                sources: CubeMapSources::Urls(self.faces),
                transform: Some(self.transform),
                show: Some(true),
                credit: self.credit,
                return_command: Some(false),
            },
        )
    }

    /// `_parseMetadata`'s cartographic result.
    pub fn parse_metadata(metadata: &Value) -> Option<Cartographic> {
        let location = metadata.get("location")?;
        Some(Cartographic::from_degrees(location.get("lng")?.as_f64()?, location.get("lat")?.as_f64()?, 0.0))
    }
}

/// A provider is transport-based, not a [`super::panorama_provider::PanoramaProvider`],
/// because Rust panorama construction needs a renderer context. This alias
/// records the concrete loaded panorama type for callers.
pub type GoogleStreetViewPanorama = GoogleStreetViewPanoramaData;
