//! Port of CesiumJS `Scene/I3SLayer`.

use crate::core::cartesian3::Cartesian3;
use crate::core::color::Color;
use crate::core::ray::Ray;
use crate::core::rectangle::Rectangle;
use crate::core::resource::{DerivedResourceOptions, Resource};
use crate::core::resource_client::ResourceClient;
use crate::core::runtime_error::RuntimeError;
use crate::scene::cesium_3d_tile_content_type::Cesium3DTileContentType;
use crate::scene::cesium_3d_tile_style::Cesium3DTileStyle;
use crate::scene::cesium_3d_tileset::{
    Cesium3DTileset, TileContentFuture, TileContentMetrics, TileContentModelState, TileContentRequest, TilesetContent,
    TilesetContentLoader, TilesetOptions,
};
use crate::scene::empty_3d_tile_content::Empty3DTileContent;
use crate::scene::frame_state::FrameState;
use crate::scene::i3s_data_provider::I3SDataProvider;
use crate::scene::i3s_node::I3SNode;
use crate::scene::i3s_sublayer::I3SSublayer;
use crate::scene::i3s_symbology::I3SSymbology;
use crate::scene::scene::Result;
use crate::scene::tileset_content::{RenderableTileFactory, TileContentFailure, TileExpansion};
use serde_json::{json, Value};
use std::cell::{Ref, RefCell, RefMut};
use std::collections::HashMap;
use std::future::poll_fn;
use std::rc::{Rc, Weak};
use std::task::{Poll, Waker};

/// Parent of a top-level layer or of a sublayer-owned layer.
#[derive(Clone, Debug)]
pub enum LayerParent {
    Provider(Weak<I3SDataProvider>),
    Sublayer(Weak<I3SSublayer>),
}

impl LayerParent {
    /// `parent.resource`.
    pub fn resource(&self) -> Resource {
        match self {
            Self::Provider(provider) => provider.upgrade().expect("I3S layer parent provider").resource().clone(),
            Self::Sublayer(sublayer) => sublayer.upgrade().expect("I3S layer parent sublayer").resource().expect("I3S layer parent sublayer resource"),
        }
    }

    /// `parent.show`.
    pub fn show(&self) -> bool {
        match self {
            Self::Provider(provider) => provider.upgrade().is_some_and(|p| p.show()),
            Self::Sublayer(sublayer) => sublayer.upgrade().is_some_and(|s| s.show()),
        }
    }
}

/// Restores a temporarily taken root `I3SNode` back into `I3SLayer::root_node` on
/// drop so async operations never hold a `RefCell` borrow across `.await`, and
/// wakes any queued concurrent tile requests waiting for tree access.
struct RootNodeGuard {
    layer: Rc<I3SLayer>,
    node: Option<I3SNode>,
}

impl RootNodeGuard {
    async fn acquire(layer: Rc<I3SLayer>) -> Self {
        let node = poll_fn(|cx| {
            if let Some(node) = layer.root_node.borrow_mut().take() {
                Poll::Ready(node)
            } else {
                layer.root_node_waiters.borrow_mut().push(cx.waker().clone());
                Poll::Pending
            }
        })
        .await;
        Self { layer, node: Some(node) }
    }

    fn as_mut(&mut self) -> Option<&mut I3SNode> {
        self.node.as_mut()
    }
}

impl Drop for RootNodeGuard {
    fn drop(&mut self) {
        if let Some(node) = self.node.take() {
            *self.layer.root_node.borrow_mut() = Some(node);
            let waiters: Vec<Waker> = self.layer.root_node_waiters.borrow_mut().drain(..).collect();
            for waker in waiters {
                waker.wake();
            }
        }
    }
}

/// Decoded I3S tile content that holds optional delegated renderable content,
/// the decoded GLB payload, and child tile expansions discovered when a node
/// loads its children on demand.
pub struct I3STileContent {
    inner: Option<Box<dyn TilesetContent>>,
    glb_bytes: Option<Vec<u8>>,
    expansions: Vec<TileExpansion>,
    publish_target: Option<(Weak<I3SLayer>, Vec<usize>)>,
    destroyed: bool,
}

impl I3STileContent {
    pub fn glb_bytes(&self) -> Option<&[u8]> {
        self.glb_bytes.as_deref()
    }
}

impl TilesetContent for I3STileContent {
    fn configure_model(&mut self, state: &TileContentModelState) {
        if let Some(inner) = self.inner.as_mut() {
            inner.configure_model(state);
        }
    }

