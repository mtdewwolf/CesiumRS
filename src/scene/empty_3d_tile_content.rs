//! CesiumJS `Empty3DTileContent`: empty content for tiles in a 3D Tiles tileset
//! that do not have content (e.g. used for hierarchical culling optimizations).

use crate::core::RuntimeError;

/// Represents empty content for tiles in a 3D Tiles tileset.
#[derive(Clone, Debug, PartialEq)]
pub struct Empty3DTileContent {
    pub feature_properties_dirty: bool,
    destroyed: bool,
}

impl Default for Empty3DTileContent {
    fn default() -> Self {
        Self::new()
    }
}

impl Empty3DTileContent {
    pub fn new() -> Self {
        Self {
            feature_properties_dirty: false,
            destroyed: false,
        }
    }

    pub fn features_length(&self) -> usize {
        0
    }

    pub fn points_length(&self) -> usize {
        0
    }

    pub fn triangles_length(&self) -> usize {
        0
    }

    pub fn geometry_byte_length(&self) -> usize {
        0
    }

    pub fn textures_byte_length(&self) -> usize {
        0
    }

    pub fn batch_table_byte_length(&self) -> usize {
        0
    }

    pub fn ready(&self) -> bool {
        true
    }

    pub fn url(&self) -> Option<&str> {
        None
    }

    pub fn has_property(&self, _batch_id: usize, _name: &str) -> bool {
        false
    }

    pub fn get_feature(&self, _batch_id: usize) -> Option<()> {
        None
    }

    pub fn set_metadata(&mut self) -> Result<(), RuntimeError> {
        Err(RuntimeError::new(
            "Empty3DTileContent cannot have content metadata",
        ))
    }

    pub fn set_group(&mut self) -> Result<(), RuntimeError> {
        Err(RuntimeError::new(
            "Empty3DTileContent cannot have group metadata",
        ))
    }

    pub fn apply_debug_settings(&mut self, _enabled: bool, _color: Option<crate::core::Color>) {}

    pub fn apply_style(&mut self, _style: Option<&str>) {}

    pub fn update(&mut self) {}

    pub fn is_destroyed(&self) -> bool {
        self.destroyed
    }

    pub fn destroy(&mut self) {
        self.destroyed = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_content_properties_match_upstream() {
        let mut content = Empty3DTileContent::new();
        assert_eq!(content.features_length(), 0);
        assert_eq!(content.points_length(), 0);
        assert_eq!(content.triangles_length(), 0);
        assert_eq!(content.geometry_byte_length(), 0);
        assert_eq!(content.textures_byte_length(), 0);
        assert_eq!(content.batch_table_byte_length(), 0);
        assert!(content.ready());
        assert_eq!(content.url(), None);
        assert_eq!(content.has_property(0, "height"), false);
        assert_eq!(content.get_feature(0), None);
        assert_eq!(content.feature_properties_dirty, false);
        assert_eq!(content.is_destroyed(), false);

        assert!(content.set_metadata().is_err());
        assert!(content.set_group().is_err());

        content.destroy();
        assert_eq!(content.is_destroyed(), true);
    }
}
