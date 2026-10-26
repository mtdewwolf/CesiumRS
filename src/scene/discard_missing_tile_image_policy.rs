//! CesiumJS `DiscardMissingTileImagePolicy`: discards a tile whose pixels match
//! a known "missing image" produced by the server.
//!
//! Cesium fetches and decodes the missing image asynchronously in its
//! constructor. Here the host performs that load and either supplies the
//! decoded image or `None` when the fetch failed, which disables the check the
//! way Cesium's failure path does. Blob byte lengths are not part of the host
//! image interface, so tiles with a blob never take Cesium's size fast path.

use super::TileDiscardPolicy;
use crate::core::{Cartesian2, ImageHost};
use std::rc::Rc;

#[derive(Clone, Debug, Default)]
pub struct DiscardMissingTileImagePolicyOptions {
    /// Pixel positions compared against the tile image.
    pub pixels_to_check: Vec<Cartesian2>,
    /// Ignore positions whose alpha is zero in the missing image.
    pub disable_check_if_all_pixels_are_transparent: bool,
}

#[derive(Clone)]
pub struct DiscardMissingTileImagePolicy<H: ImageHost> {
    host: Rc<H>,
    pixels_to_check: Vec<Cartesian2>,
    /// `None` disables the check, matching a failed missing-image load or an
    /// all-transparent set of checked pixels.
    missing_image_pixels: Option<Vec<u8>>,
}

impl<H: ImageHost> std::fmt::Debug for DiscardMissingTileImagePolicy<H> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DiscardMissingTileImagePolicy")
            .field("pixels_to_check", &self.pixels_to_check)
            .field("enabled", &self.missing_image_pixels.is_some())
            .finish_non_exhaustive()
    }
}

impl<H: ImageHost> DiscardMissingTileImagePolicy<H> {
    /// Builds the policy from the already-decoded missing image. `None` is
    /// Cesium's failed-load state: the policy is ready but keeps every tile.
    pub fn new(host: Rc<H>, options: DiscardMissingTileImagePolicyOptions, missing_image: Option<&H::Image>) -> Self {
        let missing_image_pixels = missing_image.and_then(|image| {
            let (width, height) = host.dimensions(image);
            let pixels = host.image_pixels(image, width, height).ok()?;
            if options.disable_check_if_all_pixels_are_transparent
                && all_checked_pixels_transparent(&options.pixels_to_check, &pixels, width)
            {
                return None;
            }
            Some(pixels)
        });
        Self { host, pixels_to_check: options.pixels_to_check, missing_image_pixels }
    }

    /// Whether the check is active. False only when the missing image loaded
    /// but every checked pixel is transparent.
    pub fn is_enabled(&self) -> bool {
        self.missing_image_pixels.is_some()
    }
}

/// `index = pos.x * 4 + pos.y * width`, as a typed-array index. Fractional,
/// negative and non-finite values are outside the array, so reads yield the
/// JavaScript `undefined` the policy compares as equal to another `undefined`.
fn pixel_index(position: &Cartesian2, width: u32) -> Option<usize> {
    if !position.x.is_finite() || !position.y.is_finite() || position.x.fract() != 0.0 || position.y.fract() != 0.0 {
        return None;
    }
    let index = position.x * 4.0 + position.y * f64::from(width);
    if index < 0.0 {
        return None;
    }
    usize::try_from(index as u64).ok()
}

/// Cesium disables the check when no checked pixel has a non-zero alpha;
/// indices outside the image read as `undefined`, which does not count.
fn all_checked_pixels_transparent(pixels_to_check: &[Cartesian2], pixels: &[u8], width: u32) -> bool {
    pixels_to_check.iter().all(|position| {
        pixel_index(position, width).and_then(|index| pixels.get(index + 3).copied()).is_none_or(|alpha| alpha == 0)
    })
}

impl<H: ImageHost> TileDiscardPolicy<H::Image> for DiscardMissingTileImagePolicy<H> {
    fn is_ready(&self) -> bool {
        true
    }

    fn should_discard_image(&self, image: &H::Image) -> bool {
        let Some(missing) = &self.missing_image_pixels else {
            return false;
        };
        let (width, height) = self.host.dimensions(image);
        let Ok(pixels) = self.host.image_pixels(image, width, height) else {
            return false;
        };
        self.pixels_to_check.iter().all(|position| {
            let Some(index) = pixel_index(position, width) else {
                // Both sides read `undefined` at this offset.
                return true;
            };
            (0..4).all(|offset| pixels.get(index + offset) == missing.get(index + offset))
        })
    }
}