    fn apply_style(&mut self, style: Option<&Cesium3DTileStyle>) -> Result<()> {
        if let Some(inner) = self.inner.as_mut() {
            return inner.apply_style(style);
        }
        Ok(())
    }

    fn apply_debug_settings(&mut self, enabled: bool, color: Color) -> Result<()> {
        if let Some(inner) = self.inner.as_mut() {
            return inner.apply_debug_settings(enabled, color);
        }
        Ok(())
    }

    fn process_with_frame(&mut self, frame: &mut FrameState) -> Result<bool> {
        if let Some(inner) = self.inner.as_mut() {
            return inner.process_with_frame(frame);
        }
        Ok(!self.destroyed)
    }

    fn is_renderable(&self) -> bool {
        self.inner.as_ref().map_or_else(|| self.glb_bytes.is_some(), |inner| inner.is_renderable())
    }

    fn take_failures(&mut self) -> Vec<TileContentFailure> {
        self.inner.as_mut().map_or_else(Vec::new, |inner| inner.take_failures())
    }

    fn metrics(&self) -> TileContentMetrics {
        if let Some(inner) = self.inner.as_ref() {
            return inner.metrics();
        }
        TileContentMetrics {
            geometry_bytes: self.glb_bytes.as_ref().map_or(0, |b| b.len() as u64),
            ..TileContentMetrics::default()
        }
    }

    fn process(&mut self, frame: &FrameState) -> Result<bool> {
        if let Some(inner) = self.inner.as_mut() {
            return inner.process(frame);
        }
        Ok(!self.destroyed)
    }

    fn update(&mut self, frame: &mut FrameState) -> Result<()> {
        if let Some(inner) = self.inner.as_mut() {
            return inner.update(frame);
        }
        Ok(())
    }

    fn pick(&self, ray: &Ray, frame: &FrameState) -> Result<Option<Cartesian3>> {
        if let Some(inner) = self.inner.as_ref() {
            return inner.pick(ray, frame);
        }
        Ok(None)
    }

    fn destroy(&mut self) -> Result<()> {
        self.destroyed = true;
        self.expansions.clear();
        self.glb_bytes = None;
        if let Some(inner) = self.inner.as_mut() {
            inner.destroy()?;
        }
        Ok(())
    }

    fn texture_allocations(&self) -> Vec<(Option<String>, u64)> {
        self.inner
            .as_ref()
            .map_or_else(|| vec![(None, self.metrics().texture_bytes)], |inner| inner.texture_allocations())
    }

    fn take_expansions(&mut self) -> Vec<TileExpansion> {
        let mut out = std::mem::take(&mut self.expansions);
        if let Some((layer_weak, path)) = self.publish_target.take() {
            if let Some(layer) = layer_weak.upgrade() {
                if let Some(root) = layer.root_node_mut().as_mut() {
                    root.node_at_path_mut(&path).mark_children_published();
                }
            }
        }
        if let Some(inner) = self.inner.as_mut() {
            out.extend(inner.take_expansions());
        }
        out
    }
}

/// An I3S-aware [`TilesetContentLoader`]: resolves the matching `I3SNode` for
/// the requested tile URI, loads any unloaded child nodes as tile expansions,
/// decodes the node's I3S geometry to GLB, and delegates renderable content
/// creation to the provider's [`RenderableTileFactory`] when configured.
struct I3SLayerContentLoader {
    layer: Weak<I3SLayer>,
    client: ResourceClient,
    factory: Option<Rc<dyn RenderableTileFactory>>,
}

