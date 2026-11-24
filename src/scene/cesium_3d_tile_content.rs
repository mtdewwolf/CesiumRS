//! Shared interface for structural and renderable 3D Tiles content.
use super::{cesium_3d_tileset::TileContentModelState, scene::Result, *};
use crate::core::{Cartesian3, Color, Ray};
/// Concrete content owns GPU resources and/or CPU meshes. A content loader
/// reports failure through Result; dropping an in-flight request cancels it.
pub trait Cesium3DTileContent {
    /// Metadata for each available content header, in header order.
    fn set_feature_metadata(&mut self, _metadata: &[FeatureMetadata]) {}
    fn get_feature(&self, _id: usize) -> Result<Option<std::rc::Rc<dyn TileFeature>>> {
        Ok(None)
    }
    fn has_property(&self, id: usize, name: &str) -> Result<bool> {
        self.get_feature(id)?.map_or(Ok(false), |f| f.has_property(name))
    }
    fn inner_contents(&self) -> Option<&[Box<dyn Cesium3DTileContent>]> {
        None
    }
    fn feature_properties_dirty(&self) -> bool {
        false
    }
    fn set_feature_properties_dirty(&mut self, _dirty: bool) {}
    fn configure_model(&mut self, _state: &TileContentModelState) {}
    /// Applies a tileset style to this tile's features, if supported by the content.
    fn apply_style(&mut self, _style: Option<&Cesium3DTileStyle>) -> Result<()> {
        Ok(())
    }
    /// Applies tileset debug colorization settings to this tile's content.
    fn apply_debug_settings(&mut self, _enabled: bool, _color: Color) -> Result<()> {
        Ok(())
    }
    /// Mutable processing lets model content defer readiness until after the
    /// real frame renders, just like a standalone model's ready event.
    fn process_with_frame(&mut self, frame: &mut FrameState) -> Result<bool> {
        self.process(frame)
    }
    fn is_renderable(&self) -> bool {
        true
    }
    fn take_failures(&mut self) -> Vec<TileContentFailure> {
        Vec::new()
    }
    /// Tileset accounting includes descendants. Container content has no features or geometry of its own.
    fn metrics(&self) -> TileContentMetrics;
    fn own_metrics(&self) -> TileContentMetrics {
        if self.inner_contents().is_some() {
            TileContentMetrics::default()
        } else {
            self.metrics()
        }
    }
    fn process(&mut self, frame: &FrameState) -> Result<bool>;
    fn update(&mut self, frame: &mut FrameState) -> Result<()>;
    fn pick(&self, ray: &Ray, frame: &FrameState) -> Result<Option<Cartesian3>>;
    fn destroy(&mut self) -> Result<()>;
    /// Model contents override this with stable GPU texture identities. Other
    /// content owns its texture bytes independently.
    fn texture_allocations(&self) -> Vec<(Option<String>, u64)> {
        vec![(None, self.metrics().texture_bytes)]
    }
    fn take_expansions(&mut self) -> Vec<TileExpansion> {
        Vec::new()
    }
}

/// Shared, bounds-checked little-endian framing for legacy tile payloads.
pub(super) fn read_u32(bytes: &[u8], offset: usize) -> Result<usize> {
    let word = bytes
        .get(offset..)
        .and_then(|b| b.first_chunk::<4>())
        .ok_or_else(|| crate::core::RuntimeError::new("Truncated 3D tile header"))?;
    Ok(u32::from_le_bytes(*word) as usize)
}
pub(super) fn tile_payload<'a>(bytes: &'a [u8], offset: usize, magic: &[u8; 4]) -> Result<&'a [u8]> {
    let bytes = bytes.get(offset..).ok_or_else(|| crate::core::RuntimeError::new("Invalid tile offset"))?;
    if bytes.get(..4) != Some(magic) || read_u32(bytes, 4)? != 1 {
        return Err(crate::core::RuntimeError::new("Invalid 3D tile magic or unsupported version"));
    }
    let len = read_u32(bytes, 8)?;
    if magic == b"geom" && len == 0 {
        return Ok(&[]);
    }
    bytes.get(..len).ok_or_else(|| crate::core::RuntimeError::new("Truncated 3D tile payload"))
}
