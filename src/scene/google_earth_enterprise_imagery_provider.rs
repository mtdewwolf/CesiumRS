//! CesiumJS `GoogleEarthEnterpriseImageryProvider`: encrypted GEE flat-file
//! imagery, quadtree metadata and per-tile credits.

use super::{ImageryErrorEvent, ImageryFuture, ImageryImage, ImageryLoader, ImageryProvider, TileDiscardPolicy};
use crate::core::{
    decode_google_earth_enterprise_data::decode_google_earth_enterprise_data, gee_packet, Credit, Ellipsoid,
    GeographicTilingScheme, GoogleEarthEnterpriseMetadata, ImageDecodeOptions, ImageHost, Rectangle, Resource,
    RuntimeError, TilingScheme,
};
use std::{cell::RefCell, rc::Rc, sync::Arc};

pub const MAXIMUM_LEVEL: u32 = 23;

#[derive(Clone, Default)]
pub struct GoogleEarthEnterpriseImageryProviderOptions {
    pub ellipsoid: Option<Ellipsoid>,
    pub tile_discard_policy: Option<Rc<dyn TileDiscardPolicy<ImageryImage>>>,
    pub credit: Option<Credit>,
}

/// Upstream's default policy discards a private unresolved `Image`; here the
/// sentinel is a frame carrying no host image, compared by identity.
struct GeeInvalidImagePolicy {
    invalid: ImageryImage,
}

impl TileDiscardPolicy<ImageryImage> for GeeInvalidImagePolicy {
    fn is_ready(&self) -> bool {
        true
    }
    fn should_discard_image(&self, image: &ImageryImage) -> bool {
        match (image, &self.invalid) {
            (ImageryImage::Image(a), ImageryImage::Image(b)) => a.ptr_eq(b),
            _ => false,
        }
    }
}

pub struct GoogleEarthEnterpriseImageryProvider<H: ImageHost> {
    metadata: Rc<GoogleEarthEnterpriseMetadata>,
    loader: ImageryLoader<H>,
    tiling_scheme: Arc<dyn TilingScheme>,
    tile_discard_policy: Option<Rc<dyn TileDiscardPolicy<ImageryImage>>>,
    invalid_image: ImageryImage,
    credit: Option<Credit>,
    error_event: ImageryErrorEvent,
}

impl<H: ImageHost + 'static> GoogleEarthEnterpriseImageryProvider<H> {
    /// Cesium's `fromMetadata`. The metadata must advertise imagery.
    pub fn from_metadata(
        metadata: Rc<GoogleEarthEnterpriseMetadata>,
        loader: ImageryLoader<H>,
        options: GoogleEarthEnterpriseImageryProviderOptions,
    ) -> Result<Self, RuntimeError> {
        if !metadata.imagery_present.get() {
            return Err(RuntimeError::new(format!("The server {} doesn't have imagery", metadata.resource.url())));
        }
        let ellipsoid = options.ellipsoid.unwrap_or(Ellipsoid::WGS84);
        let invalid_image = ImageryImage::Image(super::FrameResource::new(()));
        let tile_discard_policy = options.tile_discard_policy.or_else(|| {
            Some(Rc::new(GeeInvalidImagePolicy { invalid: invalid_image.clone() })
                as Rc<dyn TileDiscardPolicy<ImageryImage>>)
        });
        Ok(Self {
            metadata,
            loader,
            tiling_scheme: Arc::new(GeographicTilingScheme::new(
                ellipsoid,
                Rectangle::new(
                    -std::f64::consts::PI,
                    -std::f64::consts::PI,
                    std::f64::consts::PI,
                    std::f64::consts::PI,
                ),
                2,
                2,
            )),
            tile_discard_policy,
            invalid_image,
            credit: options.credit,
            error_event: Rc::new(RefCell::new(Default::default())),
        })
    }

    pub fn metadata(&self) -> &Rc<GoogleEarthEnterpriseMetadata> {
        &self.metadata
    }
    pub fn url(&self) -> String {
        self.metadata.resource.url().trim_end_matches('/').to_owned()
    }
    pub fn tile_url(&self, x: i32, y: i32, level: u32) -> Option<String> {
        let info = self.metadata.get_tile_information(x.max(0) as u32, y.max(0) as u32, level)??;
        if !info.has_imagery() {
            return None;
        }
        Some(self.image_resource(&info, x, y, level).url())
    }

    fn image_resource(
        &self,
        info: &crate::core::GoogleEarthEnterpriseTileInformation,
        x: i32,
        y: i32,
        level: u32,
    ) -> Resource {
        let quad_key = GoogleEarthEnterpriseMetadata::tile_xy_to_quad_key(x.max(0) as u32, y.max(0) as u32, level);
        let version = if info.imagery_version > 0 { info.imagery_version as u32 } else { 1 };
        self.metadata
            .resource
            .get_derived_resource(crate::core::resource::DerivedResourceOptions {
                url: Some(format!("flatfile?f1-0{quad_key}-i.{version}")),
                ..Default::default()
            })
            .unwrap_or_else(|| self.metadata.resource.clone())
    }
}

