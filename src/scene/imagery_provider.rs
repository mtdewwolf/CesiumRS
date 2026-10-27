//! CesiumJS `ImageryProvider`: the interface every imagery provider
//! implements, and the shared image-loading path.
//!
//! A provider's `requestImage` returns a promise in Cesium. Here it returns a
//! boxed future, or `None` when the request scheduler is at capacity and the
//! caller should retry later. The decoded host image travels in a
//! [`FrameResource`], so providers and layers stay independent of the GL host.

use super::{FrameResource, ImageryLayerFeatureInfo, TileDiscardPolicy};
use crate::core::ktx2_transcoder::{Ktx2Texture, Ktx2Transcoder};
use crate::core::{
    image_helpers::FetchImageOptions, CompressedTextureBuffer, Credit, MutableErrorEvent, Proxy, Rectangle, Resource,
    ResourceClient, RuntimeError, TileProviderError, TilingScheme,
};
use std::cell::RefCell;
use std::future::Future;
use std::pin::Pin;
use std::rc::Rc;
use std::sync::Arc;

/// A decoded image or a compressed texture returned by a provider.
#[derive(Clone, Debug)]
pub enum ImageryImage {
    /// A host image, for example an `HTMLImageElement` or a decoded RGBA
    /// bitmap. The concrete type is the image host's.
    Image(FrameResource),
    /// A KTX2 image with a single mip level.
    Compressed(CompressedTextureBuffer),
    /// A KTX2 image with supplied mip levels.
    CompressedMipmaps(Vec<CompressedTextureBuffer>),
}

impl PartialEq for ImageryImage {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Image(a), Self::Image(b)) => a.ptr_eq(b),
            (Self::Compressed(a), Self::Compressed(b)) => a == b,
            (Self::CompressedMipmaps(a), Self::CompressedMipmaps(b)) => a == b,
            _ => false,
        }
    }
}

/// A pending tile image request.
pub type ImageryFuture = Pin<Box<dyn Future<Output = Result<ImageryImage, RuntimeError>>>>;

/// A pending feature pick. `None` means picking is not supported.
pub type ImageryFeatureFuture =
    Pin<Box<dyn Future<Output = Result<Option<Vec<ImageryLayerFeatureInfo>>, RuntimeError>>>>;

/// The provider's asynchronous error event payload.
pub type ImageryProviderError = TileProviderError<()>;

/// An imagery error event. Listeners may set `retry`, so this is the mutable
/// event kind; share the `Rc` with callers that register listeners.
pub type ImageryErrorEvent = Rc<RefCell<MutableErrorEvent<ImageryProviderError>>>;

/// CesiumJS `ImageryProvider`. Implementations supply request construction,
/// metadata and credits; the shared [`ImageryLoader`] performs image loads.
pub trait ImageryProvider {
    /// The covered area, in radians.
    fn rectangle(&self) -> &Rectangle;
    fn tile_width(&self) -> f64;
    fn tile_height(&self) -> f64;
    /// `None` when there is no level limit.
    fn maximum_level(&self) -> Option<u32>;
    fn minimum_level(&self) -> u32 {
        0
    }
    fn tiling_scheme(&self) -> &dyn TilingScheme;
    fn tile_discard_policy(&self) -> Option<Rc<dyn TileDiscardPolicy<ImageryImage>>> {
        None
    }
    fn error_event(&self) -> &ImageryErrorEvent;
    fn credit(&self) -> Option<&Credit> {
        None
    }
    fn proxy(&self) -> Option<Arc<dyn Proxy>> {
        None
    }
    fn has_alpha_channel(&self) -> bool {
        true
    }
    /// Credits for one tile. `None` is Cesium's `undefined`, meaning the
    /// provider's own credit applies.
    fn get_tile_credits(&self, _x: i32, _y: i32, _level: u32) -> Option<Vec<Credit>> {
        None
    }
    /// Requests one tile image. `None` means the request was throttled.
    fn request_image(&self, x: i32, y: i32, level: u32) -> Option<ImageryFuture>;
    /// Asynchronous feature picking; providers without support return `None`.
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

/// The shared `ImageryProvider.loadImage` path for one image host and client.
#[derive(Clone)]
pub struct ImageryLoader<H: crate::core::ImageHost> {
    pub client: Rc<ResourceClient>,
    pub host: Rc<H>,
}

impl<H: crate::core::ImageHost + 'static> ImageryLoader<H> {
    pub fn new(client: Rc<ResourceClient>, host: Rc<H>) -> Self {
        Self { client, host }
    }

