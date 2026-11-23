//! Port of CesiumJS `Scene/I3SLayer`.

use crate::core::rectangle::Rectangle;
use crate::core::resource::{DerivedResourceOptions, Resource};
use crate::core::resource_client::ResourceClient;
use crate::core::runtime_error::RuntimeError;
use crate::scene::cesium_3d_tileset::{Cesium3DTileset, TilesetOptions};
use crate::scene::i3s_data_provider::I3SDataProvider;
use crate::scene::i3s_node::I3SNode;
use crate::scene::i3s_sublayer::I3SSublayer;
use crate::scene::i3s_symbology::I3SSymbology;
use crate::scene::scene::Result;
use serde_json::{json, Value};
use std::cell::{Ref, RefCell, RefMut};
use std::collections::HashMap;
use std::rc::{Rc, Weak};

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
            Self::Sublayer(sublayer) => sublayer.upgrade().expect("I3S layer parent sublayer").resource().expect("I3S layer parent sublayer resource").clone(),
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
    node_pages: RefCell<HashMap<usize, Vec<Value>>>,
    extent: Option<Rectangle>,
    tileset: RefCell<Option<Cesium3DTileset>>,
    geometry_definitions: Option<Vec<Vec<I3sGeometryBufferInfo>>>,
    filters: RefCell<Vec<Value>>,
    symbology: RefCell<Option<I3SSymbology>>,
}

impl I3SLayer {
    /// Port of `new I3SLayer(dataProvider, layerData, parent)`.
    pub fn new(data_provider: &Rc<I3SDataProvider>, parent: LayerParent, mut layer_data: Value) -> Result<Rc<I3SLayer>> {
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
            node_pages: RefCell::new(HashMap::new()),
            extent,
            tileset: RefCell::new(None),
            geometry_definitions: None,
            filters: RefCell::new(Vec::new()),
            symbology: RefCell::new(None),
        };
        layer.compute_geometry_definitions(true);
        let mut this = Rc::new(layer);
        let root_node = I3SNode::new_root(&this);
        Rc::get_mut(&mut this).expect("new layer").root_node = RefCell::new(Some(root_node));
        Ok(this)
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
        if let Some(root_node) = self.root_node_mut().as_mut() {
            root_node.load(client).await?;
        }
        self.create_3d_tileset(cesium3d_tileset_options)?;
        if self.legacy_version_16() == Some(true) {
            if let Some(root_node) = self.root_node_mut().as_mut() {
                root_node.load_children(client).await?;
            }
        }
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

    /// Port of `I3SLayer.prototype._loadNodePage`.
    pub async fn load_node_page(&self, page: usize, client: &ResourceClient) -> Result<Value> {
        if let Some(nodes) = self.node_pages.borrow().get(&page) {
            return Ok(Value::Array(nodes.clone()));
        }
        let node_page_resource = self
            .resource()
            .get_derived_resource(DerivedResourceOptions {
                url: Some(format!("nodepages/{page}/")),
                ..DerivedResourceOptions::default()
            })
            .expect("valid node page URI");
        let data = I3SDataProvider::load_json(&node_page_resource, client).await?;
        self.node_pages
            .borrow_mut()
            .insert(page, data.get("nodes").and_then(Value::as_array).cloned().unwrap_or_default());
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

    /// Port of `I3SLayer.prototype._create3DTileset`.
    fn create_3d_tileset(&self, cesium3d_tileset_options: Option<TilesetOptions>) -> Result<()> {
        let root_definition = self.root_node_mut().as_mut().and_then(|root| root.create_3d_tile_definition());
        let in_place_tileset = json!({
            "asset": { "version": "1.0" },
            "geometricError": f64::MAX,
            "root": root_definition,
        });
        let tileset = Cesium3DTileset::from_json(self.resource().clone(), &in_place_tileset, cesium3d_tileset_options.unwrap_or_default(), None)?;
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

    /// Returns the [`Ref`](std::cell::Ref) of the layer's root node, for
    /// callers that need to read it (e.g. [`I3SDataProvider::from_url`]).
    pub(crate) fn root_node_ref(&self) -> Ref<'_, Option<I3SNode>> {
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
