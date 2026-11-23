//! Port of CesiumJS `Scene/I3SGeometry`.

use crate::core::cartesian3::Cartesian3;
use crate::core::matrix3::Matrix3;
use crate::core::resource::{DerivedResourceOptions, Resource};
use crate::core::resource_client::ResourceClient;
use crate::core::srgb_to_linear::srgb_to_linear;
use crate::scene::i3s_data_provider::I3SDataProvider;
use crate::scene::i3s_layer::{I3SLayer, I3sGeometryBufferInfo};
use crate::scene::i3s_node::I3SNode;
use crate::scene::scene::Result;
use crate::workers::decode_i3s::{I3sMeshData, I3sRootExtensions};
use serde_json::{json, Map, Value};
use std::rc::{Rc, Weak};

/// Port of `I3SGeometry`: an I3S geometry — each generates an in-memory
/// glTF to be used as content for a Cesium3DTile. Do not construct directly;
/// access tiles through [`I3SNode::geometry_data`].
#[derive(Clone)]
pub struct I3SGeometry {
    data_provider: Weak<I3SDataProvider>,
    layer: Weak<I3SLayer>,
    resource: Resource,
    data: Option<Vec<u8>>,
    pub custom_attributes: Option<I3sMeshData>,
    /// Rank of the geometry-buffer definition chosen for this geometry.
    pub geometry_definitions: Option<Vec<I3sGeometryBufferInfo>>,
    /// The buffer info the rank chose for this geometry.
    pub geometry_buffer_info: Option<I3sGeometryBufferInfo>,
}

impl I3SGeometry {
    /// Port of `new I3SGeometry(parent, uri)`.
    pub fn new(parent: &I3SNode, uri: &str) -> Self {
        let layer = parent.layer().expect("I3S geometry parent has a layer");
        let resource = if parent.node_index().is_some() {
            let resource_val = parent
                .data()
                .and_then(|data| data.get("mesh"))
                .and_then(|mesh| mesh.get("geometry"))
                .and_then(|geometry| geometry.get("resource"));
            let geometry_resource = resource_val
                .and_then(Value::as_str)
                .map(str::to_owned)
                .or_else(|| resource_val.and_then(Value::as_u64).map(|n| n.to_string()))
                .unwrap_or_default();
            layer.resource().get_derived_resource(DerivedResourceOptions {
                url: Some(format!("nodes/{geometry_resource}/{uri}")),
                ..DerivedResourceOptions::default()
            })
        } else {
            parent
                .resource()
                .and_then(|resource| resource.get_derived_resource(DerivedResourceOptions {
                    url: Some(uri.to_string()),
                    ..DerivedResourceOptions::default()
                }))
        };
        Self {
            data_provider: parent
                .data_provider()
                .map(|provider| Rc::downgrade(&provider))
                .expect("I3S geometry parent has a data provider"),
            layer: Rc::downgrade(&layer),
            resource: resource.expect("valid I3S geometry URI"),
            data: None,
            custom_attributes: None,
            geometry_definitions: None,
            geometry_buffer_info: None,
        }
    }

    /// Port of the `dataProvider` getter.
    pub fn data_provider(&self) -> Option<Rc<I3SDataProvider>> {
        self.data_provider.upgrade()
    }

    /// Port of the `resource` getter.
    pub fn resource(&self) -> &Resource {
        &self.resource
    }

    /// Port of the `data` getter: the binary payload, or `None` until loaded.
    pub fn data(&self) -> Option<&Vec<u8>> {
        self.data.as_ref()
    }

    /// Port of the `customAttributes` getter.
    pub fn custom_attributes(&self) -> Option<&I3sMeshData> {
        self.custom_attributes.as_ref()
    }

    /// Port of `I3SGeometry.prototype.load`.
    pub async fn load(&mut self, client: &ResourceClient) -> Result<()> {
        let data = I3SDataProvider::load_binary(&self.resource, client).await?;
        self.data = Some(data);
        Ok(())
    }