    /// Cesium's `ImageryProvider.loadImage`. KTX2 resources are transcoded to
    /// compressed texture buffers; other URLs are decoded by the host with
    /// the flags Cesium uses (`preferBlob` when a discard policy exists).
    pub fn load_image(&self, resource: Resource, has_discard_policy: bool) -> Option<ImageryFuture> {
        if is_ktx2(&resource.url()) {
            let client = self.client.clone();
            return Some(Box::pin(async move {
                let fetch = client
                    .fetch_array_buffer(&resource)
                    .ok_or_else(|| RuntimeError::new("Imagery request was throttled."))?;
                let bytes = fetch.await.map_err(|error| RuntimeError::new(error.to_string()))?;
                let texture = Ktx2Transcoder::default().transcode(&bytes).await?;
                map_ktx2(texture)
            }));
        }

        let host = self.host.clone();
        let options =
            FetchImageOptions { prefer_blob: has_discard_policy, prefer_image_bitmap: true, flip_y: true, ..Default::default() };
        let fetch = self.client.fetch_image(&resource, host, options)?;
        Some(Box::pin(async move {
            let image = fetch.await.map_err(|error| RuntimeError::new(error.to_string()))?;
            let Some(image) = image else {
                return Err(RuntimeError::new("Failed to load imagery image."));
            };
            Ok(ImageryImage::Image(FrameResource::new(image)))
        }))
    }

    /// Transcodes one KTX2 buffer the way `loadKTX2` does. Cubemaps are not
    /// valid imagery tiles.
    pub fn load_ktx2_bytes(&self, bytes: Vec<u8>) -> ImageryFuture {
        Box::pin(async move {
            let texture = Ktx2Transcoder::default().transcode(&bytes).await?;
            map_ktx2(texture)
        })
    }
}

fn map_ktx2(texture: Ktx2Texture) -> Result<ImageryImage, RuntimeError> {
    match texture {
        Ktx2Texture::Image(image) => Ok(ImageryImage::Compressed(image)),
        Ktx2Texture::Mipmaps(mips) => Ok(ImageryImage::CompressedMipmaps(mips)),
        Ktx2Texture::Cubemap(_) => Err(RuntimeError::new("Cubemap KTX2 files cannot be used as imagery tiles.")),
    }
}

fn is_ktx2(url: &str) -> bool {
    url.len() >= 5 && url[url.len() - 5..].eq_ignore_ascii_case(".ktx2")
}

/// Creates the error Cesium's `TileProviderError.reportError` builds.
pub fn image_error(
    previous: Option<&ImageryProviderError>,
    message: impl Into<String>,
    x: Option<i32>,
    y: Option<i32>,
    level: Option<u32>,
) -> ImageryProviderError {
    TileProviderError {
        provider: (),
        message: message.into(),
        x: x.map(|v| v.max(0) as u32),
        y: y.map(|v| v.max(0) as u32),
        level,
        times_retried: previous.map_or(0, |e| e.times_retried + 1),
        retry: false,
        error: None,
    }
}

/// Raises `error` on `event`. Returns `true` when a listener requested a retry.
/// With no listeners, Cesium logs the error and does not retry.
pub fn report_image_error(event: &ImageryErrorEvent, error: &mut ImageryProviderError) -> bool {
    event.borrow_mut().raise_event(error);
    error.retry
}
