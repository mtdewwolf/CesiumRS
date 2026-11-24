//! CesiumJS `Cesium3DTilesVoxelProvider`: a voxel provider backed by a 3D
//! Tiles tileset with implicit tiling and `3DTILES_content_voxels`.
use super::{
    gltf_pipeline::{add_pipeline_extras, js, parse_glb, BufferSource, Gltf},
    has_extension,
    preprocess_3d_tile_content::{preprocess_3d_tile_content, TileContentPayload},
    voxel_content::VoxelAttributeSpec,
    Cesium3DTilesetMetadata, ImplicitSubtree, ImplicitSubtreeCache, ImplicitTileCoordinates, ImplicitTileset,
    MetadataComponentType, MetadataEntity, MetadataError, MetadataSemantic, MetadataType, MetadataValue, SubtreeSource,
    VoxelContent, VoxelMetadataOrder, VoxelProvider, VoxelRequest, VoxelShapeType,
};
use crate::core::{
    get_string_from_typed_array::get_magic,
    resource::DerivedResourceOptions,
    Cartesian3, ComponentDatatype, Ellipsoid, Matrix3, Matrix4, OrientedBoundingBox, Quaternion, Resource,
    ResourceClient, RuntimeError, TypedArray,
};
use base64::Engine;
use serde_json::Value;
use std::{cell::RefCell, collections::HashMap, rc::Rc};

type Result<T> = std::result::Result<T, RuntimeError>;

/// Constructor options for [`Cesium3DTilesVoxelProvider`].
#[derive(Clone, Debug)]
pub struct Cesium3DTilesVoxelProviderOptions {
    pub class_name: String,
    pub names: Vec<String>,
    pub types: Vec<MetadataType>,
    pub component_types: Vec<MetadataComponentType>,
    pub shape: VoxelShapeType,
    pub dimensions: Cartesian3,
    pub padding_before: Cartesian3,
    pub padding_after: Cartesian3,
    pub global_transform: Matrix4,
    pub shape_transform: Matrix4,
    pub min_bounds: Option<Cartesian3>,
    pub max_bounds: Option<Cartesian3>,
    pub minimum_values: Option<Vec<Vec<f64>>>,
    pub maximum_values: Option<Vec<Vec<f64>>>,
    pub maximum_tile_count: Option<usize>,
}
impl Default for Cesium3DTilesVoxelProviderOptions {
    fn default() -> Self {
        Self {
            class_name: String::new(),
            names: Vec::new(),
            types: Vec::new(),
            component_types: Vec::new(),
            shape: VoxelShapeType::Box,
            dimensions: Cartesian3::ONE,
            padding_before: Cartesian3::ZERO,
            padding_after: Cartesian3::ZERO,
            global_transform: Matrix4::IDENTITY,
            shape_transform: Matrix4::IDENTITY,
            min_bounds: None,
            max_bounds: None,
            minimum_values: None,
            maximum_values: None,
            maximum_tile_count: None,
        }
    }
}

/// The provider state and its implicit-tileset source.
pub struct Cesium3DTilesVoxelProvider {
    shape_transform: Matrix4,
    global_transform: Matrix4,
    shape: VoxelShapeType,
    min_bounds: Option<Cartesian3>,
    max_bounds: Option<Cartesian3>,
    dimensions: Cartesian3,
    padding_before: Cartesian3,
    padding_after: Cartesian3,
    class_name: String,
    names: Vec<String>,
    types: Vec<MetadataType>,
    component_types: Vec<MetadataComponentType>,
    metadata_order: VoxelMetadataOrder,
    minimum_values: Option<Vec<Vec<f64>>>,
    maximum_values: Option<Vec<Vec<f64>>>,
    maximum_tile_count: Option<usize>,
    available_levels: Option<u32>,
    implicit_tileset: Option<ImplicitTileset>,
    subtree_cache: Rc<RefCell<ImplicitSubtreeCache<Rc<ImplicitSubtree>>>>,
    client: Option<ResourceClient>,
}

impl Cesium3DTilesVoxelProvider {
    pub fn new(options: Cesium3DTilesVoxelProviderOptions) -> Result<Self> {
        if options.class_name.is_empty() {
            return Err(RuntimeError::new("className is required"));
        }
        if options.names.len() != options.types.len() || options.names.len() != options.component_types.len() {
            return Err(RuntimeError::new("Voxel provider metadata arrays must have the same length"));
        }
        Ok(Self {
            shape_transform: options.shape_transform,
            global_transform: options.global_transform,
            shape: options.shape,
            min_bounds: options.min_bounds,
            max_bounds: options.max_bounds,
            dimensions: options.dimensions,
            padding_before: options.padding_before,
            padding_after: options.padding_after,
            metadata_order: if options.shape == VoxelShapeType::Ellipsoid {
                VoxelMetadataOrder::ZUp
            } else {
                VoxelMetadataOrder::YUp
            },
            class_name: options.class_name,
            names: options.names,
            types: options.types,
            component_types: options.component_types,
            minimum_values: options.minimum_values,
            maximum_values: options.maximum_values,
            maximum_tile_count: options.maximum_tile_count,
            available_levels: None,
            implicit_tileset: None,
            subtree_cache: Rc::new(RefCell::new(ImplicitSubtreeCache::new(None))),
            client: None,
        })
    }

