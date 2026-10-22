//! CesiumJS `StencilConstants`: the stencil bits 3D Tiles, skip-LOD and
//! classification share.

use super::{StencilFunction, StencilOperation};
use crate::renderer::render_state::{StencilOperation as StencilOps, StencilTest};

pub const CESIUM_3D_TILE_MASK: i32 = 0x80;
pub const SKIP_LOD_MASK: i32 = 0x70;
pub const SKIP_LOD_BIT_SHIFT: i32 = 4;
pub const CLASSIFICATION_MASK: i32 = 0x0f;

/// The stencil test that marks 3D Tiles fragments with `CESIUM_3D_TILE_MASK`.
pub fn set_cesium_3d_tile_bit() -> StencilTest {
    let operation = StencilOps {
        fail: StencilOperation::Keep.value(),
        z_fail: StencilOperation::Keep.value(),
        z_pass: StencilOperation::Replace.value(),
    };
    StencilTest {
        enabled: true,
        front_function: StencilFunction::Always.value(),
        front_operation: operation.clone(),
        back_function: StencilFunction::Always.value(),
        back_operation: operation,
        reference: CESIUM_3D_TILE_MASK,
        mask: CESIUM_3D_TILE_MASK as i64,
    }
}