    /// Port of `I3SGeometry.prototype.getClosestPointIndexOnTriangle`: find
    /// a triangle touching `(px, py, pz)`, then return the vertex closest to
    /// the search point.
    pub fn get_closest_point_index_on_triangle(&self, px: f64, py: f64, pz: f64) -> I3sClosestPoint {
        let Some(attributes) = self.custom_attributes.as_ref() else {
            return I3sClosestPoint::no_hit();
        };
        let Some(positions) = attributes.positions.as_ref() else {
            return I3sClosestPoint::no_hit();
        };
        let mut position = Cartesian3::new(px, py, pz);
        if let Some(center) = attributes.cartesian_center {
            position.x -= center.x;
            position.y -= center.y;
            position.z -= center.z;
        }
        if let Some(rotation) = attributes.parent_rotation {
            position = multiply_by_vector(&rotation, position);
        }
        let mut best_tri_dist = f64::INFINITY;
        let mut best_tri: Option<usize> = None;
        let mut best_dist_sq = 0.0;
        let mut best_index: Option<u32> = None;
        let mut best_pt: Option<Cartesian3> = None;
        // Faithful to Cesium: the index count doubles as the triangle count.
        let tri_count = attributes.indices.as_ref().map(Vec::len).unwrap_or_else(|| positions.len() / 3);
        for tri_index in 0..tri_count {
            let (i0, i1, i2): (usize, usize, usize) = if let Some(indices) = attributes.indices.as_ref() {
                (
                    indices.get(tri_index).copied().unwrap_or_default() as usize,
                    indices.get(tri_index + 1).copied().unwrap_or_default() as usize,
                    indices.get(tri_index + 2).copied().unwrap_or_default() as usize,
                )
            } else {
                (tri_index * 3, tri_index * 3 + 1, tri_index * 3 + 2)
            };
            let v0 = position_at(positions, i0);
            let v1 = position_at(positions, i1);
            let v2 = position_at(positions, i2);
            if !same_side(&position, &v0, &v1, &v2) || !same_side(&position, &v1, &v0, &v2) || !same_side(&position, &v2, &v0, &v1) {
                continue;
            }
            let v0v1 = v1.subtract(v0);
            let v0v2 = v2.subtract(v0);
            let cross_prod = v0v1.cross(v0v2);
            // Skip "triangles" with 3 collinear points
            if cross_prod.magnitude() == 0.0 {
                continue;
            }
            let normal = cross_prod.normalize();
            let v0p = position.subtract(v0);
            let normal_dist = v0p.dot(normal).abs();
            if normal_dist < best_tri_dist {
                best_tri_dist = normal_dist;
                best_tri = Some(tri_index);
                let d0 = position.subtract(v0).magnitude_squared();
                let d1 = position.subtract(v1).magnitude_squared();
                let d2 = position.subtract(v2).magnitude_squared();
                if d0 < d1 && d0 < d2 {
                    best_index = Some(i0 as u32);
                    best_pt = Some(v0);
                    best_dist_sq = d0;
                } else if d1 < d2 {
                    best_index = Some(i1 as u32);
                    best_pt = Some(v1);
                    best_dist_sq = d1;
                } else {
                    best_index = Some(i2 as u32);
                    best_pt = Some(v2);
                    best_dist_sq = d2;
                }
            }
        }
        if best_tri.is_some() && best_index.is_some() {
            return I3sClosestPoint {
                index: best_index.map(|i| i as i64).unwrap_or(-1),
                distance_squared: best_dist_sq,
                distance: best_dist_sq.sqrt(),
                queried_position: Some(position),
                closest_position: best_pt,
            };
        }
        I3sClosestPoint::no_hit()
    }

