//! CesiumJS `MeshPrimitiveGpmLocal`: a mesh primitive's `NGA_gpm_local` per-point error textures.

use super::PpeTexture;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct MeshPrimitiveGpmLocal {
    pub ppe_textures: Vec<PpeTexture>,
}