    pub fn class_name(&self) -> &str {
        &self.class_name
    }

    fn attribute_spec(&self) -> VoxelAttributeSpec {
        VoxelAttributeSpec {
            class_name: self.class_name.clone(),
            names: self.names.clone(),
            types: self.types.clone(),
            component_types: self.component_types.clone(),
        }
    }

    /// Creates a provider from a tileset JSON URL.
    pub async fn from_url(resource: Resource, client: ResourceClient) -> Result<Self> {
        let json = client
            .fetch_json(&resource)
            .ok_or_else(|| RuntimeError::new("Tileset request was throttled"))?
            .await
            .map_err(|error| RuntimeError::new(error.to_string()))?;
        Self::from_json(resource, json, client).await
    }

    pub async fn from_json(resource: Resource, tileset_json: Value, client: ResourceClient) -> Result<Self> {
        validate(&tileset_json)?;
        let owned_schema: Option<Value>;
        let schema_json = if let Some(schema) = tileset_json.get("schema").filter(|v| !v.is_null()) {
            schema
        } else if let Some(uri) = tileset_json.get("schemaUri").and_then(Value::as_str) {
            let schema_resource = resource
                .get_derived_resource(DerivedResourceOptions { url: Some(uri.into()), ..Default::default() })
                .ok_or_else(|| RuntimeError::new("Invalid schema URI"))?;
            owned_schema = Some(
                client
                    .fetch_json(&schema_resource)
                    .ok_or_else(|| RuntimeError::new("Schema request was throttled"))?
                    .await
                    .map_err(|error| RuntimeError::new(error.to_string()))?,
            );
            owned_schema.as_ref().unwrap()
        } else {
            return Err(RuntimeError::new("Tileset must have a metadata schema"));
        };
        let schema = std::sync::Arc::new(
            super::MetadataSchema::from_json(schema_json).map_err(|error| RuntimeError::new(error.to_string()))?,
        );
        let metadata_json = if has_extension(&tileset_json, "3DTILES_metadata") {
            tileset_json["extensions"]["3DTILES_metadata"].clone()
        } else {
            tileset_json.clone()
        };
        let tileset_metadata = Cesium3DTilesetMetadata::new(&metadata_json, schema.clone())
            .map_err(|error| RuntimeError::new(error.to_string()))?;
        let root = &tileset_json["root"];
        let voxel = &root["content"]["extensions"]["3DTILES_content_voxels"];
        let class_name = voxel["class"]
            .as_str()
            .ok_or_else(|| RuntimeError::new("3DTILES_content_voxels must define a class"))?
            .to_string();
        let mut provider_options = get_attribute_info(&tileset_metadata, &class_name)?;
        let shape = get_shape(root)?;
        provider_options.shape = shape.shape;
        provider_options.min_bounds = shape.min_bounds;
        provider_options.max_bounds = shape.max_bounds;
        provider_options.shape_transform = shape.shape_transform;
        provider_options.global_transform = root
            .get("transform")
            .and_then(Value::as_array)
            .map(|values| {
                let values: Vec<f64> = values.iter().map(|v| v.as_f64().unwrap_or(f64::NAN)).collect();
                Matrix4::from_array(&values, 0)
            })
            .unwrap_or(Matrix4::IDENTITY);
        provider_options.dimensions = unpack_cartesian3(&voxel["dimensions"]);
        provider_options.maximum_tile_count = get_tile_count(&tileset_metadata);
        if let Some(padding) = voxel.get("padding").filter(|v| !v.is_null()) {
            provider_options.padding_before = unpack_cartesian3(&padding["before"]);
            provider_options.padding_after = unpack_cartesian3(&padding["after"]);
        }
        let mut provider = Self::new(provider_options)?;
        let implicit_tileset = ImplicitTileset::new(resource, root, Some(schema))
            .map_err(|error| RuntimeError::new(error.to_string()))?;
        provider.available_levels = if implicit_tileset.available_levels.is_finite() {
            Some(implicit_tileset.available_levels as u32)
        } else {
            None
        };
        provider.implicit_tileset = Some(implicit_tileset);
        provider.client = Some(client);
        Ok(provider)
    }

    pub async fn request_data_async(&mut self, options: VoxelRequest) -> Result<VoxelContent> {
        let implicit_tileset = self
            .implicit_tileset
            .clone()
            .ok_or_else(|| RuntimeError::new("Provider has no implicit tileset"))?;
        let client = self
            .client
            .clone()
            .ok_or_else(|| RuntimeError::new("Provider has no resource client"))?;
        let cache = Rc::clone(&self.subtree_cache);
        let spec = self.attribute_spec();
        request_tile_data(implicit_tileset, client, cache, spec, options).await
    }
}