    /// Port of `I3SGeometry.prototype._generateGltf`: the in-memory glTF
    /// document for the decoded mesh, built from the layer/node material
    /// definitions.
    pub fn generate_gltf(&self, node: &I3SNode, mesh: &I3sMeshData) -> Value {
        let layer = self.layer.upgrade();
        let node_data = node.data();
        let layer_data = layer.as_ref().map(|l| l.data());

        // Get the material definition.
        let mut gltf_material: Value = json!({
            "pbrMetallicRoughness": { "metallicFactor": 0.0 },
            "doubleSided": true,
            "name": "Material"
        });

        let mut is_textured = false;
        let mut texture_path = String::new();
        if let (Some(node_data), Some(layer_data)) = (node_data, layer_data) {
            if let Some(material_definitions) = layer_data.get("materialDefinitions").and_then(Value::as_array) {
                if let Some(material_info) = node_data.get("mesh").and_then(|m| m.get("material")) {
                    if let Some(material_index) = material_info.get("definition").and_then(Value::as_i64) {
                        if material_index >= 0 && (material_index as usize) < material_definitions.len() {
                            let mut material_definition = material_definitions[material_index as usize].clone();
                            if let Some(pbr) = material_definition.get_mut("pbrMetallicRoughness") {
                                if let Some(base_color_texture) = pbr.get_mut("baseColorTexture") {
                                    is_textured = true;
                                    base_color_texture.as_object_mut().unwrap().insert("index".to_string(), Value::from(0));
                                    // Choose the JPG for the texture.
                                    let mut texture_name = "0".to_string();
                                    if let Some(texture_set_definitions) =
                                        layer_data.get("textureSetDefinitions").and_then(Value::as_array)
                                    {
                                        'defs: for texture_set_definition in texture_set_definitions {
                                            if let Some(formats) = texture_set_definition.get("formats").and_then(Value::as_array) {
                                                for texture_format in formats {
                                                    if texture_format.get("format").and_then(Value::as_str) == Some("jpg") {
                                                        texture_name = texture_format.get("name").and_then(Value::as_str).unwrap_or_default().to_string();
                                                        break 'defs;
                                                    }
                                                }
                                            }
                                        }
                                    }
                                    let material_resource = material_info
                                        .get("resource")
                                        .and_then(|v| v.as_str().map(str::to_owned).or_else(|| v.as_u64().map(|n| n.to_string())))
                                        .unwrap_or_default();
                                    if let Some(resource) = layer
                                        .as_ref()
                                        .and_then(|layer| layer.resource().get_derived_resource(DerivedResourceOptions {
                                            url: Some(format!("nodes/{material_resource}/textures/{texture_name}")),
                                            ..DerivedResourceOptions::default()
                                        }))
                                    {
                                        texture_path = resource.url();
                                    }
                                }
                            }
                            // Convert color factors from sRGB to linear color space.
                            if let Some(base_color_factor) = material_definition
                                .get_mut("pbrMetallicRoughness")
                                .and_then(|pbr| pbr.get_mut("baseColorFactor"))
                            {
                                *base_color_factor = convert_color_factor(base_color_factor);
                            }
                            if let Some(emissive_factor) = material_definition.get_mut("emissiveFactor") {
                                *emissive_factor = convert_color_factor(emissive_factor);
                            }
                            gltf_material = material_definition;
                            if let Some(alpha_mode) = gltf_material.get_mut("alphaMode") {
                                // I3S specifies alphaMode values in lowercase; glTF expects uppercase.
                                *alpha_mode = Value::String(alpha_mode.as_str().unwrap_or_default().to_uppercase());
                            }
                        }
                    }
                }
            } else if let Some(texture_data) = node_data.get("textureData").and_then(Value::as_array) {
                // No material definition, but if there's a texture reference, we can create a simple material using it (version 1.6 support).
                is_textured = true;
                if let Some(resource) = node.resource().and_then(|resource| resource.get_derived_resource(DerivedResourceOptions {
                    url: Some(texture_data.first().and_then(|t| t.get("href")).and_then(Value::as_str).unwrap_or_default().to_string()),
                    ..DerivedResourceOptions::default()
                })) {
                    texture_path = resource.url();
                }
                if let Some(pbr) = gltf_material.get_mut("pbrMetallicRoughness") {
                    pbr.as_object_mut().unwrap().insert("baseColorTexture".to_string(), json!({"index": 0}));
                }
            }
        }

        let mut gltf_textures = Vec::new();
        let mut gltf_images = Vec::new();
        let mut gltf_samplers = Vec::new();
        if is_textured {
            gltf_textures.push(json!({ "sampler": 0, "source": 0 }));
            gltf_images.push(json!({ "uri": texture_path }));
            gltf_samplers.push(json!({ "magFilter": 9729, "minFilter": 9986, "wrapS": 10497, "wrapT": 10497 }));
        }

        let mut gltf_materials: Vec<Value> = Vec::new();
        for mesh_primitive in mesh.meshes.iter().flat_map(|m| m.primitives.iter()) {
            while mesh_primitive.material >= gltf_materials.len() {
                gltf_materials.push(gltf_material.clone());
            }
            if let Some(primitive_material) = gltf_materials.get_mut(mesh_primitive.material) {
                // If the geometry is transparent but the material is not, force BLEND.
                if mesh_primitive.transparent {
                    if primitive_material.get("alphaMode").is_none() {
                        primitive_material.as_object_mut().unwrap().insert("alphaMode".to_string(), Value::String("BLEND".to_string()));
                    }
                } else if primitive_material.get("alphaMode").and_then(Value::as_str) == Some("BLEND") {
                    // Otherwise force OPAQUE.
                    primitive_material.as_object_mut().unwrap().insert("alphaMode".to_string(), Value::String("OPAQUE".to_string()));
                }
            }
        }

