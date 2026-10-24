//! CesiumJS `PpeMetadata`: the range and source of a per-point error texture.
//! `source` is the string as given, usually a [`super::PpeSource`] value.

#[derive(Clone, Debug, PartialEq)]
pub struct PpeMetadata {
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub source: String,
}