async fn request_tile_data(
    implicit_tileset: ImplicitTileset,
    client: ResourceClient,
    subtree_cache: Rc<RefCell<ImplicitSubtreeCache<Rc<ImplicitSubtree>>>>,
    spec: VoxelAttributeSpec,
    options: VoxelRequest,
) -> Result<VoxelContent> {
    if options.keyframe != 0 {
        return Err(RuntimeError::new("3D Tiles currently doesn't support time-dynamic data."));
    }
    let coordinates = ImplicitTileCoordinates::new(
        implicit_tileset.subdivision_scheme,
        implicit_tileset.subtree_levels,
        options.tile_level as i64,
        options.tile_x as i64,
        options.tile_y as i64,
        Some(options.tile_z as i64),
    )
    .map_err(|error| RuntimeError::new(error.to_string()))?;
    let is_subtree_root = coordinates.is_subtree_root() && coordinates.level > 0;
    let subtree_coord = if is_subtree_root {
        coordinates
            .get_parent_subtree_coordinates()
            .map_err(|error| RuntimeError::new(error.to_string()))?
    } else {
        coordinates
            .get_subtree_coordinates()
            .map_err(|error| RuntimeError::new(error.to_string()))?
    };

    let subtree = fetch_subtree(&implicit_tileset, &subtree_cache, &subtree_coord, &client).await?;
    let available = if is_subtree_root {
        subtree.child_subtree_is_available_at_coordinates(&coordinates)
    } else {
        subtree.tile_is_available_at_coordinates(&coordinates)
    }
    .map_err(|error| RuntimeError::new(error.to_string()))?;
    if !available {
        return Err(RuntimeError::new(format!(
            "Tile is not available at level {}, x {}, y {}, z {}.",
            options.tile_level, options.tile_x, options.tile_y, options.tile_z
        )));
    }

    let content_template = implicit_tileset
        .content_uri_templates
        .first()
        .ok_or_else(|| RuntimeError::new("Implicit tileset has no content URI template"))?;
    let template_values: Vec<(String, String)> = coordinates
        .get_template_values()
        .into_iter()
        .map(|(name, value)| (name.to_string(), value.to_string()))
        .collect();
    let gltf_relative = content_template
        .get_derived_resource(DerivedResourceOptions { template_values, ..Default::default() })
        .ok_or_else(|| RuntimeError::new("Invalid voxel content URI template"))?;
    let gltf_resource = implicit_tileset
        .base_resource
        .get_derived_resource(DerivedResourceOptions { url: Some(gltf_relative.url()), ..Default::default() })
        .ok_or_else(|| RuntimeError::new("Invalid voxel content URL"))?;

    load_voxel_gltf(&gltf_resource, &client, &spec).await
}

async fn fetch_subtree(
    implicit_tileset: &ImplicitTileset,
    subtree_cache: &Rc<RefCell<ImplicitSubtreeCache<Rc<ImplicitSubtree>>>>,
    coordinates: &ImplicitTileCoordinates,
    client: &ResourceClient,
) -> Result<Rc<ImplicitSubtree>> {
    if let Some(subtree) = subtree_cache.borrow().find(coordinates) {
        return Ok(subtree.clone());
    }
    let template_values: Vec<(String, String)> = coordinates
        .get_template_values()
        .into_iter()
        .map(|(name, value)| (name.to_string(), value.to_string()))
        .collect();
    let subtree_relative = implicit_tileset
        .subtree_uri_template
        .get_derived_resource(DerivedResourceOptions { template_values, ..Default::default() })
        .ok_or_else(|| RuntimeError::new("Invalid subtree URI template"))?;
    let subtree_resource = implicit_tileset
        .base_resource
        .get_derived_resource(DerivedResourceOptions { url: Some(subtree_relative.url()), ..Default::default() })
        .ok_or_else(|| RuntimeError::new("Invalid subtree URL"))?;
    let bytes = client
        .fetch_array_buffer(&subtree_resource)
        .ok_or_else(|| RuntimeError::new("Subtree request was throttled"))?
        .await
        .map_err(|error| RuntimeError::new(error.to_string()))?;

    // Check cache again in case a concurrent request populated it while awaiting.
    if let Some(subtree) = subtree_cache.borrow().find(coordinates) {
        return Ok(subtree.clone());
    }

    let preprocessed = preprocess_3d_tile_content(&bytes)?;
    // Pre-fetch any external buffer URIs referenced by a JSON subtree so the
    // synchronous `ImplicitSubtree::from_subtree_json` callback always resolves.
    let mut external_buffers: HashMap<String, Vec<u8>> = HashMap::new();
    if let TileContentPayload::Json(ref json) = preprocessed.payload {
        if let Some(buffers) = json.get("buffers").and_then(Value::as_array) {
            for buffer in buffers {
                if let Some(uri) = buffer.get("uri").and_then(Value::as_str) {
                    if let Some(buf_resource) = subtree_resource
                        .get_derived_resource(DerivedResourceOptions { url: Some(uri.into()), ..Default::default() })
                    {
                        let buf_bytes = if let Some(decoded) = decode_data_uri(uri)? {
                            decoded
                        } else {
                            client
                                .fetch_array_buffer(&buf_resource)
                                .ok_or_else(|| RuntimeError::new("Subtree buffer request was throttled"))?
                                .await
                                .map_err(|error| RuntimeError::new(error.to_string()))?
                        };
                        external_buffers.insert(buf_resource.url(), buf_bytes);
                    }
                }
            }
        }
    }

    let source = match preprocessed.payload {
        TileContentPayload::Json(json) => SubtreeSource::Json(json),
        TileContentPayload::Binary(binary) => SubtreeSource::Binary(binary),
    };
    let mut load_buffer = |resource: &Resource| -> std::result::Result<Vec<u8>, MetadataError> {
        if let Some(bytes) = external_buffers.get(&resource.url()) {
            return Ok(bytes.clone());
        }
        let future = client
            .fetch_array_buffer(resource)
            .ok_or_else(|| MetadataError::Runtime("Subtree buffer request was throttled".into()))?;
        futures_util::FutureExt::now_or_never(future)
            .ok_or_else(|| MetadataError::Runtime("Subtree buffer was not immediately available".into()))?
            .map_err(|error| MetadataError::Runtime(error.to_string()))
    };
    let subtree = Rc::new(
        ImplicitSubtree::from_subtree_json(
            &subtree_resource,
            source,
            implicit_tileset,
            *coordinates,
            &mut load_buffer,
        )
        .map_err(|error| RuntimeError::new(error.to_string()))?,
    );
    let _ = subtree_cache.borrow_mut().add_subtree(subtree.clone());
    Ok(subtree)
}

