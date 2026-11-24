//! Resource loading and structural 3D Tiles content. Renderable content factories
//! own their format-specific meshes and GPU resources, as Cesium's content map does.
use super::{cesium_3d_tileset::*, *};
use crate::core::{resource::DerivedResourceOptions, *};
use serde_json::Value;
use std::{
    cell::{Cell, RefCell},
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
        let loader = self.clone();
        Some(Box::pin(async move { loader.create_payload(request, future.await.map_err(runtime)?).await }))
    }
}
impl TilesetResourceLoader {
    fn create_payload(&self, request: TileContentRequest, bytes: Vec<u8>) -> TileContentFuture {
        let loader = self.clone();
        Box::pin(async move {
            if request.cancelled.get() {
                return Err(RuntimeError::new("tile request cancelled"));
            }
            let (kind, json) = payload_kind(&bytes)?;
            match kind {
                Cesium3DTileContentType::Composite => {
                    let content = Composite3DTileContent::from_tile_type(
                        request.resource.clone(),
                        &bytes,
                        0,
                        |resource, child| {
                            loader.create_payload(TileContentRequest { resource, ..request.clone() }, child.to_vec())
                        },
                    )
                    .await?;
                    Ok(Box::new(content) as Box<dyn TilesetContent>)
                }
                Cesium3DTileContentType::ExternalTileset => Ok(Box::new(StructuralContent {
                    expansion: Some(TileExpansion::External { resource: request.resource, json: json.unwrap() }),
                    destroyed: false,
                }) as Box<dyn TilesetContent>),
                Cesium3DTileContentType::ImplicitSubtree | Cesium3DTileContentType::ImplicitSubtreeJson => {
                    let implicit = request
                        .implicit
                        .ok_or_else(|| RuntimeError::new("implicit subtree content requires implicit coordinates"))?;
                    let source = match json {
                        Some(json) => implicit_subtree::SubtreeSource::Json(json),
                        None => implicit_subtree::SubtreeSource::Binary(&bytes),
                    };
                    Ok(Box::new(
                        Implicit3DTileContent::load(&loader.client, request.resource, source, implicit).await?,
                    ) as Box<dyn TilesetContent>)
                }
                _ => {
                    loader
                        .factory
                        .ok_or_else(|| {
                            RuntimeError::new(format!("No renderable content factory for {}", kind.as_str()))
                        })?
                        .create(request, kind, bytes)
                        .await
                }
            }
        })
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

pub use super::multiple_3d_tile_content::Multiple3DTileContent as MultipleTilesetContent;

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
pub(super) use super::multiple_3d_tile_content::request_multiple;

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
