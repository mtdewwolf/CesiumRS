//! CesiumJS `Cesium3DTile`: the per-tile arena node. Tileset construction and
//! traversal live in [`super::cesium_3d_tileset`]; this module owns the tile
//! state machine helpers shared by traversal, loading and rendering.
use super::{scene::Result, *};
use crate::core::*;
use serde_json::Value;

/// A node in the tileset arena. Its parent and children are stable tile IDs.
pub type Cesium3DTile = TilesetTile;

impl TilesetTile {
    pub fn has_empty_content(&self) -> bool {
        self.content_resource.is_none()
    }
    pub fn content_ready(&self) -> bool {
        self.state == Cesium3DTileContentState::Ready
    }
    pub fn content_unloaded(&self) -> bool {
        self.state == Cesium3DTileContentState::Unloaded
    }
    pub fn content_expired(&self) -> bool {
        self.state == Cesium3DTileContentState::Expired
    }
    pub fn content_failed(&self) -> bool {
        self.state == Cesium3DTileContentState::Failed
    }
    pub fn has_unloaded_renderable_content(&self) -> bool {
        self.has_renderable_content && self.content_unloaded()
    }
    /// `contentAvailable`: ready renderable content, or expired content still
    /// rendering while its replacement loads.
    pub fn content_available(&self) -> bool {
        (self.content_ready() && self.has_renderable_content)
            || (self.expired_content.is_some() && !self.content_failed())
    }
    pub fn is_visible(&self) -> bool {
        self.traversal.visible && self.traversal.in_request_volume
    }
    pub fn extras(&self) -> Option<&Value> {
        self.header.get("extras")
    }
    /// Geometric error scaled by the maximum scale component of `computed_transform`,
    /// matching CesiumJS `Cesium3DTile.prototype.updateGeometricErrorScale`.
    pub fn scaled_geometric_error(&self) -> f64 {
        self.geometric_error * self.computed_transform.get_maximum_scale()
    }
    /// Updates the computed transform and every bounding volume. Cesium skips
    /// the work unless the transform or vertical exaggeration changed.
    pub fn update_transform(&mut self, parent: Matrix4, exaggeration: [f64; 2]) -> Result<()> {
        let transform = parent.multiply(&self.transform);
        if self.computed_transform == transform && self.volume_exaggeration == exaggeration {
            return Ok(());
        }
        let volume = |header: &Value| {
            TilesetVolume::from_header(header, transform, self.initial_transform, exaggeration[0], exaggeration[1])
        };
        let bounds = volume(&self.source_bounds)?;
        let content_bounds = if tileset_content::is_multiple(&self.header) {
            None
        } else {
            tileset_content::content_headers(&self.header)
                .first()
                .and_then(|h| h.get("boundingVolume"))
                .map(volume)
                .transpose()?
        };
        let request_bounds = self.header.get("viewerRequestVolume").map(volume).transpose()?;
        self.computed_transform = transform;
        self.volume_exaggeration = exaggeration;
        self.bounding_sphere = *bounds.sphere();
        self.volume = bounds;
        self.content_volume = content_bounds;
        self.viewer_request_volume = request_bounds;
        Ok(())
    }
    pub fn content_bounding_volume(&self) -> &TilesetVolume {
        self.content_volume.as_ref().unwrap_or(&self.volume)
    }
    pub fn distance_to_tile(&self, frame: &FrameState) -> f64 {
        self.volume.for_frame(frame).distance(frame)
    }
    pub fn distance_to_tile_center(&self, frame: &FrameState) -> f64 {
        let center = self.volume.for_frame(frame).sphere().center;
        frame.uniform.camera.direction.dot(center.subtract(frame.uniform.camera.position))
    }
    pub fn inside_viewer_request_volume(&self, frame: &FrameState) -> bool {
        self.viewer_request_volume.as_ref().is_none_or(|v| v.distance(frame) == 0.)
    }
}