fn decode_data_uri(uri: &str) -> Result<Option<Vec<u8>>> {
    let Some(rest) = uri.strip_prefix("data:") else {
        return Ok(None);
    };
    let Some((header, payload)) = rest.split_once(',') else {
        return Err(RuntimeError::new("Invalid data URI"));
    };
    if header.ends_with(";base64") {
        let decoded = base64::engine::general_purpose::STANDARD
            .decode(payload.trim())
            .map_err(|error| RuntimeError::new(format!("Invalid base64 data URI: {error}")))?;
        Ok(Some(decoded))
    } else {
        Ok(Some(payload.as_bytes().to_vec()))
    }
}

/// Loads a glTF/GLB voxel tile and decodes its `EXT_structural_metadata`
/// property-attribute accessors into typed arrays ordered by `spec`.
pub async fn load_voxel_gltf(
    gltf_resource: &Resource,
    client: &ResourceClient,
    spec: &VoxelAttributeSpec,
) -> Result<VoxelContent> {
    let bytes = client
        .fetch_array_buffer(gltf_resource)
        .ok_or_else(|| RuntimeError::new("Voxel glTF request was throttled"))?
        .await
        .map_err(|error| RuntimeError::new(error.to_string()))?;

    let mut gltf = if get_magic(&bytes, 0) == "glTF" {
        parse_glb(&BufferSource::new(bytes))
            .map_err(|error| RuntimeError::new(error.to_string()))?
            .ok_or_else(|| RuntimeError::new("Binary glTF is missing a JSON chunk"))?
    } else {
        let mut json: Value = serde_json::from_slice(&bytes)
            .map_err(|error| RuntimeError::new(format!("Invalid voxel glTF JSON: {error}")))?;
        add_pipeline_extras(&mut json).map_err(|error| RuntimeError::new(error.to_string()))?;
        Gltf::new(json)
    };

    // Resolve any external or data-URI buffers not already embedded by GLB.
    let buffer_uris: Vec<(usize, String)> = gltf
        .json
        .get("buffers")
        .and_then(Value::as_array)
        .map(|buffers| {
            buffers
                .iter()
                .enumerate()
                .filter_map(|(index, buffer)| {
                    let has_source = buffer.pointer("/extras/_pipeline/source").is_some_and(|v| !v.is_null());
                    if has_source {
                        return None;
                    }
                    buffer.get("uri").and_then(Value::as_str).map(|uri| (index, uri.to_string()))
                })
                .collect()
        })
        .unwrap_or_default();

    for (index, uri) in buffer_uris {
        let buffer_bytes = if let Some(decoded) = decode_data_uri(&uri)? {
            decoded
        } else {
            let buffer_resource = gltf_resource
                .get_derived_resource(DerivedResourceOptions { url: Some(uri), ..Default::default() })
                .ok_or_else(|| RuntimeError::new("Invalid glTF buffer URI"))?;
            client
                .fetch_array_buffer(&buffer_resource)
                .ok_or_else(|| RuntimeError::new("Voxel glTF buffer request was throttled"))?
                .await
                .map_err(|error| RuntimeError::new(error.to_string()))?
        };
        let marker = gltf.add_source(BufferSource::new(buffer_bytes));
        if let Some(buffer) = gltf.json.pointer_mut(&format!("/buffers/{index}")) {
            let pipeline = js::object_mut(buffer.pointer_mut("/extras/_pipeline"), "source")
                .map_err(|error| RuntimeError::new(error.to_string()))?;
            js::set(pipeline, "source", marker).map_err(|error| RuntimeError::new(error.to_string()))?;
        }
    }

    decode_voxel_gltf(&gltf, spec)
}

