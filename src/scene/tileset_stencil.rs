//! 3D Tiles classification bits and skip-LOD visibility derived states.
use super::stencil_constants::*;
use crate::{
    core::webgl_constants as gl,
    renderer::{
        render_state::{ColorMask, PolygonOffset},
        RenderState,
    },
};
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TilesetDrawKind {
    Regular,
    Stencil { selection_depth: usize },
    Backface,
}
impl TilesetDrawKind {
    pub fn polygon_offset(self) -> bool {
        !matches!(self, Self::Regular)
    }
    pub fn render_state(self, state: &RenderState) -> RenderState {
        let mut result = state.clone();
        result.stencil_test = set_cesium_3d_tile_bit();
        result.stencil_mask = CESIUM_3D_TILE_MASK as i64;
        match self {
            Self::Regular => {}
            Self::Stencil { selection_depth } => {
                result.stencil_test.mask = SKIP_LOD_MASK as i64;
                result.stencil_test.reference =
                    CESIUM_3D_TILE_MASK | ((selection_depth as i32).wrapping_shl(SKIP_LOD_BIT_SHIFT as u32));
                result.stencil_test.front_function = gl::GEQUAL;
                result.stencil_test.back_function = gl::GEQUAL;
                result.stencil_mask = (CESIUM_3D_TILE_MASK | SKIP_LOD_MASK) as i64;
            }
            Self::Backface => {
                result.cull.enabled = true;
                result.cull.face = gl::FRONT;
                result.color_mask = ColorMask { red: false, green: false, blue: false, alpha: false };
                result.polygon_offset = PolygonOffset { enabled: true, factor: 5., units: 5. };
            }
        }
        result
    }
}