/// Decodes the GEE imagery protobuf: image type in field 1, bytes in field 2.
/// External alpha is unsupported upstream and here is discarded.
pub fn decode_earth_imagery_packet(data: &[u8]) -> Result<(String, Vec<u8>), RuntimeError> {
    let fields = gee_packet::protobuf(data)?;
    let image_type = gee_packet::field(&fields, 1).and_then(gee_packet::Wire::uint);
    let image_data = gee_packet::field(&fields, 2).and_then(gee_packet::Wire::bytes).map(<[u8]>::to_vec);
    let image_type = match image_type {
        Some(0) => "image/jpeg",
        Some(4) => "image/png",
        Some(_) => return Err(RuntimeError::new("GoogleEarthEnterpriseImageryProvider: Unsupported image type.")),
        None => return Err(RuntimeError::new("GEE imagery packet has no image type.")),
    };
    let image_data = image_data.ok_or_else(|| RuntimeError::new("GEE imagery packet has no image data."))?;
    Ok((image_type.into(), image_data))
}

fn get_image_type(image: &[u8]) -> Option<&'static str> {
    if image.get(6..10) == Some(b"JFIF") {
        Some("image/jpeg")
    } else if image.get(1..4) == Some(b"PNG") {
        Some("image/png")
    } else {
        None
    }
}

impl<H: ImageHost + 'static> ImageryProvider for GoogleEarthEnterpriseImageryProvider<H> {
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
        Some(MAXIMUM_LEVEL)
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
        self.metadata.resource.options().proxy.clone()
    }
    fn has_alpha_channel(&self) -> bool {
        false
    }
    fn get_tile_credits(&self, x: i32, y: i32, level: u32) -> Option<Vec<Credit>> {
        let info = self.metadata.get_tile_information(x.max(0) as u32, y.max(0) as u32, level)??;
        let credit = self.metadata.providers.borrow().get(&u32::from(info.imagery_provider)).cloned();
        credit.map(|credit| vec![credit])
    }
    fn request_image(&self, x: i32, y: i32, level: u32) -> Option<ImageryFuture> {
        let (x_u, y_u) = (x.max(0) as u32, y.max(0) as u32);
        let quad_key = GoogleEarthEnterpriseMetadata::tile_xy_to_quad_key(x_u, y_u, level);
        let info = match self.metadata.get_tile_information(x_u, y_u, level) {
            Some(Some(info)) => info,
            Some(None) => return Some(Box::pin(std::future::ready(Ok(self.invalid_image.clone())))),
            None => {
                if !self.metadata.is_valid(&quad_key) {
                    return Some(Box::pin(std::future::ready(Ok(self.invalid_image.clone()))));
                }
                // Metadata is still arriving; the layer retries later.
                let _ = self.metadata.populate_subtree(x_u, y_u, level, None);
                return None;
            }
        };
        if !info.has_imagery() {
            return Some(Box::pin(std::future::ready(Ok(self.invalid_image.clone()))));
        }
        let resource = self.image_resource(&info, x, y, level);
        let fetch = self.loader.client.fetch_array_buffer(&resource)?;
        let key = self.metadata.key.borrow().clone();
        let proto_imagery = self.metadata.proto_imagery.get();
        let loader = ImageryLoader::new(self.loader.client.clone(), self.loader.host.clone());
        Some(Box::pin(async move {
            let mut bytes = fetch.await.map_err(|e| RuntimeError::new(e.to_string()))?;
            decode_google_earth_enterprise_data(&key, &mut bytes)?;
            let mut image_type = get_image_type(&bytes).map(str::to_owned);
            let mut image_bytes = bytes;
            if image_type.is_none() && proto_imagery.unwrap_or(true) {
                let (kind, data) = decode_earth_imagery_packet(&image_bytes)?;
                image_type = Some(kind);
                image_bytes = data;
            }
            let image_type = image_type.ok_or_else(|| RuntimeError::new("Unsupported GEE image data."))?;
            let decode = loader
                .host
                .decode_image(&image_bytes, &image_type, ImageDecodeOptions { flip_y: true, ..Default::default() })
                .ok_or_else(|| RuntimeError::new("GEE image decode was throttled."))?;
            let image = decode.await?;
            Ok(ImageryImage::Image(super::FrameResource::new(image)))
        }))
    }
}