/// Decodes voxel attribute arrays from a parsed glTF with loaded buffer sources,
/// matching `VoxelContent.fromGltf` / `processAttributes` in CesiumJS.
pub fn decode_voxel_gltf(gltf: &Gltf, spec: &VoxelAttributeSpec) -> Result<VoxelContent> {
    let ext = gltf
        .json
        .pointer("/extensions/EXT_structural_metadata")
        .ok_or_else(|| RuntimeError::new("Voxel glTF is missing EXT_structural_metadata extension"))?;
    let property_attributes = ext
        .get("propertyAttributes")
        .and_then(Value::as_array)
        .ok_or_else(|| RuntimeError::new("EXT_structural_metadata is missing propertyAttributes"))?;
    let property_attribute = property_attributes
        .iter()
        .find(|entry| entry.get("class").and_then(Value::as_str) == Some(spec.class_name.as_str()))
        .ok_or_else(|| RuntimeError::new(format!("No property attribute found for class '{}'", spec.class_name)))?;
    let properties = property_attribute
        .get("properties")
        .and_then(Value::as_object)
        .ok_or_else(|| RuntimeError::new("Property attribute is missing properties"))?;

    let scene_index = gltf.json.get("scene").and_then(Value::as_u64).unwrap_or(0) as usize;
    let node_index = gltf
        .json
        .pointer(&format!("/scenes/{scene_index}/nodes/0"))
        .and_then(Value::as_u64)
        .unwrap_or(0) as usize;
    let mesh_index = gltf
        .json
        .pointer(&format!("/nodes/{node_index}/mesh"))
        .and_then(Value::as_u64)
        .ok_or_else(|| RuntimeError::new("Voxel glTF node has no mesh"))? as usize;
    let primitive = gltf
        .json
        .pointer(&format!("/meshes/{mesh_index}/primitives/0"))
        .ok_or_else(|| RuntimeError::new("Voxel glTF mesh has no primitives"))?;
    let primitive_attributes = primitive
        .get("attributes")
        .and_then(Value::as_object)
        .ok_or_else(|| RuntimeError::new("Voxel glTF primitive has no attributes"))?;

    let mut metadata = Vec::with_capacity(spec.names.len());
    for i in 0..spec.names.len() {
        let name = &spec.names[i];
        let Some(attr_name) = properties
            .get(name)
            .and_then(|prop| prop.get("attribute"))
            .and_then(Value::as_str)
        else {
            metadata.push(TypedArray::Uint8(Vec::new()));
            continue;
        };
        let Some(accessor_index) = primitive_attributes.get(attr_name).and_then(Value::as_u64) else {
            metadata.push(TypedArray::Uint8(Vec::new()));
            continue;
        };
        let accessor = gltf
            .json
            .pointer(&format!("/accessors/{accessor_index}"))
            .ok_or_else(|| RuntimeError::new(format!("Missing accessor {accessor_index} for attribute {attr_name}")))?;
        let datatype = spec.component_types[i]
            .to_component_datatype()
            .ok_or_else(|| RuntimeError::new("Unsupported voxel MetadataComponentType"))?;
        let component_count = spec.types[i].get_component_count();
        let array = read_voxel_accessor(gltf, accessor, datatype, component_count)?;
        metadata.push(array);
    }

    Ok(VoxelContent::from_metadata_array(metadata))
}