        let mut gltf_doc = Map::new();
        gltf_doc.insert("scene".to_string(), Value::from(0));
        gltf_doc.insert("scenes".to_string(), json!([{ "nodes": mesh.nodes_in_scene }]));
        gltf_doc.insert("nodes".to_string(), Value::Array(mesh.nodes.iter().map(|n| json!({ "mesh": n.mesh })).collect()));
        gltf_doc.insert("meshes".to_string(), Value::Array(mesh.meshes.iter().map(gltf_mesh_value).collect()));
        let gltf_buffers: Vec<Value> = mesh
            .buffers
            .iter()
            .map(|b| {
                json!({
                    "byteLength": b.len(),
                    "uri": format!("data:application/octet-stream;base64,{}", base64_encode(b))
                })
            })
            .collect();
        gltf_doc.insert("buffers".to_string(), Value::Array(gltf_buffers));
        gltf_doc.insert("bufferViews".to_string(), Value::Array(mesh.buffer_views.iter().map(gltf_buffer_view_value).collect()));
        gltf_doc.insert("accessors".to_string(), Value::Array(mesh.accessors.iter().map(gltf_accessor_value).collect()));
        gltf_doc.insert("materials".to_string(), Value::Array(gltf_materials));
        gltf_doc.insert("textures".to_string(), Value::Array(gltf_textures));
        gltf_doc.insert("images".to_string(), Value::Array(gltf_images));
        gltf_doc.insert("samplers".to_string(), Value::Array(gltf_samplers));
        gltf_doc.insert("asset".to_string(), json!({ "version": "2.0" }));
        gltf_doc.insert("extensions".to_string(), root_extensions_value(mesh.root_extensions.as_ref()));
        gltf_doc.insert("extensionsUsed".to_string(), mesh.extensions_used.clone().map(|v| Value::Array(v.into_iter().map(Value::String).collect())).unwrap_or(Value::Null));
        Value::Object(gltf_doc)
    }
}

/// `sameSide`: the two cross products of `ab` with `p1 - a` and `p2 - a` dot
/// to a non-negative value.
fn same_side(p1: &Cartesian3, p2: &Cartesian3, a: &Cartesian3, b: &Cartesian3) -> bool {
    let ab = b.subtract(*a);
    let cp1 = ab.cross(p1.subtract(*a));
    let cp2 = ab.cross(p2.subtract(*a));
    cp1.dot(cp2) >= 0.0
}

/// `positions[i * 3..]` as a `Cartesian3`; missing entries are zero, like
/// `undefined` read from the array.
fn position_at(positions: &[f64], index: usize) -> Cartesian3 {
    Cartesian3::new(
        positions.get(index * 3).copied().unwrap_or_default(),
        positions.get(index * 3 + 1).copied().unwrap_or_default(),
        positions.get(index * 3 + 2).copied().unwrap_or_default(),
    )
}

/// `Matrix3::multiplyByVector` port for this port's column-major storage.
fn multiply_by_vector(matrix: &Matrix3, v: Cartesian3) -> Cartesian3 {
    let m = &matrix.0;
    Cartesian3::new(
        m[0] * v.x + m[3] * v.y + m[6] * v.z,
        m[1] * v.x + m[4] * v.y + m[7] * v.z,
        m[2] * v.x + m[5] * v.y + m[8] * v.z,
    )
}

/// `convertColorFactor`: sRGB to linear for the first three channels.
fn convert_color_factor(factor: &Value) -> Value {
    let mut converted = Vec::new();
    if let Some(items) = factor.as_array() {
        for (i, item) in items.iter().enumerate() {
            let value = item.as_f64().unwrap_or_default();
            if i < 3 {
                converted.push(Value::from(srgb_to_linear(value)));
            } else {
                converted.push(item.clone());
            }
        }
    }
    Value::Array(converted)
}

/// Minimal base64 encoder for data-URI glTF buffers.
fn base64_encode(data: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(data.len().div_ceil(3) * 4);
    for chunk in data.chunks(3) {
        let b = [chunk[0], *chunk.get(1).unwrap_or(&0), *chunk.get(2).unwrap_or(&0)];
        let n = ((b[0] as u32) << 16) | ((b[1] as u32) << 8) | b[2] as u32;
        out.push(ALPHABET[(n >> 18) as usize & 63] as char);
        out.push(ALPHABET[(n >> 12) as usize & 63] as char);
        out.push(if chunk.len() > 1 { ALPHABET[(n >> 6) as usize & 63] as char } else { '=' });
        out.push(if chunk.len() > 2 { ALPHABET[n as usize & 63] as char } else { '=' });
    }
    out
}

