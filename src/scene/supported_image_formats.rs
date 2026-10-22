//! CesiumJS `SupportedImageFormats`: the optional image formats the context
//! can decode.

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SupportedImageFormats {
    pub webp: bool,
    pub basis: bool,
}
