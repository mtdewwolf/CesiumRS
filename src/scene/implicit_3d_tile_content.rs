//! Loads a subtree and transfers its expansion to the owning tileset on installation.
use super::{implicit_subtree::SubtreeSource, *};
use crate::core::*;
use std::collections::HashMap;

pub struct Implicit3DTileContent {
    pub resource: Resource,
    expansion: Option<TileExpansion>,
    destroyed: bool,
}
impl Implicit3DTileContent {
    pub async fn load(
        client: &ResourceClient,
        resource: Resource,
        source: SubtreeSource<'_>,
        implicit: ImplicitContentRequest,
    ) -> Result<Self, RuntimeError> {
        let mut buffers = HashMap::<String, Vec<u8>>::new();
        let subtree = loop {
            let mut missing = None;

            let result = ImplicitSubtree::from_subtree_json(
                &resource,
                match &source {
                    SubtreeSource::Json(v) => SubtreeSource::Json(v.clone()),
                    SubtreeSource::Binary(v) => SubtreeSource::Binary(v),
                },
                &implicit.description,
                implicit.coordinates,
                &mut |resource| {
                    if let Some(buffer) = buffers.get(&resource.url()) {
                        Ok(buffer.clone())
                    } else {
                        missing = Some(resource.clone());
                        Err(MetadataError::Runtime("external buffer is pending".into()))
                    }
                },
            );
            if let Some(resource) = missing {
                let bytes = client
                    .fetch_array_buffer(&resource)
                    .ok_or_else(|| RuntimeError::new("subtree buffer request was throttled"))?
                    .await
                    .map_err(|e| RuntimeError::new(e.to_string()))?;
                buffers.insert(resource.url(), bytes);
                continue;
            }
            break result.map_err(|e| RuntimeError::new(e.to_string()))?;
        };
        Ok(Self {
            resource,
            expansion: Some(TileExpansion::Implicit {
                subtree: Box::new(subtree),
                description: Box::new(implicit.description),
                coordinates: implicit.coordinates,
            }),
            destroyed: false,
        })
    }
    pub fn ready(&self) -> bool {
        !self.destroyed
    }
}
impl TilesetContent for Implicit3DTileContent {
    fn is_renderable(&self) -> bool {
        false
    }
    fn metrics(&self) -> TileContentMetrics {
        Default::default()
    }
    fn process(&mut self, _: &FrameState) -> Result<bool, RuntimeError> {
        Ok(self.ready())
    }
    fn update(&mut self, _: &mut FrameState) -> Result<(), RuntimeError> {
        Ok(())
    }
    fn pick(&self, _: &Ray, _: &FrameState) -> Result<Option<Cartesian3>, RuntimeError> {
        Ok(None)
    }
    fn destroy(&mut self) -> Result<(), RuntimeError> {
        self.destroyed = true;
        self.expansion = None;
        Ok(())
    }
    fn take_expansions(&mut self) -> Vec<TileExpansion> {
        self.expansion.take().into_iter().collect()
    }
}