impl TilesetContentLoader for I3SLayerContentLoader {
    fn request(&self, request: TileContentRequest) -> Option<TileContentFuture> {
        let layer = self.layer.upgrade()?;
        if layer.root_node.borrow().is_none() {
            // Defer scheduling while another in-flight request holds the node tree.
            return None;
        }
        let client = self.client.clone();
        let factory = self.factory.clone();
        Some(Box::pin(async move {
            if request.cancelled.get() {
                return Err(RuntimeError::new("tile request cancelled"));
            }
            let target_url = request.resource.url();
            let mut root_guard = RootNodeGuard::acquire(Rc::clone(&layer)).await;
            if request.cancelled.get() {
                return Err(RuntimeError::new("tile request cancelled"));
            }
            let Some(root_node) = root_guard.as_mut() else {
                return Ok(Box::new(Empty3DTileContent::new()) as Box<dyn TilesetContent>);
            };
            let target_path = root_node.find_node_path_by_url(&target_url).unwrap_or_default();
            let target_node = root_node.node_at_path_mut(&target_path);
            let mut expansions = Vec::new();
            let mut should_mark_published = false;
            if !target_node.children_ready() {
                target_node.load_children(&client).await?;
            }
            if !target_node.children_published() {
                for child in target_node.children_mut() {
                    if let Some(child_def) = child.create_3d_tile_definition() {
                        expansions.push(TileExpansion::External {
                            resource: layer.resource().clone(),
                            json: json!({
                                "asset": { "version": "1.0" },
                                "root": child_def,
                            }),
                        });
                    }
                }
                should_mark_published = true;
            }
            let glb = target_node.create_content_url(&client).await?;
            drop(root_guard);
            if request.cancelled.get() {
                return Err(RuntimeError::new("tile request cancelled"));
            }
            let inner = match (factory, glb.clone()) {
                (Some(factory), Some(glb_bytes)) => {
                    Some(factory.create(request, Cesium3DTileContentType::GltfBinary, glb_bytes).await?)
                }
                _ => None,
            };
            let publish_target = should_mark_published.then(|| (Rc::downgrade(&layer), target_path));
            Ok(Box::new(I3STileContent {
                inner,
                glb_bytes: glb,
                expansions,
                publish_target,
                destroyed: false,
            }) as Box<dyn TilesetContent>)
        }))
    }
}

/// Port of `I3SLayerGeometryBufferInfo`: the info of one geometry buffer of
/// a geometry definition rank.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct I3sGeometryBufferInfo {
    pub compressed: bool,
    pub attributes: Vec<String>,
    pub index: usize,
}

/// Port of `I3SLayer`: an I3S layer. In CesiumJS each layer creates a
/// `Cesium3DTileset`.
pub struct I3SLayer {
    pub data_provider: Weak<I3SDataProvider>,
    parent: LayerParent,
    version: String,
    major_version: f64,
    minor_version: f64,
    resource: Resource,
    data: Value,
    root_node: RefCell<Option<I3SNode>>,
    root_node_waiters: RefCell<Vec<Waker>>,
    node_pages: RefCell<HashMap<usize, Value>>,
    extent: Option<Rectangle>,
    tileset: RefCell<Option<Cesium3DTileset>>,
    geometry_definitions: Option<Vec<Vec<I3sGeometryBufferInfo>>>,
    filters: RefCell<Vec<Value>>,
    symbology: RefCell<Option<I3SSymbology>>,
    weak_self: RefCell<Option<Weak<I3SLayer>>>,
    client: Option<ResourceClient>,
}

