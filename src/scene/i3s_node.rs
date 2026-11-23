//! Port of CesiumJS `Scene/I3SNode`.

use crate::core::cartesian3::Cartesian3;
use crate::core::cartographic::Cartographic;
use crate::core::ellipsoid::Ellipsoid;
use crate::core::heading_pitch_roll::HeadingPitchRoll;
use crate::core::matrix3::Matrix3;
use crate::core::matrix4::Matrix4;
use crate::core::quaternion::Quaternion;
use crate::core::resource::{DerivedResourceOptions, Resource};
use crate::core::resource_client::ResourceClient;
use crate::core::transforms::{LocalFrame, Transforms};
use crate::scene::i3s_data_provider::I3SDataProvider;
use crate::scene::i3s_decoder;
use crate::scene::i3s_feature::I3SFeature;
use crate::scene::i3s_field::I3SField;
use crate::scene::i3s_geometry::I3SGeometry;
use crate::scene::i3s_layer::I3SLayer;
use crate::scene::scene::Result;
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use std::rc::{Rc, Weak};

/// Port of `I3SNode`: an I3S node. In CesiumJS each I3SNode creates a
/// `Cesium3DTile`. The Rust port stores the parent link through `Weak`
/// instead of the shared JS reference to avoid a reference cycle.
pub struct I3SNode {
    layer: Weak<I3SLayer>,
    parent: Option<Weak<I3SNode>>,
    /// `parent._globalTransform`, copied at construction (see
    /// [`I3SNode::create_3d_tile_definition`]).
    parent_global_transform: Option<Matrix4>,
    /// `parent._inverseGlobalTransform`, copied at construction.
    parent_inverse_global_transform: Option<Matrix4>,
    data_provider: Weak<I3SDataProvider>,
    is_root: bool,
    level: usize,
    node_index: Option<usize>,
    resource: Option<Resource>,
    data: Option<Value>,
    geometry_data: Vec<I3SGeometry>,
    feature_data: Vec<I3SFeature>,
    fields: BTreeMap<String, I3SField>,
    children: Vec<I3SNode>,
    global_transform: Option<Matrix4>,
    inverse_global_transform: Option<Matrix4>,
    inverse_rotation_matrix: Option<Matrix3>,
    symbology_data: Option<Value>,
    is_loading: bool,
    children_ready: bool,
}

impl I3SNode {
    /// Port of `new I3SNode(parent, ref, isRoot)` for the root node: `ref`
    /// is the layer's root node reference (a number with node pages,
    /// otherwise `store.rootNode`).
    pub fn new_root(layer: &Rc<I3SLayer>) -> Self {
        if layer.data().get("nodePages").is_some() {
            let root_index = layer
                .data()
                .get("nodePages")
                .and_then(|np| np.get("rootIndex"))
                .and_then(Value::as_u64)
                .unwrap_or(0) as usize;
            Self::new_internal(layer, Some(root_index), None)
        } else {
            let root_node = layer
                .data()
                .get("store")
                .and_then(|s| s.get("rootNode"))
                .and_then(Value::as_str)
                .unwrap_or_default()
                .to_string();
            let resource = layer.resource().get_derived_resource(DerivedResourceOptions {
                url: Some(format!("{root_node}/")),
                ..DerivedResourceOptions::default()
            });
            Self::new_internal(layer, None, resource)
        }
    }

