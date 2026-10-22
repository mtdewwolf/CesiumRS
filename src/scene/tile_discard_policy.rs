//! CesiumJS `TileDiscardPolicy`: decides whether an imagery provider's tile
//! image should be discarded. Cesium's abstract base class is this trait.

/// Implemented by [`NeverTileDiscardPolicy`](super::NeverTileDiscardPolicy)
/// and the policies imagery providers define. `Image` is the host's decoded
/// image type.
pub trait TileDiscardPolicy<Image> {
    /// Whether the policy has what it needs to decide.
    fn is_ready(&self) -> bool;
    /// Whether `image` should be replaced by no tile.
    fn should_discard_image(&self, image: &Image) -> bool;
}
