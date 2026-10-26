//! CesiumJS `SingleTileImageryProvider`: provides a single top-level imagery tile.

use super::{
    image_error, report_image_error, ImageryErrorEvent, ImageryFuture, ImageryImage, ImageryLoader, ImageryProvider,
};
use crate::core::{
    Credit, Ellipsoid, GeographicTilingScheme, ImageHost, Rectangle, Resource, RuntimeError, TilingScheme,
};
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;

#[derive(Clone)]
pub struct SingleTileImageryProviderOptions {
    pub url: Resource,
    pub tile_width: f64,
    pub tile_height: f64,
    pub rectangle: Option<Rectangle>,
    pub credit: Option<Credit>,
    pub ellipsoid: Option<Ellipsoid>,
}

pub struct SingleTileImageryProvider<H: ImageHost> {
    resource: Resource,
    loader: ImageryLoader<H>,
    tiling_scheme: Arc<dyn TilingScheme>,
    rectangle: Rectangle,
    tile_width: f64,
    tile_height: f64,
    credit: Option<Credit>,
    error_event: ImageryErrorEvent,
    image: RefCell<Option<ImageryImage>>,
    has_error: RefCell<bool>,
}

impl<H: ImageHost + 'static> SingleTileImageryProvider<H> {
    pub fn new(loader: ImageryLoader<H>, options: SingleTileImageryProviderOptions) -> Self {
        let rectangle = options.rectangle.unwrap_or(Rectangle::MAX_VALUE);
        let ellipsoid = options.ellipsoid.unwrap_or(Ellipsoid::WGS84);
        let tiling_scheme = Arc::new(GeographicTilingScheme::new(ellipsoid, rectangle, 1, 1));

        Self {
            resource: options.url,
            loader,
            tiling_scheme,
            rectangle,
            tile_width: options.tile_width,
            tile_height: options.tile_height,
            credit: options.credit,
            error_event: Rc::new(RefCell::new(Default::default())),
            image: RefCell::new(None),
            has_error: RefCell::new(false),
        }
    }

    /// Asynchronously fetches the image to determine tile dimensions and returns a ready provider.
    pub async fn from_url(
        loader: ImageryLoader<H>,
        url: Resource,
        rectangle: Option<Rectangle>,
        credit: Option<Credit>,
        ellipsoid: Option<Ellipsoid>,
    ) -> Result<Self, RuntimeError> {
        let fetch =
            loader.load_image(url.clone(), false).ok_or_else(|| RuntimeError::new("Image request throttled."))?;
        let image = fetch.await?;
        let (tile_width, tile_height) = match &image {
            ImageryImage::Image(res) => {
                let img = res.get::<H::Image>().ok_or_else(|| RuntimeError::new("Invalid image resource."))?;
                let (w, h) = loader.host.dimensions(img);
                (f64::from(w), f64::from(h))
            }
            ImageryImage::Compressed(buf) => (f64::from(buf.width), f64::from(buf.height)),
            ImageryImage::CompressedMipmaps(mips) => {
                let first = mips.first().ok_or_else(|| RuntimeError::new("Empty mipmaps."))?;
                (f64::from(first.width), f64::from(first.height))
            }
        };

        let provider = Self::new(
            loader,
            SingleTileImageryProviderOptions { url, tile_width, tile_height, rectangle, credit, ellipsoid },
        );
        *provider.image.borrow_mut() = Some(image);
        Ok(provider)
    }

    pub fn url(&self) -> String {
        self.resource.url()
    }
}

impl<H: ImageHost + 'static> ImageryProvider for SingleTileImageryProvider<H> {
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
        Some(0)
    }

    fn minimum_level(&self) -> u32 {
        0
    }

    fn tiling_scheme(&self) -> &dyn TilingScheme {
        self.tiling_scheme.as_ref()
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

    fn request_image(&self, x: i32, y: i32, level: u32) -> Option<ImageryFuture> {
        if level != 0 || x != 0 || y != 0 {
            return None;
        }

        if let Some(image) = self.image.borrow().clone() {
            return Some(Box::pin(std::future::ready(Ok(image))));
        }

        if *self.has_error.borrow() {
            return None;
        }

        let fetch = self.loader.load_image(self.resource.clone(), false)?;
        let error_event = self.error_event.clone();
        let resource_url = self.resource.url();

        Some(Box::pin(async move {
            match fetch.await {
                Ok(image) => Ok(image),
                Err(err) => {
                    let mut error = image_error(
                        None,
                        format!("Failed to load image {resource_url}: {err}"),
                        Some(0),
                        Some(0),
                        Some(0),
                    );
                    report_image_error(&error_event, &mut error);
                    Err(RuntimeError::new(format!("Failed to load image {resource_url}: {err}")))
                }
            }
        }))
    }
}
