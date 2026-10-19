//! CesiumJS image helpers through an explicit canvas and image-decoder host.
//! Resampling and decoding belong to the host so browser color management and
//! filtering can be retained instead of approximated by a Rust pixel filter.

use super::{math, RuntimeError};
use std::{future::Future, pin::Pin};

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct RgbaImage {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
}

impl RgbaImage {
    pub fn new(width: u32, height: u32, pixels: Vec<u8>) -> Option<Self> {
        let length = (width as usize).checked_mul(height as usize)?.checked_mul(4)?;
        (pixels.len() == length).then_some(Self { width, height, pixels })
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct ImageDecodeOptions {
    pub flip_y: bool,
    pub skip_color_space_conversion: bool,
}

pub type ImageFuture<'a, I> = Pin<Box<dyn Future<Output = Result<I, RuntimeError>> + 'a>>;

pub trait ImageHost {
    type Image;
    fn dimensions(&self, image: &Self::Image) -> (u32, u32);
    fn image_from_rgba(&self, image: RgbaImage) -> Result<Self::Image, RuntimeError>;
    /// Draw with the canvas `copy` composition operation and read RGBA pixels.
    fn image_pixels(&self, image: &Self::Image, width: u32, height: u32) -> Result<Vec<u8>, RuntimeError>;
    /// Resample the entire source to fill the requested output dimensions.
    fn resize_image(&self, image: &Self::Image, width: u32, height: u32) -> Result<Self::Image, RuntimeError>;
    /// Decode without alpha premultiplication. A browser host must release any
    /// temporary object URL on success and failure. `None` indicates throttling.
    fn decode_image<'a>(
        &'a self,
        bytes: &'a [u8],
        mime_type: &'a str,
        options: ImageDecodeOptions,
    ) -> Option<ImageFuture<'a, Self::Image>>;
}

pub fn get_image_from_typed_array<H: ImageHost>(
    host: &H,
    bytes: &[u8],
    width: u32,
    height: u32,
) -> Result<H::Image, RuntimeError> {
    let image = RgbaImage::new(width, height, bytes.to_vec())
        .ok_or_else(|| RuntimeError::new("Invalid RGBA image dimensions or buffer length."))?;
    host.image_from_rgba(image)
}
pub fn get_image_pixels<H: ImageHost>(
    host: &H,
    image: &H::Image,
    width: Option<u32>,
    height: Option<u32>,
) -> Result<Vec<u8>, RuntimeError> {
    let (w, h) = host.dimensions(image);
    host.image_pixels(image, width.unwrap_or(w), height.unwrap_or(h))
}
pub fn resize_image_to_next_power_of_two<H: ImageHost>(host: &H, image: &H::Image) -> Result<H::Image, RuntimeError> {
    let (width, height) = host.dimensions(image);
    if width > 1 << 31 || height > 1 << 31 {
        return Err(RuntimeError::new("Image dimensions exceed 2^31."));
    }
    host.resize_image(image, math::next_power_of_two(width), math::next_power_of_two(height))
}
pub fn load_image_from_typed_array<'a, H: ImageHost>(
    host: &'a H,
    bytes: &'a [u8],
    mime_type: &'a str,
    options: ImageDecodeOptions,
) -> Option<ImageFuture<'a, H::Image>> {
    host.decode_image(bytes, mime_type, options)
}
