//! CesiumJS `NeverTileDiscardPolicy`: a discard policy that keeps every tile.

use super::TileDiscardPolicy;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct NeverTileDiscardPolicy;

impl<Image> TileDiscardPolicy<Image> for NeverTileDiscardPolicy {
    fn is_ready(&self) -> bool {
        true
    }

    fn should_discard_image(&self, _image: &Image) -> bool {
        false
    }
}
