//! CesiumJS `PpeTexture`: a per-point error texture reference and its metadata.

use super::PpeMetadata;

#[derive(Clone, Debug, PartialEq)]
pub struct PpeTexture {
    pub traits: PpeMetadata,
    pub no_data: Option<f64>,
    pub offset: Option<f64>,
    pub scale: Option<f64>,
    /// The index of the texture in the glTF.
    pub index: usize,
    pub tex_coord: Option<u32>,
}
