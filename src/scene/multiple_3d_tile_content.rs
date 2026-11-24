//! Ordered child requests and aggregate content lifecycle for 3D Tiles 1.1.
use super::{
    cesium_3d_tileset::TileContentModelState,
    tileset_content::{content_headers, content_resource},
    *,
};
use crate::core::*;
use serde_json::Value;
use std::{cell::Cell, rc::Rc};
pub struct Multiple3DTileContent {
    contents: Vec<Box<dyn TilesetContent>>,
    ready: bool,
    failures: Vec<TileContentFailure>,
    header_indices: Vec<usize>,
    destroyed: bool,
}
impl Multiple3DTileContent {
    pub fn new(contents: Vec<Box<dyn TilesetContent>>) -> Self {
        Self {
            header_indices: (0..contents.len()).collect(),
            contents,
            ready: false,
            failures: Vec::new(),
            destroyed: false,
        }
    }
    pub fn ready(&self) -> bool {
        self.ready
    }
    pub fn contents(&self) -> &[Box<dyn TilesetContent>] {
        &self.contents
    }
    pub fn is_destroyed(&self) -> bool {
        self.destroyed
    }
}
impl TilesetContent for Multiple3DTileContent {
    fn set_feature_metadata(&mut self, metadata: &[FeatureMetadata]) {
        for (content, &index) in self.contents.iter_mut().zip(&self.header_indices) {
            if let Some(layer) = metadata.get(index) {
                content.set_feature_metadata(std::slice::from_ref(layer));
            }
        }
    }
    fn inner_contents(&self) -> Option<&[Box<dyn TilesetContent>]> {
        Some(&self.contents)
    }
    fn feature_properties_dirty(&self) -> bool {
        self.contents.iter().any(|c| c.feature_properties_dirty())
    }
    fn set_feature_properties_dirty(&mut self, dirty: bool) {
        for c in &mut self.contents {
            c.set_feature_properties_dirty(dirty);
        }
    }
    fn apply_style(&mut self, style: Option<&Cesium3DTileStyle>) -> Result<(), RuntimeError> {
        self.contents.iter_mut().try_for_each(|c| c.apply_style(style))
    }
    fn apply_debug_settings(&mut self, enabled: bool, color: Color) -> Result<(), RuntimeError> {
        self.contents.iter_mut().try_for_each(|c| c.apply_debug_settings(enabled, color))
    }
    fn configure_model(&mut self, state: &TileContentModelState) {
        for content in &mut self.contents {
            content.configure_model(state);
        }
    }
    fn process_with_frame(&mut self, frame: &mut FrameState) -> Result<bool, RuntimeError> {
        let mut ready = true;
        for content in &mut self.contents {
            ready &= content.process_with_frame(frame)?;
        }
        self.ready |= ready;
        Ok(self.ready)
    }
    fn is_renderable(&self) -> bool {
        self.contents.iter().any(|c| c.is_renderable())
    }
    fn take_failures(&mut self) -> Vec<TileContentFailure> {
        std::mem::take(&mut self.failures)
    }
    fn metrics(&self) -> TileContentMetrics {
        self.contents.iter().fold(TileContentMetrics::default(), |mut total, c| {
            let m = c.metrics();
            total.features += m.features;
            total.points += m.points;
            total.triangles += m.triangles;
            total.geometry_bytes += m.geometry_bytes;
            total.texture_bytes += m.texture_bytes;
            total.batch_table_bytes += m.batch_table_bytes;
            total
        })
    }
    fn texture_allocations(&self) -> Vec<(Option<String>, u64)> {
        self.contents.iter().flat_map(|c| c.texture_allocations()).collect()
    }
    fn process(&mut self, frame: &FrameState) -> Result<bool, RuntimeError> {
        let mut ready = true;
        for content in &mut self.contents {
            ready &= content.process(frame)?;
        }
        self.ready |= ready;
        Ok(self.ready)
    }
    fn update(&mut self, frame: &mut FrameState) -> Result<(), RuntimeError> {
        if self.destroyed {
            return Err(RuntimeError::new("This object was destroyed."));
        }
        for content in &mut self.contents {
            content.update(frame)?;
        }
        if !self.ready {
            let mut ready = true;
            for content in &mut self.contents {
                ready &= content.process(frame)?;
            }
            self.ready |= ready;
        }
        Ok(())
    }
    fn pick(&self, ray: &Ray, frame: &FrameState) -> Result<Option<Cartesian3>, RuntimeError> {
        if !self.ready {
            return Ok(None);
        }
        let mut hit = None;
        let mut distance = f64::INFINITY;
        for content in &self.contents {
            if let Some(candidate) = content.pick(ray, frame)? {
                let d = candidate.distance_squared(ray.origin);
                if d < distance {
                    hit = Some(candidate);
                    distance = d;
                }
            }
        }
        Ok(hit)
    }
    fn destroy(&mut self) -> Result<(), RuntimeError> {
        for content in &mut self.contents {
            content.destroy()?;
        }
        self.contents.clear();
        self.ready = false;
        self.destroyed = true;
        Ok(())
    }
    fn take_expansions(&mut self) -> Vec<TileExpansion> {
        self.contents.iter_mut().flat_map(|c| c.take_expansions()).collect()
    }
}

pub(super) fn request_multiple(
    loader: &dyn TilesetContentLoader,
    request: TileContentRequest,
    remaining: &Rc<Cell<u64>>,
) -> Option<TileContentFuture> {
    let mut futures = Vec::new();
    remaining.set(0);
    for (index, content) in content_headers(&request.header).into_iter().enumerate() {
        let Some(resource) = content_resource(&request.base_resource, &content) else { continue };
        let mut header = request.header.clone();
        if let Some(map) = header.as_object_mut() {
            map.remove("contents");
            map.remove("implicitTiling");
            if let Some(e) = map.get_mut("extensions").and_then(Value::as_object_mut) {
                e.remove("3DTILES_multiple_contents");
            }
            map.insert("content".into(), content);
        }
        let child = TileContentRequest { resource, header, ..request.clone() };
        let resource = child.resource.clone();
        remaining.set(remaining.get() + 1);
        let remaining = Rc::clone(remaining);
        match loader.request(child) {
            Some(future) => futures.push(async move {
                let result = future.await;
                remaining.set(remaining.get().saturating_sub(1));
                (index, resource, result)
            }),
            None => {
                request.cancelled.set(true);
                return None;
            }
        }
    }
    Some(Box::pin(async move {
        let mut contents = Vec::new();
        let mut failures = Vec::new();
        let mut header_indices = Vec::new();
        for (index, resource, result) in futures_util::future::join_all(futures).await {
            match result {
                Ok(content) => {
                    contents.push(content);
                    header_indices.push(index);
                }
                Err(error) => failures.push(TileContentFailure { resource, message: error.message }),
            }
        }
        let mut multiple = Multiple3DTileContent::new(contents);
        multiple.header_indices = header_indices;
        multiple.failures = failures;
        Ok(Box::new(multiple) as Box<dyn TilesetContent>)
    }))
}
