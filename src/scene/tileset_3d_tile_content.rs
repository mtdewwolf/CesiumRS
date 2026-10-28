//! CesiumJS `Tileset3DTileContent`: external tileset structural content. The
//! synchronous loader callback installs the child tree before readiness is set.
use super::{FrameState, TileContentMetrics, TileId, TilesetContent};
use crate::core::{Cartesian3, Ray, Resource, RuntimeError};
use serde_json::Value;
pub struct Tileset3DTileContent {
    pub tile: TileId,
    pub resource: Resource,
    pub feature_properties_dirty: bool,
    pub metadata: Option<Value>,
    pub group: Option<Value>,
    ready: bool,
    destroyed: bool,
}
impl Tileset3DTileContent {
    pub fn from_json(
        tile: TileId,
        resource: Resource,
        json: &Value,
        load: impl FnOnce(&Resource, &Value, TileId) -> Result<(), RuntimeError>,
    ) -> Result<Self, RuntimeError> {
        load(&resource, json, tile)?;
        Ok(Self {
            tile,
            resource,
            feature_properties_dirty: false,
            metadata: None,
            group: None,
            ready: true,
            destroyed: false,
        })
    }
    pub fn ready(&self) -> bool {
        self.ready
    }
    pub fn url(&self) -> String {
        self.resource.get_url_component(true, false)
    }
    pub fn has_property(&self, _batch: usize, _name: &str) -> bool {
        false
    }
    pub fn get_feature(&self, _batch: usize) -> Option<()> {
        None
    }
    pub fn is_destroyed(&self) -> bool {
        self.destroyed
    }
    fn check(&self) -> Result<(), RuntimeError> {
        if self.destroyed {
            Err(RuntimeError::new("Content was destroyed"))
        } else {
            Ok(())
        }
    }
}
impl TilesetContent for Tileset3DTileContent {
    fn metrics(&self) -> TileContentMetrics {
        TileContentMetrics::default()
    }
    fn process(&mut self, _: &FrameState) -> Result<bool, RuntimeError> {
        self.check()?;
        Ok(self.ready)
    }
    fn update(&mut self, _: &mut FrameState) -> Result<(), RuntimeError> {
        self.check()
    }
    fn pick(&self, _: &Ray, _: &FrameState) -> Result<Option<Cartesian3>, RuntimeError> {
        self.check()?;
        Ok(None)
    }
    fn destroy(&mut self) -> Result<(), RuntimeError> {
        self.destroyed = true;
        self.ready = false;
        Ok(())
    }
}