impl I3SLayer {
    /// Port of `new I3SLayer(dataProvider, layerData, parent)`.
    pub fn new(data_provider: &Rc<I3SDataProvider>, parent: LayerParent, mut layer_data: Value, client: Option<ResourceClient>) -> Result<Rc<I3SLayer>> {
        if layer_data.get("href").is_none() && layer_data.get("id").is_some() {
            let id = layer_data.get("id").map(Value::to_string).unwrap_or_default();
            layer_data.as_object_mut().expect("I3S layer data is an object").insert("href".to_string(), Value::String(format!("layers/{id}")));
        }
        let parent_url = parent.resource().get_url_component(false, false);
        let href = layer_data.get("href").and_then(Value::as_str).unwrap_or_default().to_string();
        let tileset_url = if is_layers_url(&parent_url) {
            parent_url.trim_end_matches('/').to_string()
        } else {
            let base = if parent_url.ends_with('/') { parent_url } else { format!("{parent_url}/") };
            format!("{base}{href}")
        };
        let version = layer_data.get("store").and_then(|s| s.get("version")).and_then(Value::as_str).unwrap_or_default().to_string();
        let split_version: Vec<&str> = version.split('.').collect();
        let major = split_version.first().and_then(|s| s.parse::<f64>().ok()).unwrap_or_default();
        let minor = split_version.get(1).and_then(|s| s.parse::<f64>().ok()).unwrap_or_default();

        let resource = Resource::new(tileset_url).expect("valid tileset URL");
        resource.set_query_parameters(&data_provider.resource().options().query_parameters.clone(), false);
        resource.append_forward_slash();

        let mut extent = None;
        if let Some(full_extent) = layer_data.get("fullExtent") {
            extent = Some(Rectangle::from_degrees(
                full_extent.get("xmin").and_then(Value::as_f64).unwrap_or_default(),
                full_extent.get("ymin").and_then(Value::as_f64).unwrap_or_default(),
                full_extent.get("xmax").and_then(Value::as_f64).unwrap_or_default(),
                full_extent.get("ymax").and_then(Value::as_f64).unwrap_or_default(),
            ));
        } else if let Some(store_extent) = layer_data.get("store").and_then(|s| s.get("extent")).and_then(Value::as_array) {
            extent = Some(Rectangle::from_degrees(
                store_extent.first().and_then(Value::as_f64).unwrap_or_default(),
                store_extent.get(1).and_then(Value::as_f64).unwrap_or_default(),
                store_extent.get(2).and_then(Value::as_f64).unwrap_or_default(),
                store_extent.get(3).and_then(Value::as_f64).unwrap_or_default(),
            ));
        }

        let mut layer = I3SLayer {
            data_provider: Rc::downgrade(data_provider),
            parent,
            version,
            major_version: major,
            minor_version: minor,
            resource,
            data: layer_data,
            root_node: RefCell::new(None),
            root_node_waiters: RefCell::new(Vec::new()),
            node_pages: RefCell::new(HashMap::new()),
            client,
            extent,
            tileset: RefCell::new(None),
            geometry_definitions: None,
            filters: RefCell::new(Vec::new()),
            symbology: RefCell::new(None),
            weak_self: RefCell::new(None),
        };
        layer.compute_geometry_definitions(true);
        let this = Rc::new(layer);
        this.root_node.replace(Some(I3SNode::new_root(&this)));
        *this.weak_self.borrow_mut() = Some(Rc::downgrade(&this));
        Ok(this)
    }

    /// The layer's [`Weak`] (the content loader holds one).
    pub(crate) fn weak_self(&self) -> Option<Weak<I3SLayer>> {
        self.weak_self.borrow().clone()
    }

    /// Port of the `resource` getter.
    pub fn resource(&self) -> &Resource {
        &self.resource
    }