/// Serializes `I3sRootExtensions` to the glTF document `extensions` shape.
fn root_extensions_value(extensions: Option<&I3sRootExtensions>) -> Value {
    let Some(extensions) = extensions else { return Value::Null };
    let mut obj = Map::new();
    if let Some(property_table) = &extensions.property_table {
        obj.insert(
            "EXT_structural_metadata".to_string(),
            json!({
                "schema": {
                    "id": "i3s-metadata-schema-001",
                    "name": "I3S metadata schema 001",
                    "description": "The schema for I3S metadata",
                    "version": "1.0",
                    "classes": {
                        "feature": {
                            "name": "feature",
                            "description": "Feature metadata",
                            "properties": {
                                "index": {
                                    "description": "The feature index",
                                    "type": "SCALAR",
                                    "componentType": "FLOAT32",
                                    "required": true
                                }
                            }
                        }
                    }
                },
                "propertyTables": [{
                    "name": "feature-indices-mapping",
                    "class": "feature",
                    "count": property_table.count as u64,
                    "properties": {
                        "index": {
                            "values": property_table.values
                        }
                    }
                }]
            }),
        );
    }
    Value::Object(obj)
}


fn gltf_accessor_value(accessor: &crate::workers::decode_i3s::I3sAccessor) -> Value {
    let mut value = Map::new();
    value.insert("bufferView".to_string(), Value::from(accessor.buffer_view as u32));
    value.insert("byteOffset".to_string(), Value::from(accessor.byte_offset));
    value.insert("componentType".to_string(), Value::from(accessor.component_type));
    value.insert("count".to_string(), Value::from(accessor.count as u32));
    value.insert("type".to_string(), Value::String(accessor.type_name.to_string()));
    if let Some(min) = &accessor.min {
        value.insert("min".to_string(), json!([min[0], min[1], min[2]]));
    }
    if let Some(max) = &accessor.max {
        value.insert("max".to_string(), json!([max[0], max[1], max[2]]));
    }
    Value::Object(value)
}

fn gltf_buffer_view_value(buffer_view: &crate::workers::decode_i3s::I3sBufferView) -> Value {
    json!({
        "buffer": buffer_view.buffer as u32,
        "byteOffset": buffer_view.byte_offset,
        "byteLength": buffer_view.byte_length,
        "target": buffer_view.target
    })
}

fn gltf_primitive_value(primitive: &crate::workers::decode_i3s::I3sPrimitive) -> Value {
    let mut value = Map::new();
    value.insert("indices".to_string(), Value::from(primitive.indices as u32));
    value.insert("material".to_string(), Value::from(primitive.material as u32));
    let mut attributes = Map::new();
    for (semantic, index) in &primitive.attributes {
        attributes.insert(semantic.to_string(), Value::from(*index as u32));
    }
    value.insert("attributes".to_string(), Value::Object(attributes));
    let mut extensions = Map::new();
    if let Some(feature_count) = primitive.extensions.feature_count {
        extensions.insert(
            "EXT_mesh_features".to_string(),
            json!({
                "featureIds": [{
                    "attribute": 0,
                    "propertyTable": 0,
                    "featureCount": feature_count as u64
                }]
            }),
        );
    }
    if let Some(outline) = primitive.extensions.outline {
        extensions.insert(
            "CESIUM_primitive_outline".to_string(),
            json!({ "indices": outline as u32 }),
        );
    }
    if !extensions.is_empty() {
        value.insert("extensions".to_string(), Value::Object(extensions));
    }
    if primitive.transparent {
        value.insert("extra".to_string(), json!({ "isTransparent": true }));
    }
    Value::Object(value)
}

fn gltf_mesh_value(mesh: &crate::workers::decode_i3s::I3sMesh) -> Value {
    json!({ "primitives": mesh.primitives.iter().map(gltf_primitive_value).collect::<Vec<_>>() })
}

/// Success shape of [`I3SGeometry::get_closest_point_index_on_triangle`].
#[derive(Clone, Debug)]
pub struct I3sClosestPoint {
    /// Index of the closest vertex, or `-1` when no triangle touched the point.
    pub index: i64,
    pub distance_squared: f64,
    pub distance: f64,
    pub queried_position: Option<Cartesian3>,
    pub closest_position: Option<Cartesian3>,
}

impl I3sClosestPoint {
    fn no_hit() -> Self {
        Self {
            index: -1,
            distance_squared: f64::INFINITY,
            distance: f64::INFINITY,
            queried_position: None,
            closest_position: None,
        }
    }
}