fn read_voxel_accessor(
    gltf: &Gltf,
    accessor: &Value,
    datatype: ComponentDatatype,
    component_count: usize,
) -> Result<TypedArray> {
    let count = accessor
        .get("count")
        .and_then(Value::as_u64)
        .ok_or_else(|| RuntimeError::new("Accessor is missing count"))? as usize;
    let total_components = count
        .checked_mul(component_count)
        .ok_or_else(|| RuntimeError::new("Accessor component count overflow"))?;
    let component_size = datatype.get_size_in_bytes();
    let elem_bytes = component_count
        .checked_mul(component_size)
        .ok_or_else(|| RuntimeError::new("Accessor element size overflow"))?;

    let Some(buffer_view_index) = accessor.get("bufferView").and_then(Value::as_u64) else {
        let zeros = vec![0u8; total_components.checked_mul(component_size).unwrap_or(0)];
        return bytes_to_typed_array(datatype, &zeros);
    };
    let buffer_view = gltf
        .json
        .pointer(&format!("/bufferViews/{buffer_view_index}"))
        .ok_or_else(|| RuntimeError::new(format!("Missing bufferView {buffer_view_index}")))?;
    let buffer_index = buffer_view
        .get("buffer")
        .and_then(Value::as_u64)
        .ok_or_else(|| RuntimeError::new("bufferView is missing buffer index"))? as usize;
    let marker = gltf
        .json
        .pointer(&format!("/buffers/{buffer_index}/extras/_pipeline/source"))
        .ok_or_else(|| RuntimeError::new(format!("glTF buffer {buffer_index} has no loaded source")))?;
    let source = gltf
        .source(marker)
        .ok_or_else(|| RuntimeError::new(format!("glTF buffer {buffer_index} source marker is invalid")))?;

    let bv_offset = buffer_view.get("byteOffset").and_then(Value::as_u64).unwrap_or(0) as usize;
    let acc_offset = accessor.get("byteOffset").and_then(Value::as_u64).unwrap_or(0) as usize;
    let start = source
        .byte_offset
        .checked_add(bv_offset)
        .and_then(|v| v.checked_add(acc_offset))
        .ok_or_else(|| RuntimeError::new("Accessor byte offset overflow"))?;
    let stride = buffer_view
        .get("byteStride")
        .and_then(Value::as_u64)
        .map(|s| s as usize)
        .filter(|&s| s > 0)
        .unwrap_or(elem_bytes);

    if stride == elem_bytes {
        let total_bytes = count
            .checked_mul(elem_bytes)
            .ok_or_else(|| RuntimeError::new("Accessor byte length overflow"))?;
        let end = start
            .checked_add(total_bytes)
            .ok_or_else(|| RuntimeError::new("Accessor byte range overflow"))?;
        let slice = source
            .buffer
            .get(start..end)
            .ok_or_else(|| RuntimeError::new("Voxel accessor byte range out of bounds"))?;
        bytes_to_typed_array(datatype, slice)
    } else {
        let mut packed = Vec::with_capacity(count.saturating_mul(elem_bytes));
        for i in 0..count {
            let elem_start = start
                .checked_add(i.checked_mul(stride).ok_or_else(|| RuntimeError::new("Stride overflow"))?)
                .ok_or_else(|| RuntimeError::new("Element offset overflow"))?;
            let elem_end = elem_start
                .checked_add(elem_bytes)
                .ok_or_else(|| RuntimeError::new("Element end overflow"))?;
            let slice = source
                .buffer
                .get(elem_start..elem_end)
                .ok_or_else(|| RuntimeError::new("Strided voxel accessor out of bounds"))?;
            packed.extend_from_slice(slice);
        }
        bytes_to_typed_array(datatype, &packed)
    }
}

fn bytes_to_typed_array(datatype: ComponentDatatype, bytes: &[u8]) -> Result<TypedArray> {
    Ok(match datatype {
        ComponentDatatype::Byte => TypedArray::Int8(bytes.iter().map(|&b| b as i8).collect()),
        ComponentDatatype::UnsignedByte => TypedArray::Uint8(bytes.to_vec()),
        ComponentDatatype::Short => {
            TypedArray::Int16(bytes.as_chunks::<2>().0.iter().map(|&c| i16::from_le_bytes(c)).collect())
        }
        ComponentDatatype::UnsignedShort => {
            TypedArray::Uint16(bytes.as_chunks::<2>().0.iter().map(|&c| u16::from_le_bytes(c)).collect())
        }
        ComponentDatatype::Int => {
            TypedArray::Int32(bytes.as_chunks::<4>().0.iter().map(|&c| i32::from_le_bytes(c)).collect())
        }
        ComponentDatatype::UnsignedInt => {
            TypedArray::Uint32(bytes.as_chunks::<4>().0.iter().map(|&c| u32::from_le_bytes(c)).collect())
        }
        ComponentDatatype::Float => {
            TypedArray::Float32(bytes.as_chunks::<4>().0.iter().map(|&c| f32::from_le_bytes(c)).collect())
        }
        ComponentDatatype::Double => {
            TypedArray::Float64(bytes.as_chunks::<8>().0.iter().map(|&c| f64::from_le_bytes(c)).collect())
        }
    })
}

