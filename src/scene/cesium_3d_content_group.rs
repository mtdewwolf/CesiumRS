//! CesiumJS `Cesium3DContentGroup`: the metadata group a tile content
//! belongs to.
//!
//! CesiumJS shares the tileset's group object; this owns a copy of it, as
//! [`GroupMetadata`] owns a copy of its properties.

use super::GroupMetadata;

#[derive(Clone, Debug, PartialEq)]
pub struct Cesium3DContentGroup {
    metadata: GroupMetadata,
}

impl Cesium3DContentGroup {
    pub fn new(metadata: GroupMetadata) -> Self {
        Self { metadata }
    }

    pub fn metadata(&self) -> &GroupMetadata {
        &self.metadata
    }

    pub fn metadata_mut(&mut self) -> &mut GroupMetadata {
        &mut self.metadata
    }
}