    /// Port of the `rootNode` getter (mutating).
    pub fn root_node_mut(&self) -> RefMut<'_, Option<I3SNode>> {
        self.root_node.borrow_mut()
    }

    /// Port of the `tileset` getter (mutating).
    pub fn tileset_mut(&self) -> RefMut<'_, Option<Cesium3DTileset>> {
        self.tileset.borrow_mut()
    }

    /// Port of the `data` getter.
    pub fn data(&self) -> &Value {
        &self.data
    }

    /// Port of the `version` getter.
    pub fn version(&self) -> &str {
        &self.version
    }

    /// Port of the `majorVersion` getter.
    pub fn major_version(&self) -> f64 {
        self.major_version
    }

    /// Port of the `minorVersion` getter.
    pub fn minor_version(&self) -> f64 {
        self.minor_version
    }

    /// Port of the `legacyVersion16` getter.
    pub fn legacy_version_16(&self) -> Option<bool> {
        Some(self.major_version < 1.0 || (self.major_version == 1.0 && self.minor_version <= 6.0))
    }

    /// Port of the `symbology` getter.
    pub fn symbology(&self) -> Option<I3SSymbology> {
        self.symbology.borrow().clone()
    }

    /// Port of the `extent` getter.
    pub fn extent(&self) -> Option<&Rectangle> {
        self.extent.as_ref()
    }

    /// The geometry definition ranks, computed in [`I3SLayer::new`].
    pub fn geometry_definitions(&self) -> Option<Vec<Vec<I3sGeometryBufferInfo>>> {
        self.geometry_definitions.clone()
    }

    /// Port of `I3SLayer.prototype.load`.
    pub async fn load(&self, cesium3d_tileset_options: Option<TilesetOptions>, client: &ResourceClient) -> Result<()> {
        let wkid = self.data.get("spatialReference").and_then(|r| r.get("wkid")).and_then(Value::as_u64).unwrap_or_default();
        if wkid != 4326 {
            return Err(RuntimeError::new(format!("Unsupported spatial reference: {wkid}")));
        }
        if self.data_provider.upgrade().is_some_and(|provider| provider.apply_symbology()) {
            *self.symbology.borrow_mut() = Some(I3SSymbology::new(self));
        }
        if let Some(provider) = self.data_provider.upgrade() {
            provider.load_geoid_data().await;
        }
        if let Some(this) = self.weak_self().and_then(|w| w.upgrade()) {
            let mut root_guard = RootNodeGuard::acquire(this).await;
            if let Some(root_node) = root_guard.as_mut() {
                root_node.load(client).await?;
                if self.legacy_version_16() == Some(true) {
                    root_node.load_children(client).await?;
                }
            }
            drop(root_guard);
        }
        self.create_3d_tileset(cesium3d_tileset_options)?;
        Ok(())
    }

    /// Port of `I3SLayer.prototype._computeGeometryDefinitions`.
    fn compute_geometry_definitions(&mut self, use_compression: bool) {
        self.geometry_definitions = None;
        if let Some(definitions) = self.data.get("geometryDefinitions").and_then(Value::as_array) {
            let mut defs: Vec<Vec<I3sGeometryBufferInfo>> = Vec::new();
            for definition in definitions {
                let Some(buffers) = definition.get("geometryBuffers").and_then(Value::as_array) else { continue };
                let mut geometry_buffers_info: Vec<I3sGeometryBufferInfo> = Vec::new();
                for (buf_index, geometry_buffer) in buffers.iter().enumerate() {
                    let mut collected_attributes: Vec<String> = Vec::new();
                    let mut compressed = false;
                    if let Some(compressed_attributes) = geometry_buffer.get("compressedAttributes") {
                        if use_compression {
                            compressed = true;
                            if let Some(attributes) = compressed_attributes.get("attributes").and_then(Value::as_array) {
                                collected_attributes.extend(attributes.iter().filter_map(Value::as_str).map(str::to_owned));
                            }
                        } else {
                            for key in geometry_buffer.as_object().cloned().unwrap_or_default().keys() {
                                if key != "offset" {
                                    collected_attributes.push(key.clone());
                                }
                            }
                        }
                    } else {
                        for key in geometry_buffer.as_object().cloned().unwrap_or_default().keys() {
                            if key != "offset" {
                                collected_attributes.push(key.clone());
                            }
                        }
                    }
                    geometry_buffers_info.push(I3sGeometryBufferInfo { compressed, attributes: collected_attributes, index: buf_index });
                }
                // Rank: compressed first, then fewer attributes.
                geometry_buffers_info.sort_by(|a, b| {
                    if a.compressed && !b.compressed {
                        std::cmp::Ordering::Less
                    } else if !a.compressed && b.compressed {
                        std::cmp::Ordering::Greater
                    } else {
                        a.attributes.len().cmp(&b.attributes.len())
                    }
                });
                defs.push(geometry_buffers_info);
            }
            self.geometry_definitions = Some(defs);
        }
    }

    /// Port of `I3SLayer.prototype._findBestGeometryBuffers`.
    pub fn find_best_geometry_buffers(&self, definition: usize, attributes: &[&str]) -> Option<I3sGeometryBufferInfo> {
        let geometry_definition = self.geometry_definitions.as_ref()?.get(definition)?;
        for geometry_buffer_info in geometry_definition {
            if attributes.iter().all(|attr| geometry_buffer_info.attributes.iter().any(|a| a == attr)) {
                return Some(geometry_buffer_info.clone());
            }
        }
        // If no match found return the first geometry buffer.
        geometry_definition.first().cloned()
    }

    /// Port of `I3SLayer.prototype._getNodeInNodePages`.
    pub async fn get_node_in_node_pages(&self, node_index: usize, client: &ResourceClient) -> Result<Value> {
        let nodes_per_page = self
            .data
            .get("nodePages")
            .and_then(|np| np.get("nodesPerPage"))
            .and_then(Value::as_u64)
            .unwrap_or(1)
            .max(1) as usize;
        let index = node_index / nodes_per_page;
        let offset_in_page = node_index % nodes_per_page;
        let data = self.load_node_page(index, client).await?;
        Ok(data.get("nodes").and_then(Value::as_array).and_then(|nodes| nodes.get(offset_in_page)).cloned().unwrap_or(Value::Null))
    }

    /// Port of `I3SLayer.prototype._loadNodePage`: returns the page object,
    /// cached under the same shape the fetch produces.
    pub async fn load_node_page(&self, page: usize, client: &ResourceClient) -> Result<Value> {
        if let Some(data) = self.node_pages.borrow().get(&page) {
            return Ok(data.clone());
        }
        let node_page_resource = self
            .resource()
            .get_derived_resource(DerivedResourceOptions {
                url: Some(format!("nodepages/{page}/")),
                ..DerivedResourceOptions::default()
            })
            .expect("valid node page URI");
        let data = I3SDataProvider::load_json(&node_page_resource, client).await?;
        self.node_pages.borrow_mut().insert(page, data.clone());
        Ok(data)
    }

    /// Port of `I3SLayer.prototype._computeExtent`.
    pub fn compute_extent(&mut self) {
        self.extent = None;
        if let Some(full_extent) = self.data.get("fullExtent") {
            self.extent = Some(Rectangle::from_degrees(
                full_extent.get("xmin").and_then(Value::as_f64).unwrap_or_default(),
                full_extent.get("ymin").and_then(Value::as_f64).unwrap_or_default(),
                full_extent.get("xmax").and_then(Value::as_f64).unwrap_or_default(),
                full_extent.get("ymax").and_then(Value::as_f64).unwrap_or_default(),
            ));
        } else if let Some(store_extent) = self.data.get("store").and_then(|s| s.get("extent")).and_then(Value::as_array) {
            self.extent = Some(Rectangle::from_degrees(
                store_extent.first().and_then(Value::as_f64).unwrap_or_default(),
                store_extent.get(1).and_then(Value::as_f64).unwrap_or_default(),
                store_extent.get(2).and_then(Value::as_f64).unwrap_or_default(),
                store_extent.get(3).and_then(Value::as_f64).unwrap_or_default(),
            ));
        }
    }

    /// An I3S-aware [`TilesetContentLoader`] for `create_3d_tileset`.
    fn client(&self) -> Option<ResourceClient> {
        self.client.clone()
    }

    /// Port of `I3SLayer.prototype._create3DTileset`, wiring an I3S-aware
    /// [`TilesetContentLoader`] into `Cesium3DTileset::from_json`.
    fn create_3d_tileset(&self, cesium3d_tileset_options: Option<TilesetOptions>) -> Result<()> {
        let root_definition = self.root_node_mut().as_mut().and_then(|root| root.create_3d_tile_definition());
        let in_place_tileset = json!({
            "asset": { "version": "1.0" },
            "geometricError": f64::MAX,
            "root": root_definition,
        });
        let factory = self.data_provider.upgrade().and_then(|p| p.content_factory());
        let loader = self
            .weak_self()
            .zip(self.client())
            .map(|(layer, client)| Rc::new(I3SLayerContentLoader { layer, client, factory }) as Rc<dyn TilesetContentLoader>);
        let tileset = Cesium3DTileset::from_json(self.resource().clone(), &in_place_tileset, cesium3d_tileset_options.unwrap_or_default(), loader)?;
        *self.tileset.borrow_mut() = Some(tileset);
        if let Some(tileset) = self.tileset_mut().as_mut() {
            tileset.options.show = self.parent.show();
        }
        Ok(())
    }

    /// Port of `I3SLayer.prototype._updateVisibility`.
    pub fn update_visibility(&self) {
        if let Some(tileset) = self.tileset_mut().as_mut() {
            tileset.options.show = self.parent.show();
        }
    }

    /// Port of `I3SLayer.prototype.filterByAttributes`.
    pub async fn filter_by_attributes(&self, filters: Option<Vec<Value>>, _client: &ResourceClient) -> Result<()> {
        *self.filters.borrow_mut() = filters.unwrap_or_default();
        if let Some(root_node) = self.root_node_mut().as_mut() {
            root_node.filter_features()?;
        }
        Ok(())
    }

    /// The filters set by [`I3SLayer::filter_by_attributes`].
    pub fn filters(&self) -> Ref<'_, Vec<Value>> {
        self.filters.borrow()
    }

    /// Returns the [`Ref`](std::cell::Ref) of the layer's root node.
    pub fn root_node(&self) -> Ref<'_, Option<I3SNode>> {
        self.root_node.borrow()
    }
}

/// `/layers\/\d/` test on a URL.
fn is_layers_url(url: &str) -> bool {
    let mut rest = url;
    while let Some(idx) = rest.find("layers/") {
        rest = &rest[idx + "layers/".len()..];
        if rest.starts_with(|c: char| c.is_ascii_digit()) {
            return true;
        }
    }
    false
}