    /// Port of `new I3SNode(parent, ref, false)`.
    pub fn new_child(parent: &I3SNode, r#ref: Value) -> Self {
        let resource = parent.resource().and_then(|resource| {
            resource.get_derived_resource(DerivedResourceOptions {
                url: Some(format!("{}/", r#ref.as_str().unwrap_or_default())),
                ..DerivedResourceOptions::default()
            })
        });
        Self {
            layer: parent.layer.clone(),
            parent: None,
            parent_global_transform: parent.global_transform.clone(),
            parent_inverse_global_transform: parent.inverse_global_transform.clone(),
            data_provider: parent.data_provider.clone(),
            is_root: false,
            level: parent.level + 1,
            node_index: None,
            resource,
            data: None,
            geometry_data: Vec::new(),
            feature_data: Vec::new(),
            fields: BTreeMap::new(),
            children: Vec::new(),
            global_transform: None,
            inverse_global_transform: None,
            inverse_rotation_matrix: None,
            symbology_data: None,
            is_loading: false,
            children_ready: false,
        }
    }

    fn new_internal(layer: &Rc<I3SLayer>, node_index: Option<usize>, resource: Option<Resource>) -> Self {
        Self {
            layer: Rc::downgrade(layer),
            parent: None,
            parent_global_transform: None,
            parent_inverse_global_transform: None,
            data_provider: layer.data_provider.clone(),
            is_root: true,
            level: 0,
            node_index,
            resource,
            data: None,
            geometry_data: Vec::new(),
            feature_data: Vec::new(),
            fields: BTreeMap::new(),
            children: Vec::new(),
            global_transform: None,
            inverse_global_transform: None,
            inverse_rotation_matrix: None,
            symbology_data: None,
            is_loading: false,
            children_ready: false,
        }
    }

    /// Port of the `resource` getter.
    pub fn resource(&self) -> Option<&Resource> {
        self.resource.as_ref()
    }

    /// Port of the `layer` getter.
    pub fn layer(&self) -> Option<Rc<I3SLayer>> {
        self.layer.upgrade()
    }

    /// Port of the `children` getter.
    pub fn children(&self) -> &[I3SNode] {
        &self.children
    }

    /// Port of the `geometryData` getter.
    pub fn geometry_data(&self) -> &[I3SGeometry] {
        &self.geometry_data
    }

    /// Port of the `featureData` getter.
    pub fn feature_data(&self) -> &[I3SFeature] {
        &self.feature_data
    }

    /// Port of the `fields` getter.
    pub fn fields(&self) -> &BTreeMap<String, I3SField> {
        &self.fields
    }

    /// Port of the `data` getter.
    pub fn data(&self) -> Option<&Value> {
        self.data.as_ref()
    }

    /// Port of the `dataProvider` getter.
    pub fn data_provider(&self) -> Option<Rc<I3SDataProvider>> {
        self.data_provider.upgrade()
    }

    /// Port of `I3SNode._inverseRotationMatrix`.
    pub fn inverse_rotation_matrix(&self) -> Option<Matrix3> {
        self.inverse_rotation_matrix
    }

    /// Port of the `isRoot` getter.
    pub fn is_root(&self) -> bool {
        self.is_root
    }

    /// Port of the `nodeIndex` getter.
    pub fn node_index(&self) -> Option<usize> {
        self.node_index
    }

    /// Port of `I3SNode.prototype.load`.
    pub async fn load(&mut self, client: &ResourceClient) -> Result<()> {
        if self.node_index.is_none() {
            let resource = self.resource.clone().expect("I3SNode root resource");
            let data = I3SDataProvider::load_json(&resource, client).await?;
            self.data = Some(data);
            return Ok(());
        }
        let Some(layer) = self.layer() else { return Ok(()) };
        let node_index = self.node_index.expect("I3SNode node index");
        let node = layer.get_node_in_node_pages(node_index, client).await?;
        self.data = Some(node);
        let mut uri: Option<String> = None;
        if self.is_root {
            uri = Some("nodes/root/".to_string());
        } else if let Some(mesh) = self.data.as_ref().and_then(|d| d.get("mesh")) {
            if let Some(resource_index) = mesh.get("geometry").and_then(|g| g.get("resource")).and_then(Value::as_str) {
                uri = Some(format!("../{resource_index}/"));
            }
        }
        if let Some(uri) = uri {
            let base = self
                .parent
                .as_ref()
                .and_then(|parent| parent.upgrade())
                .and_then(|parent| parent.resource().cloned())
                .or_else(|| self.layer().map(|layer| layer.resource().clone()));
            if let Some(base) = base {
                self.resource = base.get_derived_resource(DerivedResourceOptions {
                    url: Some(uri),
                    ..DerivedResourceOptions::default()
                });
            }
        }
        Ok(())
    }

    /// Port of `I3SNode.prototype.loadFields`.
    pub async fn load_fields(&mut self, client: &ResourceClient) -> Result<()> {
        let Some(layer) = self.layer() else { return Ok(()) };
        let fields = layer.data().get("attributeStorageInfo").and_then(Value::as_array).cloned().unwrap_or_default();
        for storage_info in fields {
            let Some(name) = storage_info.get("name").and_then(Value::as_str).map(str::to_owned) else { continue };
            if !self.fields.contains_key(&name) {
                let field = I3SField::new(self, storage_info);
                self.fields.insert(name.clone(), field);
            }
            if let Some(field) = self.fields.get_mut(&name) {
                let _ = field.load(client).await;
            }
        }
        Ok(())
    }

    /// Port of `I3SNode.prototype.loadField`.
    pub async fn load_field(&mut self, name: &str, client: &ResourceClient) -> Result<()> {
        if let Some(field) = self.fields.get_mut(name) {
            let _ = field.load(client).await;
            return Ok(());
        }
        let Some(layer) = self.layer() else { return Ok(()) };
        let fields = layer.data().get("attributeStorageInfo").and_then(Value::as_array).cloned().unwrap_or_default();
        for storage_info in fields {
            if storage_info.get("name").and_then(Value::as_str) == Some(name) {
                let field = I3SField::new(self, storage_info);
                self.fields.insert(name.to_string(), field);
                if let Some(field) = self.fields.get_mut(name) {
                    let _ = field.load(client).await;
                }
                return Ok(());
            }
        }
        Ok(())
    }

    /// Port of `I3SNode.prototype.getFieldsForPickedPosition`.
    pub fn get_fields_for_picked_position(&self, picked_position: &Cartesian3) -> BTreeMap<String, Value> {
        let Some(geometry) = self.geometry_data.first() else { return BTreeMap::new() };
        let Some(feature_index) = geometry.custom_attributes().and_then(|a| a.feature_index.as_ref()) else { return BTreeMap::new() };
        let location = geometry.get_closest_point_index_on_triangle(picked_position.x, picked_position.y, picked_position.z);
        if location.index == -1 || (location.index as usize) > feature_index.len() {
            return BTreeMap::new();
        }
        let feature_index = feature_index.get(location.index as usize).copied().flatten().unwrap_or(0.0);
        self.get_fields_for_feature(feature_index)
    }

    /// Port of `I3SNode.prototype.getFieldsForFeature`.
    pub fn get_fields_for_feature(&self, feature_index: f64) -> BTreeMap<String, Value> {
        let mut feature_fields = BTreeMap::new();
        for (field_name, field) in &self.fields {
            let values = field.values();
            if feature_index >= 0.0 && feature_index < values.len() as f64 {
                feature_fields.insert(field.name().unwrap_or(field_name).to_string(), values[feature_index as usize].clone());
            }
        }
        feature_fields
    }

    /// Port of `I3SNode.prototype._loadChildren`.
    pub async fn load_children(&mut self, client: &ResourceClient) -> Result<()> {
        if self.children_ready {
            return Ok(());
        }
        if let Some(children) = self.data.as_ref().and_then(|d| d.get("children")).and_then(Value::as_array).cloned() {
            for child in &children {
                let r#ref = child.get("href").cloned().unwrap_or_else(|| child.clone());
                let new_child = I3SNode::new_child(self, r#ref);
                self.children.push(new_child);
            }
        }
        self.children_ready = true;
        for child in &mut self.children {
            child.load(client).await?;
        }
        Ok(())
    }

    /// Port of `I3SNode.prototype._loadGeometryData`.
    pub async fn load_geometry_data(&mut self, client: &ResourceClient) -> Result<()> {
        let hrefs: Vec<String> = self
            .data
            .as_ref()
            .and_then(|d| d.get("geometryData"))
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(|g| g.get("href").and_then(Value::as_str).map(str::to_owned)).collect())
            .unwrap_or_default();
        for href in hrefs {
            let mut geometry = I3SGeometry::new(self, &href);
            geometry.load(client).await?;
            self.geometry_data.push(geometry);
        }
        if !self.geometry_data.is_empty() {
            return Ok(());
        }
        let Some(mesh) = self.data.as_ref().and_then(|d| d.get("mesh")).cloned() else { return Ok(()) };
        let Some(layer) = self.layer() else { return Ok(()) };
        let Some(definition_index) = mesh.get("geometry").and_then(|g| g.get("definition")).and_then(Value::as_u64) else { return Ok(()) };
        let Some(geometry_definition) = layer.find_best_geometry_buffers(definition_index as usize, &["position", "uv0"]) else { return Ok(()) };
        let geometry_uri = format!("./geometries/{}/", geometry_definition.index);
        let mut new_geometry_data = I3SGeometry::new(self, &geometry_uri);
        let geometry_definitions = layer.geometry_definitions();
        new_geometry_data.geometry_definitions = geometry_definitions.as_ref().and_then(|defs| defs.get(definition_index as usize)).cloned();
        new_geometry_data.geometry_buffer_info = Some(geometry_definition);
        if let Some(geometry_data) = self.geometry_data.last_mut() {
            geometry_data.load(client).await?;
        } else {
            new_geometry_data.load(client).await?;
            self.geometry_data.push(new_geometry_data);
        }
        Ok(())
    }

    /// Port of `I3SNode.prototype._loadFeatureData`.
    pub async fn load_feature_data(&mut self, client: &ResourceClient) -> Result<()> {
        let hrefs: Vec<String> = self
            .data
            .as_ref()
            .and_then(|d| d.get("featureData"))
            .and_then(Value::as_array)
            .map(|a| a.iter().filter_map(|f| f.get("href").and_then(Value::as_str).map(str::to_owned)).collect())
            .unwrap_or_default();
        for href in hrefs {
            let feature = I3SFeature::new(self, &href);
            self.feature_data.push(feature);
            if let Some(feature) = self.feature_data.last_mut() {
                feature.load(client).await?;
            }
        }
        Ok(())
    }

    /// Port of `I3SNode.prototype._clearGeometryData`.
    pub fn clear_geometry_data(&mut self) {
        self.geometry_data.clear();
    }

    /// Port of `I3SNode.prototype._loadSymbology`.
    pub async fn load_symbology(&mut self, client: &ResourceClient) -> Result<()> {
        let Some(layer) = self.layer() else { return Ok(()) };
        if self.symbology_data.is_none() {
            if let Some(symbology) = layer.symbology() {
                self.symbology_data = Some(symbology.get_symbology(self, client).await?);
            }
        }
        Ok(())
    }

    /// Port of `I3SNode.prototype._create3DTileDefinition`.
    pub fn create_3d_tile_definition(&mut self) -> Option<Value> {
        let obb = self.data.as_ref()?.get("obb").cloned();
        let mbs = self.data.as_ref()?.get("mbs").cloned();
        let mut geo_position = if let Some(obb) = &obb {
            let center = obb.get("center").and_then(Value::as_array)?;
            Cartographic::from_degrees(
                center.first().and_then(Value::as_f64).unwrap_or_default(),
                center.get(1).and_then(Value::as_f64).unwrap_or_default(),
                center.get(2).and_then(Value::as_f64).unwrap_or_default(),
            )
        } else {
            let mbs = mbs.as_ref()?.as_array()?;
            Cartographic::from_degrees(
                mbs.first().and_then(Value::as_f64).unwrap_or_default(),
                mbs.get(1).and_then(Value::as_f64).unwrap_or_default(),
                mbs.get(2).and_then(Value::as_f64).unwrap_or_default(),
            )
        };

        // Offset bounding box position if we have a geoid service defined.
        if let Some(geoid_data_list) = self.data_provider().and_then(|p| p.geoid_data_list()) {
            for tile in geoid_data_list {
                let projected = tile.project(&geo_position);
                if projected.x > tile.native_extent.west
                    && projected.x < tile.native_extent.east
                    && projected.y > tile.native_extent.south
                    && projected.y < tile.native_extent.north
                {
                    geo_position.height += tile.sample(projected.x, projected.y);
                    break;
                }
            }
        }

        let mut bounding_volume = Map::new();
        let mut span = 0.0;
        let half_size = obb.as_ref().and_then(|obb| obb.get("halfSize")).and_then(Value::as_array).cloned();
        let hs = |i: usize| half_size.as_ref().and_then(|a| a.get(i)).and_then(Value::as_f64).unwrap_or_default();
        let mbs3 = mbs.as_ref().and_then(|m| m.as_array()).and_then(|a| a.get(3)).and_then(Value::as_f64).unwrap_or_default();
        let position: Cartesian3;
        if let Some(_obb) = &obb {
            bounding_volume.insert("box".to_string(), json!([0.0, 0.0, 0.0, hs(0), 0.0, 0.0, 0.0, hs(1), 0.0, 0.0, 0.0, hs(2)]));
            span = hs(0).max(hs(1)).max(hs(2));
            position = Ellipsoid::WGS84.cartographic_to_cartesian(&geo_position);
        } else {
            bounding_volume.insert("sphere".to_string(), json!([0.0, 0.0, 0.0, mbs3]));
            position = Ellipsoid::WGS84.cartographic_to_cartesian(&geo_position);
            span = mbs3;
        }
        span *= 2.0;

        // Compute the geometric error: meters/pixel density required to pop the next LOD.
        let mut meters_per_pixel = f64::INFINITY;
        if let Some(lod_threshold) = self.data.as_ref()?.get("lodThreshold").and_then(Value::as_f64) {
            let metric_type = self
                .layer()
                .and_then(|l| l.data().get("nodePages").and_then(|np| np.get("lodSelectionMetricType")).and_then(Value::as_str).map(str::to_owned));
            if metric_type.as_deref() == Some("maxScreenThresholdSQ") {
                let max_screen_threshold = (lod_threshold / (std::f64::consts::PI * 0.25)).sqrt();
                meters_per_pixel = span / max_screen_threshold;
            } else if metric_type.as_deref() == Some("maxScreenThreshold") {
                meters_per_pixel = span / lod_threshold;
            } else {
                eprintln!("Invalid lodSelectionMetricType in Layer");
            }
        } else if let Some(lod_selection) = self.data.as_ref()?.get("lodSelection").and_then(Value::as_array) {
            for lod in lod_selection {
                if lod.get("metricType").and_then(Value::as_str) == Some("maxScreenThreshold") {
                    meters_per_pixel = span / lod.get("maxError").and_then(Value::as_f64).unwrap_or_default();
                }
            }
        }
        if meters_per_pixel == f64::INFINITY {
            meters_per_pixel = 100000.0;
        }
        let geometric_error = meters_per_pixel * 16.0;

        // Transformations.
        let hpr = HeadingPitchRoll::new(0.0, 0.0, 0.0);
        let mut orientation = Transforms::heading_pitch_roll_quaternion(position, &hpr, &Ellipsoid::WGS84, LocalFrame::EAST_NORTH_UP);
        if let Some(obb) = &obb {
            orientation = Quaternion::new(
                obb.get("quaternion").and_then(|q| q.get(0)).and_then(Value::as_f64).unwrap_or_default(),
                obb.get("quaternion").and_then(|q| q.get(1)).and_then(Value::as_f64).unwrap_or_default(),
                obb.get("quaternion").and_then(|q| q.get(2)).and_then(Value::as_f64).unwrap_or_default(),
                obb.get("quaternion").and_then(|q| q.get(3)).and_then(Value::as_f64).unwrap_or_default(),
            );
        }
        let rotation_matrix = Matrix3::from_quaternion(&orientation);
        let inverse_rotation_matrix = rotation_matrix.inverse();
        let global_transform = Matrix4::from_rotation_translation(&rotation_matrix, position);
        let inverse_global_transform = global_transform.inverse().ok()?;
        let mut local_transform = global_transform.clone();
        if self.parent_global_transform.is_some() {
            if let Some(parent_inverse_global) = &self.parent_inverse_global_transform {
                local_transform = Matrix4::multiply(&global_transform, parent_inverse_global);
            }
        }
        self.global_transform = Some(global_transform);
        self.inverse_global_transform = Some(inverse_global_transform);
        self.inverse_rotation_matrix = Some(inverse_rotation_matrix);

        // Get children definition.
        let children_definition: Vec<Value> = self.children.iter_mut().map(|child| child.create_3d_tile_definition().unwrap_or(Value::Null)).collect();

        let transform = [
            local_transform.0[0], local_transform.0[4], local_transform.0[8], local_transform.0[12],
            local_transform.0[1], local_transform.0[5], local_transform.0[9], local_transform.0[13],
            local_transform.0[2], local_transform.0[6], local_transform.0[10], local_transform.0[14],
            local_transform.0[3], local_transform.0[7], local_transform.0[11], local_transform.0[15],
        ];

        Some(json!({
            "children": children_definition,
            "refine": "REPLACE",
            "boundingVolume": bounding_volume,
            "transform": transform,
            "content": match &self.resource { Some(resource) => json!({ "uri": resource.url() }), None => json!({}) },
            "geometricError": geometric_error,
        }))
    }

    /// Port of `I3SNode.prototype._createContentURL`: decodes the I3S
    /// geometry and returns the in-memory glTF document as binary glTF
    /// (GLB) bytes, or `None` when no content was produced.
    pub async fn create_content_url(&mut self, client: &ResourceClient) -> Result<Option<Vec<u8>>> {
        let mut raw_gltf = json!({
            "scene": 0,
            "scenes": [{ "nodes": [0] }],
            "nodes": [{ "name": "singleNode" }],
            "meshes": [],
            "buffers": [],
            "bufferViews": [],
            "accessors": [],
            "materials": [],
            "textures": [],
            "images": [],
            "samplers": [],
            "asset": { "version": "2.0" }
        });

        self.load_geometry_data(client).await?;
        if self.layer().and_then(|l| l.legacy_version_16()).unwrap_or(false) {
            self.load_feature_data(client).await?;
        }
        if !self.geometry_data.is_empty() {
            if self.data_provider().is_some_and(|p| p.apply_symbology()) {
                self.load_symbology(client).await?;
            }
            let url = self.geometry_data[0].resource().url();
            let geometry_schema = self
                .layer()
                .and_then(|l| l.data().get("store").and_then(|s| s.get("defaultGeometrySchema")).cloned())
                .unwrap_or(Value::Null);
            let feature_data = self.feature_data.first();
            let symbology_data = self.symbology_data.clone();
            let Some(mesh_data) = i3s_decoder::decode(&url, &geometry_schema, self, feature_data, symbology_data.as_ref()) else {
                // Postponed: no decode scheduled this frame.
                return Ok(None);
            };
            raw_gltf = self.geometry_data[0].generate_gltf(self, &mesh_data);
            if let Some(geometry_data) = self.geometry_data.first_mut() {
                geometry_data.custom_attributes = Some(mesh_data);
            }
        }
        Ok(Some(I3SDataProvider::binarize_gltf(&raw_gltf)))
    }

    /// Port of `I3SNode.prototype._filterFeatures`. The tile/content
    /// batch-table hiding is tied to the `Cesium3DTile` content
    /// integration and is not exercised by the port yet.
    pub fn filter_features(&mut self) -> Result<()> {
        for child in &mut self.children {
            child.filter_features()?;
        }
        Ok(())
    }
}
