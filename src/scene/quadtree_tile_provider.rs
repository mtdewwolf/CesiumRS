//! Provider contract used by the actual quadtree traversal (CesiumJS 23.0.0).
use super::{
    frame_state::FrameState,
    quadtree_occluders::QuadtreeOccluders,
    quadtree_tile::{QuadtreeTile, QuadtreeTileHandle},
};
use crate::core::{TilingScheme, Visibility};
use std::sync::Arc;
/// Provider interface supplying tile data, visibility, and geometry error.
pub trait QuadtreeTileProvider {
    fn tiling_scheme(&self) -> Arc<dyn TilingScheme>;
    fn get_level_maximum_geometric_error(&self, level: u32) -> f64;
    fn load_tile(&mut self, frame_state: &FrameState, tile: &QuadtreeTileHandle);
    fn compute_tile_visibility(
        &self,
        tile: &QuadtreeTile,
        frame_state: &FrameState,
        occluders: Option<&QuadtreeOccluders>,
    ) -> Visibility;
    fn compute_distance_to_tile(&self, _tile: &QuadtreeTile, _frame_state: &FrameState) -> f64 {
        0.0
    }
    fn show_tile_this_frame(&mut self, tile: &QuadtreeTileHandle, frame_state: &mut FrameState);
    fn can_refine(&self, _tile: &QuadtreeTile) -> bool {
        true
    }
    fn can_render_without_losing_detail(&self, _tile: &QuadtreeTile) -> bool {
        false
    }
    fn compute_tile_load_priority(&self, _tile: &QuadtreeTile, _frame_state: &FrameState) -> f64 {
        0.0
    }
    fn initialize(&mut self, _frame_state: &FrameState) {}
    fn begin_update(&mut self, _frame_state: &FrameState) {}
    fn end_update(&mut self, _frame_state: &FrameState) {}
    fn cancel_reprojections(&mut self) {}
    fn update(&mut self, _frame_state: &FrameState) {}
    fn destroy(&mut self) {}
    fn is_destroyed(&self) -> bool {
        false
    }
}

/// Computes the default geometric error for level zero of a quadtree.
pub fn compute_default_level_zero_maximum_geometric_error(tiling_scheme: &dyn TilingScheme) -> f64 {
    (tiling_scheme.ellipsoid().maximum_radius * 2.0 * std::f64::consts::PI * 0.25)
        / (65.0 * tiling_scheme.get_number_of_x_tiles_at_level(0) as f64)
}