impl VoxelProvider for Cesium3DTilesVoxelProvider {
    fn global_transform(&self) -> &Matrix4 {
        &self.global_transform
    }
    fn shape_transform(&self) -> &Matrix4 {
        &self.shape_transform
    }
    fn shape(&self) -> VoxelShapeType {
        self.shape
    }
    fn min_bounds(&self) -> Option<Cartesian3> {
        self.min_bounds
    }
    fn max_bounds(&self) -> Option<Cartesian3> {
        self.max_bounds
    }
    fn dimensions(&self) -> Cartesian3 {
        self.dimensions
    }
    fn padding_before(&self) -> Cartesian3 {
        self.padding_before
    }
    fn padding_after(&self) -> Cartesian3 {
        self.padding_after
    }
    fn names(&self) -> &[String] {
        &self.names
    }
    fn types(&self) -> &[MetadataType] {
        &self.types
    }
    fn component_types(&self) -> &[MetadataComponentType] {
        &self.component_types
    }
    fn minimum_values(&self) -> Option<&[Vec<f64>]> {
        self.minimum_values.as_deref()
    }
    fn maximum_values(&self) -> Option<&[Vec<f64>]> {
        self.maximum_values.as_deref()
    }
    fn maximum_tile_count(&self) -> Option<usize> {
        self.maximum_tile_count
    }
    fn available_levels(&self) -> Option<u32> {
        self.available_levels
    }
    fn keyframe_count(&self) -> Option<u32> {
        Some(1)
    }
    fn time_interval_collection(&self) -> Option<&crate::core::TimeIntervalCollection<u32>> {
        None
    }
    fn metadata_order(&self) -> VoxelMetadataOrder {
        self.metadata_order
    }
    fn request_data(&mut self, options: VoxelRequest) -> Option<super::voxel_provider::VoxelFuture> {
        if options.keyframe != 0 {
            return Some(Box::pin(async {
                Err(RuntimeError::new("3D Tiles currently doesn't support time-dynamic data."))
            }));
        }
        let implicit_tileset = self.implicit_tileset.clone()?;
        let client = self.client.clone()?;
        let subtree_cache = Rc::clone(&self.subtree_cache);
        let spec = self.attribute_spec();
        Some(Box::pin(request_tile_data(implicit_tileset, client, subtree_cache, spec, options)))
    }
}

/// The shape fields derived from a tile's bounding volume.
pub struct ShapeInfo {
    pub shape: VoxelShapeType,
    pub min_bounds: Option<Cartesian3>,
    pub max_bounds: Option<Cartesian3>,
    pub shape_transform: Matrix4,
}

/// `validate`: the four required tileset features.
pub fn validate(tileset: &Value) -> Result<()> {
    let root = &tileset["root"];
    if root.get("content").is_none_or(Value::is_null) {
        return Err(RuntimeError::new("Root must have content"));
    }
    if !has_extension(&root["content"], "3DTILES_content_voxels") {
        return Err(RuntimeError::new("Root tile content must have 3DTILES_content_voxels extension"));
    }
    if !has_extension(root, "3DTILES_implicit_tiling") && root.get("implicitTiling").is_none_or(Value::is_null) {
        return Err(RuntimeError::new("Root tile must have implicit tiling"));
    }
    if tileset.get("schema").is_none_or(Value::is_null)
        && tileset.get("schemaUri").is_none_or(Value::is_null)
        && !has_extension(tileset, "3DTILES_metadata")
    {
        return Err(RuntimeError::new("Tileset must have a metadata schema"));
    }
    Ok(())
}

/// `getShape`: box, region or `3DTILES_bounding_volume_cylinder`.
pub fn get_shape(tile: &Value) -> Result<ShapeInfo> {
    let bounding_volume = &tile["boundingVolume"];
    if let Some(box_values) = bounding_volume.get("box").and_then(Value::as_array) {
        return Ok(get_box_shape(box_values));
    }
    if let Some(region) = bounding_volume.get("region").and_then(Value::as_array) {
        return Ok(get_ellipsoid_shape(region));
    }
    if let Some(cylinder) = bounding_volume
        .get("extensions")
        .and_then(|extensions| extensions.get("3DTILES_bounding_volume_cylinder"))
    {
        return get_cylinder_shape(cylinder);
    }
    Err(RuntimeError::new(
        "Only box, region and 3DTILES_bounding_volume_cylinder are supported in Cesium3DTilesVoxelProvider",
    ))
}

fn get_ellipsoid_shape(region: &[Value]) -> ShapeInfo {
    let number = |index: usize| region.get(index).and_then(Value::as_f64).unwrap_or(f64::NAN);
    ShapeInfo {
        shape: VoxelShapeType::Ellipsoid,
        min_bounds: Some(Cartesian3::new(number(0), number(1), number(4))),
        max_bounds: Some(Cartesian3::new(number(2), number(3), number(5))),
        shape_transform: Matrix4::from_scale(Ellipsoid::WGS84.radii),
    }
}

fn get_box_shape(box_values: &[Value]) -> ShapeInfo {
    let values: Vec<f64> = box_values.iter().map(|v| v.as_f64().unwrap_or(f64::NAN)).collect();
    let obb = OrientedBoundingBox::unpack(&values, 0);
    let c0 = obb.half_axes.get_column(0);
    let c1 = obb.half_axes.get_column(1);
    let c2 = obb.half_axes.get_column(2);
    let scale = Cartesian3::new(c0.magnitude(), c1.magnitude(), c2.magnitude());
    let rotation = Matrix3::IDENTITY
        .set_column(0, c0.multiply_by_scalar(1.0 / scale.x))
        .set_column(1, c1.multiply_by_scalar(1.0 / scale.y))
        .set_column(2, c2.multiply_by_scalar(1.0 / scale.z));
    ShapeInfo {
        shape: VoxelShapeType::Box,
        min_bounds: Some(Cartesian3::new(-scale.x, -scale.y, -scale.z)),
        max_bounds: Some(scale),
        shape_transform: Matrix4::from_rotation_translation(&rotation, obb.center),
    }
}

