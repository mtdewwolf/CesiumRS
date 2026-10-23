//! CesiumJS `BoundingVolumeSemantics`: bounding volumes and heights stored
//! as `TILE_*` or `CONTENT_*` metadata semantics.
//!
//! Cesium checks the prefix with a `DeveloperError`; the port takes a
//! [`BoundingVolumePrefix`], so an invalid prefix cannot be passed.

use super::metadata_entity::MetadataEntity;
use super::metadata_value::{MetadataError, MetadataValue};

/// The semantic prefix: tile or content metadata.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum BoundingVolumePrefix {
    Tile,
    Content,
}

impl BoundingVolumePrefix {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Tile => "TILE",
            Self::Content => "CONTENT",
        }
    }
}

/// A bounding volume from metadata, keyed as in tileset JSON.
#[derive(Clone, Debug, PartialEq)]
pub enum SemanticBoundingVolume {
    /// `<prefix>_BOUNDING_BOX`.
    Box(MetadataValue),
    /// `<prefix>_BOUNDING_REGION`.
    Region(MetadataValue),
    /// `<prefix>_BOUNDING_SPHERE`; a four-element array arrives as a
    /// `Cartesian4`.
    Sphere(MetadataValue),
}

/// `parseAllBoundingVolumeSemantics`.
#[derive(Clone, Debug, PartialEq)]
pub struct BoundingVolumeSemanticValues {
    pub bounding_volume: Option<SemanticBoundingVolume>,
    pub minimum_height: Option<MetadataValue>,
    pub maximum_height: Option<MetadataValue>,
}

type Result<T> = std::result::Result<T, MetadataError>;

/// The property for a semantic, or `None` when it is undefined or null.
fn semantic(metadata: &dyn MetadataEntity, prefix: BoundingVolumePrefix, name: &str) -> Result<Option<MetadataValue>> {
    let value = metadata.get_property_by_semantic(&format!("{}_{name}", prefix.as_str()))?;
    Ok(value.filter(|v| *v != MetadataValue::Null))
}

pub fn parse_all_bounding_volume_semantics(
    prefix: BoundingVolumePrefix,
    metadata: &dyn MetadataEntity,
) -> Result<BoundingVolumeSemanticValues> {
    Ok(BoundingVolumeSemanticValues {
        bounding_volume: parse_bounding_volume_semantic(prefix, metadata)?,
        minimum_height: parse_minimum_height(prefix, metadata)?,
        maximum_height: parse_maximum_height(prefix, metadata)?,
    })
}

/// The box, else the region, else the sphere.
pub fn parse_bounding_volume_semantic(
    prefix: BoundingVolumePrefix,
    metadata: &dyn MetadataEntity,
) -> Result<Option<SemanticBoundingVolume>> {
    if let Some(bounding_box) = semantic(metadata, prefix, "BOUNDING_BOX")? {
        return Ok(Some(SemanticBoundingVolume::Box(bounding_box)));
    }
    if let Some(region) = semantic(metadata, prefix, "BOUNDING_REGION")? {
        return Ok(Some(SemanticBoundingVolume::Region(region)));
    }
    Ok(semantic(metadata, prefix, "BOUNDING_SPHERE")?.map(SemanticBoundingVolume::Sphere))
}

/// `<prefix>_MINIMUM_HEIGHT`, as stored (null included).
pub fn parse_minimum_height(
    prefix: BoundingVolumePrefix,
    metadata: &dyn MetadataEntity,
) -> Result<Option<MetadataValue>> {
    metadata.get_property_by_semantic(&format!("{}_MINIMUM_HEIGHT", prefix.as_str()))
}

/// `<prefix>_MAXIMUM_HEIGHT`, as stored (null included).
pub fn parse_maximum_height(
    prefix: BoundingVolumePrefix,
    metadata: &dyn MetadataEntity,
) -> Result<Option<MetadataValue>> {
    metadata.get_property_by_semantic(&format!("{}_MAXIMUM_HEIGHT", prefix.as_str()))
}
