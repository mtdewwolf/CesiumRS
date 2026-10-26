//! Resource loading and structural 3D Tiles content. Renderable content factories
//! own their format-specific meshes and GPU resources, as Cesium's content map does.
use super::{cesium_3d_tileset::*, *};
use crate::core::{resource::DerivedResourceOptions, *};
use serde_json::Value;
use std::{
    cell::{Cell, RefCell},
    collections::HashMap,
    rc::Rc,
};

pub enum TileExpansion {
    External { resource: Resource, json: Value },
    Implicit { subtree: Box<ImplicitSubtree>, description: Box<ImplicitTileset>, coordinates: ImplicitTileCoordinates },
}
#[derive(Clone)]
pub struct ImplicitContentRequest {
    pub description: ImplicitTileset,
    pub coordinates: ImplicitTileCoordinates,
}
pub struct TileContentFailure {
    pub resource: Resource,
    pub message: String,
}

/// This is the boundary to Model3DTileContent, PointCloud3DTileContent, etc.
/// A factory must return real content or an error; bytes never stand in for a
/// successfully rendered tile.
pub trait RenderableTileFactory {
    fn create(&self, request: TileContentRequest, kind: Cesium3DTileContentType, bytes: Vec<u8>) -> TileContentFuture;
}
#[derive(Clone)]
pub struct TilesetResourceLoader {
    pub client: ResourceClient,
    pub factory: Option<Rc<dyn RenderableTileFactory>>,
}
impl TilesetResourceLoader {
    pub fn new(client: ResourceClient, factory: Option<Rc<dyn RenderableTileFactory>>) -> Self {
        Self { client, factory }
    }
}
fn runtime(error: impl std::fmt::Display) -> RuntimeError {
    RuntimeError::new(error.to_string())
}
fn payload_kind(bytes: &[u8]) -> Result<(Cesium3DTileContentType, Option<Value>), RuntimeError> {
    use Cesium3DTileContentType::*;
    let magic = bytes.get(..4).and_then(|b| std::str::from_utf8(b).ok()).unwrap_or("");
    if magic == "glTF" {
        return Ok((GltfBinary, None));
    }
    if let Some(kind) = Cesium3DTileContentType::from_str_value(magic).filter(|k| k.is_binary_format()) {
        return Ok((kind, None));
    }
    let json: Value = serde_json::from_slice(bytes).map_err(|_| RuntimeError::new("Invalid tile content."))?;
    let defined = |key: &str| json.get(key).is_some_and(|v| !v.is_null());
    let kind = if defined("root") {
        ExternalTileset
    } else if defined("asset") {
        Gltf
    } else if defined("tileAvailability") {
        ImplicitSubtreeJson
    } else if defined("type") {
        GeoJson
    } else if defined("voxelTable") {
        VoxelJson
    } else {
        return Err(RuntimeError::new("Invalid tile content."));
    };
    Ok((kind, Some(json)))
}
impl TilesetContentLoader for TilesetResourceLoader {
    fn request(&self, request: TileContentRequest) -> Option<TileContentFuture> {
        let future = self.client.fetch_array_buffer(&request.resource)?;
        let client = self.client.clone();
        let factory = self.factory.clone();
        Some(Box::pin(async move {
            let bytes = future.await.map_err(runtime)?;
            if request.cancelled.get() {
                return Err(RuntimeError::new("tile request cancelled"));
            }
            let (kind, json) = payload_kind(&bytes)?;
            match kind {
                Cesium3DTileContentType::ExternalTileset => Ok(Box::new(StructuralContent {
                    expansion: Some(TileExpansion::External { resource: request.resource, json: json.unwrap() }),
                    destroyed: false,
                }) as Box<dyn TilesetContent>),
                Cesium3DTileContentType::ImplicitSubtree | Cesium3DTileContentType::ImplicitSubtreeJson => {
                    let implicit = request
                        .implicit
                        .ok_or_else(|| RuntimeError::new("implicit subtree content requires implicit coordinates"))?;
                    let mut buffers = HashMap::<String, Vec<u8>>::new();
                    let subtree = loop {
                        let mut missing = None;
                        let source = if let Some(json) = &json {
                            implicit_subtree::SubtreeSource::Json(json.clone())
                        } else {
                            implicit_subtree::SubtreeSource::Binary(&bytes)
                        };
                        let result = ImplicitSubtree::from_subtree_json(
                            &request.resource,
                            source,
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
                                .map_err(runtime)?;
                            buffers.insert(resource.url(), bytes);
                            continue;
                        }
                        break result.map_err(runtime)?;
                    };
                    Ok(Box::new(StructuralContent {
                        expansion: Some(TileExpansion::Implicit {
                            subtree: Box::new(subtree),
                            description: Box::new(implicit.description),
                            coordinates: implicit.coordinates,
                        }),
                        destroyed: false,
                    }) as Box<dyn TilesetContent>)
                }
                _ => {
                    factory
                        .ok_or_else(|| {
                            RuntimeError::new(format!("No renderable content factory for {}", kind.as_str()))
                        })?
                        .create(request, kind, bytes)
                        .await
                }
            }
        }))
    }
}
struct StructuralContent {
    expansion: Option<TileExpansion>,
    destroyed: bool,
}
impl TilesetContent for StructuralContent {
    fn is_renderable(&self) -> bool {
        false
    }
    fn metrics(&self) -> TileContentMetrics {
        TileContentMetrics::default()
    }
    fn process(&mut self, _: &FrameState) -> Result<bool, RuntimeError> {
        Ok(true)
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

pub struct MultipleTilesetContent {
    contents: Vec<Box<dyn TilesetContent>>,
    ready: bool,
    failures: Vec<TileContentFailure>,
}
impl MultipleTilesetContent {
    pub fn new(contents: Vec<Box<dyn TilesetContent>>) -> Self {
        Self { contents, ready: false, failures: Vec::new() }
    }
    pub fn contents(&self) -> &[Box<dyn TilesetContent>] {
        &self.contents
    }
}
impl TilesetContent for MultipleTilesetContent {
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
        for content in &mut self.contents {
            content.update(frame)?;
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
        Ok(())
    }
    fn take_expansions(&mut self) -> Vec<TileExpansion> {
        self.contents.iter_mut().flat_map(|c| c.take_expansions()).collect()
    }
}

pub(super) fn content_headers(header: &Value) -> Vec<Value> {
    if let Some(contents) = header
        .pointer("/extensions/3DTILES_multiple_contents/contents")
        .or_else(|| header.get("contents"))
        .and_then(Value::as_array)
    {
        return contents.clone();
    }
    header.get("content").filter(|c| !c.is_null()).cloned().into_iter().collect()
}
pub(super) fn is_multiple(header: &Value) -> bool {
    header.pointer("/extensions/3DTILES_multiple_contents").is_some()
        || header.get("contents").and_then(Value::as_array).is_some_and(|a| a.len() > 1)
}
pub(super) fn content_resource(base: &Resource, content: &Value) -> Option<Resource> {
    let uri = content.get("url").or_else(|| content.get("uri")).and_then(Value::as_str);
    if uri == Some("") {
        return None;
    }
    base.get_derived_resource(DerivedResourceOptions { url: uri.map(str::to_owned), ..Default::default() })
}
pub(super) fn request_multiple(
    loader: &dyn TilesetContentLoader,
    request: TileContentRequest,
    remaining: &Rc<Cell<u64>>,
) -> Option<TileContentFuture> {
    let mut futures = Vec::new();
    remaining.set(0);
    for content in content_headers(&request.header) {
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
                (resource, result)
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
        for (resource, result) in futures_util::future::join_all(futures).await {
            match result {
                Ok(content) => contents.push(content),
                Err(error) => failures.push(TileContentFailure { resource, message: error.message }),
            }
        }
        let mut multiple = MultipleTilesetContent::new(contents);
        multiple.failures = failures;
        Ok(Box::new(multiple) as Box<dyn TilesetContent>)
    }))
}

#[derive(Clone)]
pub struct HeightSubscription(pub(super) Rc<Cell<bool>>);
impl HeightSubscription {
    pub fn cancel(&self) {
        self.0.set(false);
    }
    pub fn is_active(&self) -> bool {
        self.0.get()
    }
}
pub(super) struct TileHeightRequest {
    pub position: Cartographic,
    pub ellipsoid: Ellipsoid,
    pub callback: Rc<RefCell<dyn FnMut()>>,
    pub active: Rc<Cell<bool>>,
    pub invoked: Rc<Cell<bool>>,
}