fn get_cylinder_shape(cylinder: &Value) -> Result<ShapeInfo> {
    let number = |name: &str| -> Result<f64> {
        cylinder
            .get(name)
            .and_then(Value::as_f64)
            .ok_or_else(|| RuntimeError::new(format!("{name} must be a number")))
    };
    let min_radius = number("minRadius")?;
    let max_radius = number("maxRadius")?;
    let height = number("height")?;
    let min_angle = cylinder.get("minAngle").and_then(Value::as_f64).unwrap_or(-std::f64::consts::PI);
    let max_angle = cylinder.get("maxAngle").and_then(Value::as_f64).unwrap_or(std::f64::consts::PI);
    let translation = cylinder
        .get("translation")
        .map(unpack_cartesian3)
        .unwrap_or(Cartesian3::ZERO);
    let rotation = cylinder
        .get("rotation")
        .and_then(Value::as_array)
        .map(|values| {
            let values: Vec<f64> = values.iter().map(|v| v.as_f64().unwrap_or(f64::NAN)).collect();
            Quaternion::unpack(&values, 0)
        })
        .unwrap_or(Quaternion::IDENTITY);
    Ok(ShapeInfo {
        shape: VoxelShapeType::Cylinder,
        min_bounds: Some(Cartesian3::new(min_radius, min_angle, -0.5 * height + translation.z)),
        max_bounds: Some(Cartesian3::new(max_radius, max_angle, 0.5 * height + translation.z)),
        shape_transform: Matrix4::from_translation_quaternion_rotation_scale(
            translation,
            &rotation,
            Cartesian3::ONE,
        ),
    })
}

fn get_tile_count(metadata: &Cesium3DTilesetMetadata) -> Option<usize> {
    let tileset = metadata.tileset()?;
    let value = tileset.get_property_by_semantic(MetadataSemantic::TilesetTileCount.as_str()).ok()??;
    match value {
        MetadataValue::Number(number) => Some(number.max(0.0) as usize),
        MetadataValue::Array(values) => values.first().map(|v| v.to_number().max(0.0) as usize),
        _ => None,
    }
}

fn get_attribute_info(
    metadata: &Cesium3DTilesetMetadata,
    class_name: &str,
) -> Result<Cesium3DTilesVoxelProviderOptions> {
    let Some(class) = metadata.schema().classes.get(class_name) else {
        return Err(RuntimeError::new(format!("Tileset schema has no class \"{class_name}\"")));
    };
    let mut names = Vec::new();
    let mut types = Vec::new();
    let mut component_types = Vec::new();
    let mut minimum_values = Vec::new();
    let mut maximum_values = Vec::new();
    let class_statistics = metadata.statistics().and_then(|statistics| statistics["classes"].get(class_name));
    for (id, property) in class.properties.iter() {
        names.push(id.to_string());
        types.push(property.metadata_type());
        component_types.push(property.component_type().unwrap_or(MetadataComponentType::Float32));
        let component_count = property.metadata_type().get_component_count();
        let statistics = class_statistics.and_then(|statistics| statistics["properties"].get(id));
        minimum_values.push(copy_array(statistics.and_then(|s| s.get("min")), component_count));
        maximum_values.push(copy_array(statistics.and_then(|s| s.get("max")), component_count));
    }
    let has_minimum_values = minimum_values.iter().any(Option::is_some);
    let minimum_values = has_minimum_values.then(|| minimum_values.into_iter().map(|v| v.unwrap_or_default()).collect());
    let maximum_values = has_minimum_values.then(|| maximum_values.into_iter().map(|v| v.unwrap_or_default()).collect());
    Ok(Cesium3DTilesVoxelProviderOptions {
        class_name: class_name.into(),
        names,
        types,
        component_types,
        minimum_values,
        maximum_values,
        ..Default::default()
    })
}

fn copy_array(values: Option<&Value>, length: usize) -> Option<Vec<f64>> {
    let values = values?;
    let array = values.as_array().cloned().unwrap_or_else(|| vec![values.clone()]);
    Some((0..length).map(|index| array.get(index).and_then(Value::as_f64).unwrap_or(f64::NAN)).collect())
}

fn unpack_cartesian3(value: &Value) -> Cartesian3 {
    let number = |index: usize| value.get(index).and_then(Value::as_f64).unwrap_or(f64::NAN);
    Cartesian3::new(number(0), number(1), number(2))
}
