//! CesiumJS `Cesium3DTilesVoxelProvider`: a voxel provider backed by a 3D
//! Tiles tileset with implicit tiling and `3DTILES_content_voxels`.
use super::{
    has_extension, Cesium3DTilesetMetadata, ImplicitSubtree, ImplicitSubtreeCache, ImplicitTileCoordinates,
    ImplicitTileset, MetadataEntity, MetadataError, MetadataSemantic, MetadataType, MetadataValue, SubtreeSource,
    VoxelContent, VoxelMetadataOrder, VoxelProvider, VoxelRequest, VoxelShapeType,
};
use crate::core::{
    Cartesian3, Ellipsoid, Matrix3, Matrix4, OrientedBoundingBox, Quaternion, Resource, ResourceClient,
    RuntimeError,
};
use serde_json::Value;
use std::{collections::BTreeMap, rc::Rc};

type Result<T> = std::result::Result<T, RuntimeError>;

/// Constructor options for [`Cesium3DTilesVoxelProvider`].
#[derive(Clone, Debug)]
pub struct Cesium3DTilesVoxelProviderOptions {
    pub class_name: String,
    pub names: Vec<String>,
    pub types: Vec<MetadataType>,
    pub component_types: Vec<super::MetadataComponentType>,
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
    component_types: Vec<super::MetadataComponentType>,
    metadata_order: VoxelMetadataOrder,
    minimum_values: Option<Vec<Vec<f64>>>,
    maximum_values: Option<Vec<Vec<f64>>>,
    maximum_tile_count: Option<usize>,
    available_levels: Option<u32>,
    implicit_tileset: Option<ImplicitTileset>,
    subtree_cache: ImplicitSubtreeCache<Rc<ImplicitSubtree>>,
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
            subtree_cache: ImplicitSubtreeCache::new(None),
            client: None,
        })
    }

    pub fn class_name(&self) -> &str {
        &self.class_name
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
        let schema_json = if let Some(schema) = tileset_json.get("schema").filter(|v| !v.is_null()) {
            schema
        } else if let Some(uri) = tileset_json.get("schemaUri").and_then(Value::as_str) {
            let schema_resource = resource
                .get_derived_resource(crate::core::resource::DerivedResourceOptions { url: Some(uri.into()), ..Default::default() })
                .ok_or_else(|| RuntimeError::new("Invalid schema URI"))?;
            &client
                .fetch_json(&schema_resource)
                .ok_or_else(|| RuntimeError::new("Schema request was throttled"))?
                .await
                .map_err(|error| RuntimeError::new(error.to_string()))?
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
        if options.keyframe != 0 {
            return Err(RuntimeError::new("3D Tiles currently doesn't support time-dynamic data."));
        }
        let Some(implicit_tileset) = &self.implicit_tileset else {
            return Err(RuntimeError::new("Provider has no implicit tileset"));
        };
        let client = self
            .client
            .clone()
            .ok_or_else(|| RuntimeError::new("Provider has no resource client"))?;
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
        let subtree = self.get_subtree(&subtree_coord, &client).await?;
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
        let values = coordinates
            .get_template_values()
            .into_iter()
            .collect::<BTreeMap<_, _>>();
        let _ = values;
        // The content loader parses glTF voxel attributes; that GPU-free
        // extraction is not yet implemented in this port.
        Err(RuntimeError::new(
            "Voxel glTF content decoding is not implemented in the Rust port",
        ))
    }

    async fn get_subtree(
        &mut self,
        coordinates: &ImplicitTileCoordinates,
        client: &ResourceClient,
    ) -> Result<Rc<ImplicitSubtree>> {
        if let Some(subtree) = self.subtree_cache.find(coordinates) {
            return Ok(subtree.clone());
        }
        let implicit_tileset = self
            .implicit_tileset
            .as_ref()
            .ok_or_else(|| RuntimeError::new("Provider has no implicit tileset"))?;
        let template_values: Vec<(String, String)> = coordinates
            .get_template_values()
            .into_iter()
            .map(|(name, value)| (name.to_string(), value.to_string()))
            .collect();
        let subtree_relative = implicit_tileset
            .subtree_uri_template
            .get_derived_resource(crate::core::resource::DerivedResourceOptions {
                template_values,
                ..Default::default()
            })
            .ok_or_else(|| RuntimeError::new("Invalid subtree URI template"))?;
        let subtree_resource = implicit_tileset
            .base_resource
            .get_derived_resource(crate::core::resource::DerivedResourceOptions {
                url: Some(subtree_relative.url()),
                ..Default::default()
            })
            .ok_or_else(|| RuntimeError::new("Invalid subtree URL"))?;
        let bytes = client
            .fetch_array_buffer(&subtree_resource)
            .ok_or_else(|| RuntimeError::new("Subtree request was throttled"))?
            .await
            .map_err(|error| RuntimeError::new(error.to_string()))?;
        let mut load_buffer = |resource: &Resource| -> std::result::Result<Vec<u8>, MetadataError> {
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
                SubtreeSource::Binary(&bytes),
                implicit_tileset,
                *coordinates,
                &mut load_buffer,
            )
            .map_err(|error| RuntimeError::new(error.to_string()))?,
        );
        let _ = self.subtree_cache.add_subtree(subtree.clone());
        Ok(subtree)
    }
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
    fn component_types(&self) -> &[super::MetadataComponentType] {
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
        let coordinates = ImplicitTileCoordinates::new(
            implicit_tileset.subdivision_scheme,
            implicit_tileset.subtree_levels,
            options.tile_level as i64,
            options.tile_x as i64,
            options.tile_y as i64,
            Some(options.tile_z as i64),
        )
        .ok()?;
        let is_subtree_root = coordinates.is_subtree_root() && coordinates.level > 0;
        let subtree_coord = if is_subtree_root {
            coordinates.get_parent_subtree_coordinates().ok()?
        } else {
            coordinates.get_subtree_coordinates().ok()?
        };
        let template_values = subtree_coord.get_template_values();
        let base = implicit_tileset.base_resource.clone();
        let template = implicit_tileset.subtree_uri_template.clone();
        Some(Box::pin(async move {
            // Without a cached subtree this request needs the resource client;
            // the async path is `request_data_async`, so report the deferred
            // form rather than silently succeeding.
            let _ = (&client, &base, &template, &template_values);
            Err(RuntimeError::new(
                "Cesium3DTilesVoxelProvider::request_data requires request_data_async in the Rust port",
            ))
        }))
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
    if !has_extension(root, "3DTILES_implicit_tiling")
        && root.get("implicitTiling").is_none_or(Value::is_null)
    {
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
        component_types.push(property.component_type().unwrap_or(super::MetadataComponentType::Float32));
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
