//! CesiumJS `DiscardEmptyTileImagePolicy`: discards a tile when the provider
//! returned the shared empty image.
//!
//! CesiumJS creates that image itself, once, from [`EMPTY_IMAGE_URI`]. Here
//! the host decodes it and hands the shared image to the policy, which
//! compares tile images with it by identity.

use super::TileDiscardPolicy;
use std::rc::Rc;

/// A 1×1 transparent PNG.
pub const EMPTY_IMAGE_URI: &str = "data:image/png;base64,iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mNkYAAAAAYAAjCB0C8AAAAASUVORK5CYII=";

#[derive(Debug)]
pub struct DiscardEmptyTileImagePolicy<Image> {
    empty_image: Rc<Image>,
}

impl<Image> Clone for DiscardEmptyTileImagePolicy<Image> {
    fn clone(&self) -> Self {
        Self { empty_image: self.empty_image.clone() }
    }
}

impl<Image> DiscardEmptyTileImagePolicy<Image> {
    /// `empty_image` is the host's decoding of [`EMPTY_IMAGE_URI`], shared by
    /// every policy and provider.
    pub fn new(empty_image: Rc<Image>) -> Self {
        Self { empty_image }
    }

    /// `DiscardEmptyTileImagePolicy.EMPTY_IMAGE`.
    pub fn empty_image(&self) -> &Rc<Image> {
        &self.empty_image
    }
}

impl<Image> TileDiscardPolicy<Rc<Image>> for DiscardEmptyTileImagePolicy<Image> {
    fn is_ready(&self) -> bool {
        true
    }

    fn should_discard_image(&self, image: &Rc<Image>) -> bool {
        Rc::ptr_eq(image, &self.empty_image)
    }
}
