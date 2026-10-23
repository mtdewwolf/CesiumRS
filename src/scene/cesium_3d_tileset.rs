//! Tileset JSON, content lifecycle, pass orchestration and memory management.
//! Tile content decoders/renderers implement `TilesetContentLoader`; content
//! stays in Rust and contributes executable Scene commands. Advanced skip-LOD
//! and implicit-subtree expansion are separate dependencies (docs/scene.md).
use super::{
    scene::{Result, ScenePrimitive},
    *,
};
use crate::core::resource::DerivedResourceOptions;
use crate::core::*;
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::{
    cell::{Cell, RefCell},
    future::Future,
    pin::Pin,
    rc::Rc,
    sync::Arc,
    task::{Context, Poll, Waker},
};
pub type TileId = usize;
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TileContentMetrics {
    pub features: u64,
    pub points: u64,
    pub triangles: u64,
    pub geometry_bytes: u64,
    pub texture_bytes: u64,
    pub batch_table_bytes: u64,
}
impl TileContentMetrics {
    pub fn memory(&self) -> u64 {
        self.geometry_bytes.saturating_add(self.texture_bytes).saturating_add(self.batch_table_bytes)
    }
}
/// Concrete content owns GPU resources and/or CPU meshes. A content loader
/// reports failure through Result; dropping an in-flight request cancels it.
pub trait TilesetContent {
    fn metrics(&self) -> TileContentMetrics;
    fn process(&mut self, frame: &FrameState) -> Result<bool>;
    fn update(&mut self, frame: &mut FrameState) -> Result<()>;
    fn pick(&self, ray: &Ray, frame: &FrameState) -> Result<Option<Cartesian3>>;
    fn destroy(&mut self) -> Result<()>;
}
pub type TileContentFuture = Pin<Box<dyn Future<Output = Result<Box<dyn TilesetContent>>>>>;
#[derive(Clone)]
pub struct TileContentRequest {
    pub tile: TileId,
    pub resource: Resource,
    pub cancelled: Rc<Cell<bool>>,
    pub header: Value,
}
pub trait TilesetContentLoader {
    fn request(&self, request: TileContentRequest) -> Option<TileContentFuture>;
}

#[derive(Clone, Debug, Deserialize, Serialize)]
#[serde(default, rename_all = "camelCase")]
pub struct TilesetOptions {
    pub maximum_screen_space_error: f64,
    pub cache_bytes: u64,
    pub maximum_cache_overflow_bytes: u64,
    pub show: bool,
    pub cull_with_children_bounds: bool,
    pub cull_requests_while_moving: bool,
    pub cull_requests_while_moving_multiplier: f64,
    pub progressive_resolution_height_fraction: f64,
    pub prefer_leaves: bool,
    pub preload_when_hidden: bool,
    pub preload_flight_destinations: bool,
    pub dynamic_screen_space_error: bool,
    pub dynamic_screen_space_error_density: f64,
    pub dynamic_screen_space_error_factor: f64,
    pub dynamic_screen_space_error_height_falloff: f64,
    pub foveated_screen_space_error: bool,
    pub foveated_cone_size: f64,
    pub foveated_minimum_screen_space_error_relaxation: f64,
    pub foveated_time_delay: f64,
    pub skip_level_of_detail: bool,
    pub base_screen_space_error: f64,
    pub skip_screen_space_error_factor: f64,
    pub skip_levels: u32,
    pub immediately_load_desired_level_of_detail: bool,
    pub load_siblings: bool,
    pub enable_collision: bool,
    pub enable_pick: bool,
    pub show_credits_on_screen: bool,
}
impl Default for TilesetOptions {
    fn default() -> Self {
        Self {
            maximum_screen_space_error: 16.,
            cache_bytes: 512 * 1024 * 1024,
            maximum_cache_overflow_bytes: 512 * 1024 * 1024,
            show: true,
            cull_with_children_bounds: true,
            cull_requests_while_moving: true,
            cull_requests_while_moving_multiplier: 60.,
            progressive_resolution_height_fraction: 0.3,
            prefer_leaves: false,
            preload_when_hidden: false,
            preload_flight_destinations: true,
            dynamic_screen_space_error: true,
            dynamic_screen_space_error_density: 2e-4,
            dynamic_screen_space_error_factor: 24.,
            dynamic_screen_space_error_height_falloff: 0.25,
            foveated_screen_space_error: true,
            foveated_cone_size: 0.1,
            foveated_minimum_screen_space_error_relaxation: 0.,
            foveated_time_delay: 0.2,
            skip_level_of_detail: false,
            base_screen_space_error: 1024.,
            skip_screen_space_error_factor: 16.,
            skip_levels: 1,
            immediately_load_desired_level_of_detail: false,
            load_siblings: false,
            enable_collision: false,
            enable_pick: false,
            show_credits_on_screen: false,
        }
    }
}
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct TilesetStatistics {
    pub selected: u64,
    pub visited: u64,
    pub number_of_commands: u64,
    pub number_of_attempted_requests: u64,
    pub number_of_pending_requests: u64,
    pub number_of_tiles_processing: u64,
    pub number_of_tiles_with_content_ready: u64,
    pub number_of_tiles_total: u64,
    pub number_of_loaded_tiles_total: u64,
    pub number_of_features_selected: u64,
    pub number_of_points_selected: u64,
    pub number_of_triangles_selected: u64,
    pub number_of_features_loaded: u64,
    pub number_of_points_loaded: u64,
    pub geometry_byte_length: u64,
    pub textures_byte_length: u64,
    pub batch_table_byte_length: u64,
}
impl TilesetStatistics {
    pub fn clear(&mut self) {
        self.selected = 0;
        self.visited = 0;
        self.number_of_commands = 0;
        self.number_of_attempted_requests = 0;
        self.number_of_features_selected = 0;
        self.number_of_points_selected = 0;
        self.number_of_triangles_selected = 0;
    }
    pub fn total_memory_usage_in_bytes(&self) -> u64 {
        self.geometry_byte_length.saturating_add(self.textures_byte_length).saturating_add(self.batch_table_byte_length)
    }
    fn load(&mut self, m: TileContentMetrics, add: bool) {
        let apply = |v: &mut u64, n: u64| *v = if add { v.saturating_add(n) } else { v.saturating_sub(n) };
        apply(&mut self.geometry_byte_length, m.geometry_bytes);
        apply(&mut self.textures_byte_length, m.texture_bytes);
        apply(&mut self.batch_table_byte_length, m.batch_table_bytes);
        apply(&mut self.number_of_features_loaded, m.features);
        apply(&mut self.number_of_points_loaded, m.points);
    }
}
#[derive(Clone, Debug)]
pub enum TilesetEvent {
    TileLoad(TileId),
    TileUnload(TileId),
    TileVisible(TileId),
    TileFailed { tile: TileId, url: Option<String>, message: String },
    LoadProgress { pending: u64, processing: u64 },
    AllTilesLoaded,
    InitialTilesLoaded,
}
pub struct TilesetTile {
    pub parent: Option<TileId>,
    pub children: Vec<TileId>,
    pub header: Value,
    pub depth: u32,
    pub transform: Matrix4,
    pub computed_transform: Matrix4,
    pub refine: Cesium3DTileRefine,
    pub geometric_error: f64,
    pub bounding_sphere: BoundingSphere,
    pub content_resource: Option<Resource>,
    pub state: Cesium3DTileContentState,
    pub content: Option<Box<dyn TilesetContent>>,
    pub priority: f64,
    pub touched_frame: u32,
    pub distance_to_camera: f64,
    pub implicit_tileset: Option<ImplicitTileset>,
    pub destroyed: bool,
    source_bounds: Value,
}
impl TilesetTile {
    pub fn has_empty_content(&self) -> bool {
        self.content_resource.is_none()
    }
    pub fn content_ready(&self) -> bool {
        self.state == Cesium3DTileContentState::Ready
    }
}
struct PendingContent {
    id: TileId,
    future: TileContentFuture,
    cancelled: Rc<Cell<bool>>,
}
/// A sentinel separates reusable LRU entries from the tiles used this frame.
#[derive(Default)]
pub struct TilesetCache {
    entries: Vec<TileId>,
    sentinel: usize,
    trim: bool,
}
impl TilesetCache {
    pub fn reset(&mut self) {
        self.sentinel = self.entries.len();
    }
    pub fn add(&mut self, id: TileId) {
        if !self.entries.contains(&id) {
            self.entries.push(id);
        }
    }
    pub fn touch(&mut self, id: TileId) {
        if let Some(i) = self.entries.iter().position(|x| *x == id) {
            self.entries.remove(i);
            if i < self.sentinel {
                self.sentinel -= 1;
            }
            self.entries.insert(self.sentinel, id);
        }
    }
    pub fn remove(&mut self, id: TileId) -> bool {
        if let Some(i) = self.entries.iter().position(|x| *x == id) {
            self.entries.remove(i);
            if i < self.sentinel {
                self.sentinel -= 1;
            }
            true
        } else {
            false
        }
    }
    pub fn trim(&mut self) {
        self.trim = true;
    }
    pub fn entries(&self) -> &[TileId] {
        &self.entries
    }
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TilesetTraversalKind {
    Base,
    Skip,
    MostDetailed,
}
pub struct TilesetPassState {
    pub pass: Cesium3DTilePass,
    pub ready: bool,
    pub camera: Option<crate::renderer::UniformCamera>,
    pub culling_volume: Option<CullingVolume>,
}
impl TilesetPassState {
    pub fn new(pass: Cesium3DTilePass) -> Self {
        Self { pass, ready: false, camera: None, culling_volume: None }
    }
}

pub struct Cesium3DTileset {
    pub options: TilesetOptions,
    pub model_matrix: Matrix4,
    pub asset: Value,
    pub extras: Option<Value>,
    pub properties: Option<Value>,
    pub extensions: Option<Value>,
    pub extensions_used: Vec<String>,
    pub metadata_extension: Option<Cesium3DTilesetMetadata>,
    pub resource: Resource,
    pub url: String,
    pub base_path: Option<String>,
    pub root: Option<TileId>,
    pub tiles: Vec<TilesetTile>,
    pub statistics: TilesetStatistics,
    pub statistics_per_pass: [TilesetStatistics; 8],
    pub selected_tiles: Vec<TileId>,
    pub requested_tiles: Vec<TileId>,
    pub events: Rc<RefCell<Event<TilesetEvent>>>,
    pub cache: TilesetCache,
    pub geometric_error: f64,
    pub dynamic_screen_space_error_computed_density: f64,
    pub time_since_load: f64,
    pub classification_type: Option<ClassificationType>,
    pub disable_skip_level_of_detail: bool,
    pub all_tiles_additive: bool,
    pub model_up_axis: Axis,
    pub model_forward_axis: Axis,
    pub feature_id_label: String,
    pub instance_feature_id_label: String,
    loader: Option<Rc<dyn TilesetContentLoader>>,
    pending: Vec<PendingContent>,
    processing: Vec<TileId>,
    memory_adjusted_screen_space_error: f64,
    load_timestamp: Option<JulianDate>,
    previous_model_matrix: Option<Matrix4>,
    updated_model_matrix_frame: Option<u32>,
    model_matrix_changed: bool,
    cull_requests_while_moving: bool,
    tiles_loaded: bool,
    initial_tiles_loaded: bool,
    last_progress: (u64, u64),
    initial_clipping_planes_origin_matrix: Matrix4,
    alive: Rc<Cell<bool>>,
    credits: Vec<Credit>,
}
fn number(v: &Value, key: &str, default: f64) -> f64 {
    v.get(key).and_then(Value::as_f64).unwrap_or(default)
}
fn array<const N: usize>(v: &Value) -> Result<[f64; N]> {
    let values =
        v.as_array().filter(|a| a.len() == N).ok_or_else(|| RuntimeError::new("invalid 3D Tiles numeric array"))?;
    let mut out = [0.; N];
    for (i, v) in values.iter().enumerate() {
        out[i] = v.as_f64().ok_or_else(|| RuntimeError::new("invalid 3D Tiles numeric component"))?;
    }
    Ok(out)
}
fn bounds(v: &Value, transform: Matrix4, ellipsoid: &Ellipsoid) -> Result<BoundingSphere> {
    if let Some(s) = v.pointer("/extensions/3DTILES_bounding_volume_S2") {
        let volume = TileBoundingS2Cell::new(tile_bounding_s2_cell::TileBoundingS2CellOptions {
            token: s["token"].as_str().ok_or_else(|| RuntimeError::new("S2 token is required"))?.into(),
            minimum_height: s["minimumHeight"].as_f64(),
            maximum_height: s["maximumHeight"].as_f64(),
            ellipsoid: Some(*ellipsoid),
        })
        .ok_or_else(|| RuntimeError::new("invalid S2 tile bounding volume"))?;
        return Ok(*volume.bounding_sphere());
    }
    if let Some(s) = v.get("sphere") {
        let [x, y, z, r] = array::<4>(s)?;
        return Ok(BoundingSphere::new(Cartesian3::new(x, y, z), if r == 0. { 1e-7 } else { r }).transform(&transform));
    }
    if let Some(b) = v.get("box") {
        let a = array::<12>(b)?;
        let center = transform.multiply_by_point(Cartesian3::new(a[0], a[1], a[2]));
        let half = transform.get_matrix3().multiply(&Matrix3::from_array(&a[3..12], 0));
        return Ok(*TileOrientedBoundingBox::new(center, half).bounding_sphere());
    }
    if let Some(r) = v.get("region") {
        let [w, s, e, n, min, max] = array::<6>(r)?;
        let mut opts = tile_bounding_region::TileBoundingRegionOptions::new(Rectangle::new(w, s, e, n));
        opts.minimum_height = Some(min);
        opts.maximum_height = Some(max);
        opts.ellipsoid = Some(*ellipsoid);
        return TileBoundingRegion::new(opts)
            .bounding_sphere_opt()
            .copied()
            .ok_or_else(|| RuntimeError::new("invalid tile region"));
    }
    Err(RuntimeError::new("tile boundingVolume must contain a sphere, box or region"))
}
impl Cesium3DTileset {
    pub const SUPPORTED_EXTENSIONS: &'static [&'static str] = &[
        "3DTILES_metadata",
        "3DTILES_implicit_tiling",
        "3DTILES_content_gltf",
        "3DTILES_multiple_contents",
        "3DTILES_bounding_volume_S2",
        "3DTILES_batch_table_hierarchy",
        "3DTILES_draco_point_compression",
        "MAXAR_content_geojson",
    ];
    pub fn check_supported_extensions(names: &[String]) -> Result<()> {
        for name in names {
            if !Self::SUPPORTED_EXTENSIONS.contains(&name.as_str()) {
                return Err(RuntimeError::new(format!("Unsupported 3D Tiles Extension: {name}")));
            }
        }
        Ok(())
    }
    pub fn from_json(
        resource: Resource,
        json: &Value,
        mut options: TilesetOptions,
        loader: Option<Rc<dyn TilesetContentLoader>>,
    ) -> Result<Self> {
        let asset =
            json.get("asset").cloned().ok_or_else(|| RuntimeError::new("Tileset must have an asset property."))?;
        if !matches!(asset["version"].as_str(), Some("0.0" | "1.0" | "1.1")) {
            return Err(RuntimeError::new("The tileset must be 3D Tiles version 0.0, 1.0, or 1.1"));
        }
        let required = json["extensionsRequired"]
            .as_array()
            .map(|a| a.iter().filter_map(Value::as_str).map(str::to_owned).collect::<Vec<_>>())
            .unwrap_or_default();
        Self::check_supported_extensions(&required)?;
        if options.maximum_screen_space_error < 0.
            || !(0. ..=1.).contains(&options.foveated_cone_size)
            || options.foveated_minimum_screen_space_error_relaxation < 0.
            || options.foveated_minimum_screen_space_error_relaxation > options.maximum_screen_space_error
        {
            return Err(RuntimeError::new("invalid tileset screen-space-error options"));
        }
        options.progressive_resolution_height_fraction = options.progressive_resolution_height_fraction.clamp(0., 0.5);
        let url = resource.url();
        let base_path = if resource.extension() == "json" {
            Some(resource.get_base_uri(true))
        } else if resource.is_data_uri() {
            Some(String::new())
        } else {
            None
        };
        let metadata_json = json.get("extensions").and_then(|x| x.get("3DTILES_metadata")).unwrap_or(json);
        let metadata_extension = if let Some(schema) = metadata_json.get("schema") {
            let schema = Arc::new(MetadataSchema::from_json(schema).map_err(|e| RuntimeError::new(e.to_string()))?);
            Some(Cesium3DTilesetMetadata::new(metadata_json, schema).map_err(|e| RuntimeError::new(e.to_string()))?)
        } else {
            None
        };
        let mut credits = resource.options().credits.clone().unwrap_or_default();
        if options.show_credits_on_screen {
            for credit in &mut credits {
                credit.show_on_screen = true;
            }
        }
        if let Some(extra) = asset.pointer("/extras/cesium/credits").and_then(Value::as_array) {
            for credit in extra {
                if let Some(html) = credit["html"].as_str() {
                    credits.push(Credit::new(html, options.show_credits_on_screen));
                }
            }
        }
        let mut out = Self {
            memory_adjusted_screen_space_error: options.maximum_screen_space_error,
            options,
            model_matrix: Matrix4::IDENTITY,
            asset,
            extras: json.get("extras").cloned(),
            properties: json.get("properties").cloned(),
            extensions: json.get("extensions").cloned(),
            extensions_used: json["extensionsUsed"]
                .as_array()
                .map(|a| a.iter().filter_map(Value::as_str).map(str::to_owned).collect())
                .unwrap_or_default(),
            metadata_extension,
            resource,
            url,
            base_path,
            root: None,
            tiles: Vec::new(),
            statistics: TilesetStatistics::default(),
            statistics_per_pass: std::array::from_fn(|_| TilesetStatistics::default()),
            selected_tiles: Vec::new(),
            requested_tiles: Vec::new(),
            events: Rc::new(RefCell::new(Event::new())),
            cache: TilesetCache::default(),
            geometric_error: number(json, "geometricError", f64::NAN),
            dynamic_screen_space_error_computed_density: 0.,
            time_since_load: 0.,
            classification_type: None,
            disable_skip_level_of_detail: false,
            all_tiles_additive: true,
            model_up_axis: Axis::Y,
            model_forward_axis: Axis::X,
            feature_id_label: "featureId_0".into(),
            instance_feature_id_label: "instanceFeatureId_0".into(),
            loader,
            pending: Vec::new(),
            processing: Vec::new(),
            load_timestamp: None,
            previous_model_matrix: None,
            updated_model_matrix_frame: None,
            model_matrix_changed: false,
            cull_requests_while_moving: false,
            tiles_loaded: false,
            initial_tiles_loaded: false,
            last_progress: (0, 0),
            initial_clipping_planes_origin_matrix: Matrix4::IDENTITY,
            alive: Rc::new(Cell::new(true)),
            credits,
        };
        if let Some(axis) = out.asset["gltfUpAxis"].as_str().and_then(Axis::from_name) {
            out.model_up_axis = axis;
        }
        let root = out.load_tileset(out.resource.clone(), json, None)?;
        out.root = Some(root);
        let center = bounds(&out.tiles[root].source_bounds, Matrix4::IDENTITY, &Ellipsoid::WGS84)?.center;
        if Ellipsoid::WGS84.cartesian_to_cartographic(center).is_some_and(|c| c.height > -100000.) {
            out.initial_clipping_planes_origin_matrix =
                Transforms::east_north_up_to_fixed_frame(center, &Ellipsoid::WGS84);
        }
        Ok(out)
    }
    pub async fn from_url(
        resource: Resource,
        client: &ResourceClient,
        options: TilesetOptions,
        loader: Option<Rc<dyn TilesetContentLoader>>,
    ) -> Result<Self> {
        let mut json = client
            .fetch_json(&resource)
            .ok_or_else(|| RuntimeError::new("tileset request was throttled"))?
            .await
            .map_err(|e| RuntimeError::new(e.to_string()))?;
        let metadata = json.pointer("/extensions/3DTILES_metadata").unwrap_or(&json);
        if let Some(uri) = metadata["schemaUri"].as_str() {
            let schema_resource = resource
                .get_derived_resource(DerivedResourceOptions {
                    url: Some(uri.into()),
                    ..DerivedResourceOptions::default()
                })
                .ok_or_else(|| RuntimeError::new("invalid schemaUri"))?;
            let schema = client
                .fetch_json(&schema_resource)
                .ok_or_else(|| RuntimeError::new("schema request was throttled"))?
                .await
                .map_err(|e| RuntimeError::new(e.to_string()))?;
            if let Some(metadata) = json.pointer_mut("/extensions/3DTILES_metadata") {
                metadata["schema"] = schema;
            } else {
                json["schema"] = schema;
            }
        }
        Self::from_json(resource, &json, options, loader)
    }
    pub fn load_tileset(&mut self, resource: Resource, json: &Value, parent: Option<TileId>) -> Result<TileId> {
        let asset = json.get("asset").ok_or_else(|| RuntimeError::new("Tileset must have an asset property."))?;
        if !matches!(asset["version"].as_str(), Some("0.0" | "1.0" | "1.1")) {
            return Err(RuntimeError::new("The tileset must be 3D Tiles version 0.0, 1.0, or 1.1"));
        }
        if let Some(names) = json["extensionsRequired"].as_array() {
            Self::check_supported_extensions(
                &names.iter().filter_map(Value::as_str).map(str::to_owned).collect::<Vec<_>>(),
            )?;
        }
        if let Some(version) = asset.get("tilesetVersion") {
            let version = version.as_str().map(str::to_owned).unwrap_or_else(|| version.to_string());
            self.base_path = Some(format!("{}?v={version}", self.base_path.as_deref().unwrap_or("undefined")));
            resource.set_query_parameters(
                &vec![("v".into(), Some(crate::core::query::QueryValue::Single(version)))],
                false,
            );
        }
        let header = json.get("root").ok_or_else(|| RuntimeError::new("Tileset must have a root property."))?;
        let root = self.make_tile(&resource, header, parent)?;
        let mut stack = vec![(root, header)];
        while let Some((id, header)) = stack.pop() {
            if let Some(children) = header["children"].as_array() {
                for child in children {
                    let child_id = self.make_tile(&resource, child, Some(id))?;
                    stack.push((child_id, child));
                }
            }
        }
        Ok(root)
    }
    fn make_tile(&mut self, resource: &Resource, header: &Value, parent: Option<TileId>) -> Result<TileId> {
        let transform = header.get("transform").map(array::<16>).transpose()?.map(Matrix4).unwrap_or(Matrix4::IDENTITY);
        let parent_transform = parent.map_or(self.model_matrix, |i| self.tiles[i].computed_transform);
        let computed_transform = parent_transform.multiply(&transform);
        let refine = match header["refine"].as_str() {
            Some("ADD" | "add") => Cesium3DTileRefine::Add,
            Some("REPLACE" | "replace") => Cesium3DTileRefine::Replace,
            _ => parent.map_or(Cesium3DTileRefine::Replace, |i| self.tiles[i].refine),
        };
        let geometric_error =
            number(header, "geometricError", parent.map_or(self.geometric_error, |i| self.tiles[i].geometric_error));
        let source_bounds = header
            .get("boundingVolume")
            .cloned()
            .ok_or_else(|| RuntimeError::new("tile boundingVolume is required"))?;
        let bounding_sphere = bounds(&source_bounds, computed_transform, &Ellipsoid::WGS84)?;
        let implicit =
            header.get("implicitTiling").is_some() || header.pointer("/extensions/3DTILES_implicit_tiling").is_some();
        let implicit_tileset = if implicit {
            Some(
                ImplicitTileset::new(
                    resource.clone(),
                    header,
                    self.metadata_extension.as_ref().map(|m| Arc::clone(m.schema())),
                )
                .map_err(|e| RuntimeError::new(e.to_string()))?,
            )
        } else {
            None
        };
        let content_resource = if let Some(implicit) = &implicit_tileset {
            implicit.subtree_uri_template.get_derived_resource(DerivedResourceOptions {
                template_values: vec![
                    ("level".into(), "0".into()),
                    ("x".into(), "0".into()),
                    ("y".into(), "0".into()),
                    ("z".into(), "0".into()),
                ],
                ..DerivedResourceOptions::default()
            })
        } else {
            let content = header.get("content").or_else(|| header["contents"].as_array().and_then(|a| a.first()));
            content.and_then(|c| c.get("uri").or(c.get("url"))).and_then(Value::as_str).and_then(|uri| {
                resource.get_derived_resource(DerivedResourceOptions {
                    url: Some(uri.into()),
                    ..DerivedResourceOptions::default()
                })
            })
        };
        let state = if content_resource.is_none() {
            Cesium3DTileContentState::Ready
        } else {
            Cesium3DTileContentState::Unloaded
        };
        let id = self.tiles.len();
        let depth = parent.map_or(0, |i| self.tiles[i].depth + 1);
        self.tiles.push(TilesetTile {
            parent,
            children: Vec::new(),
            header: header.clone(),
            depth,
            transform,
            computed_transform,
            refine,
            geometric_error,
            bounding_sphere,
            content_resource,
            state,
            content: None,
            priority: 0.,
            touched_frame: 0,
            distance_to_camera: 0.,
            implicit_tileset,
            destroyed: false,
            source_bounds,
        });
        if let Some(parent) = parent {
            self.tiles[parent].children.push(id);
        }
        self.statistics.number_of_tiles_total += 1;
        self.all_tiles_additive &= refine == Cesium3DTileRefine::Add;
        Ok(id)
    }
    pub fn has_extension(&self, name: &str) -> bool {
        self.extensions_used.iter().any(|x| x == name)
    }
    pub fn is_gltf_extension_used(&self, name: &str) -> bool {
        self.has_extension("3DTILES_content_gltf")
            && self
                .extensions
                .as_ref()
                .and_then(|e| e.pointer("/3DTILES_content_gltf/extensionsUsed"))
                .and_then(Value::as_array)
                .is_some_and(|a| a.iter().any(|v| v == name))
    }
    pub fn is_gltf_extension_required(&self, name: &str) -> bool {
        self.is_gltf_extension_used(name)
            && self
                .extensions
                .as_ref()
                .and_then(|e| e.pointer("/3DTILES_content_gltf/extensionsRequired"))
                .and_then(Value::as_array)
                .is_some_and(|a| a.iter().any(|v| v == name))
    }
    pub fn is_skipping_level_of_detail(&self) -> bool {
        self.options.skip_level_of_detail
            && self.classification_type.is_none()
            && !self.disable_skip_level_of_detail
            && !self.all_tiles_additive
    }
    pub fn traversal_kind(&self, pass: Cesium3DTilePass) -> TilesetTraversalKind {
        if matches!(pass, Cesium3DTilePass::MostDetailedPick | Cesium3DTilePass::MostDetailedPreload) {
            TilesetTraversalKind::MostDetailed
        } else if self.is_skipping_level_of_detail() {
            TilesetTraversalKind::Skip
        } else {
            TilesetTraversalKind::Base
        }
    }
    pub fn set_maximum_screen_space_error(&mut self, value: f64) -> Result<()> {
        if value < 0. {
            return Err(RuntimeError::new("maximumScreenSpaceError must be nonnegative"));
        }
        self.options.maximum_screen_space_error = value;
        self.memory_adjusted_screen_space_error = value;
        Ok(())
    }
    pub fn memory_adjusted_screen_space_error(&self) -> f64 {
        self.memory_adjusted_screen_space_error
    }
    pub fn tiles_loaded(&self) -> bool {
        self.tiles_loaded
    }
    pub fn is_destroyed(&self) -> bool {
        !self.alive.get()
    }
    pub fn bounding_sphere(&mut self) -> Result<Option<BoundingSphere>> {
        self.update_transforms()?;
        Ok(self.root.map(|i| self.tiles[i].bounding_sphere))
    }
    pub fn clipping_planes_origin_matrix(&mut self) -> Result<Matrix4> {
        self.update_transforms()?;
        Ok(self.root.map_or(Matrix4::IDENTITY, |i| {
            self.tiles[i].computed_transform.multiply(&self.initial_clipping_planes_origin_matrix)
        }))
    }
    fn update_transforms(&mut self) -> Result<()> {
        for i in 0..self.tiles.len() {
            if self.tiles[i].destroyed {
                continue;
            }
            let parent = self.tiles[i].parent.map_or(self.model_matrix, |p| self.tiles[p].computed_transform);
            let transform = parent.multiply(&self.tiles[i].transform);
            self.tiles[i].computed_transform = transform;
            self.tiles[i].bounding_sphere = bounds(&self.tiles[i].source_bounds, transform, &Ellipsoid::WGS84)?;
        }
        Ok(())
    }
    fn emit(&self, event: TilesetEvent) {
        if self.alive.get() {
            self.events.borrow_mut().raise_event(&event);
        }
    }
    fn defer(&self, frame: &mut FrameState, event: TilesetEvent) {
        let events = Rc::clone(&self.events);
        let alive = Rc::clone(&self.alive);
        frame.after_render.push(Box::new(move || {
            if alive.get() {
                events.borrow_mut().raise_event(&event);
            }
            Ok(true)
        }));
    }
    pub fn install_content(&mut self, id: TileId, content: Box<dyn TilesetContent>) -> Result<()> {
        let tile = self.tiles.get_mut(id).ok_or_else(|| RuntimeError::new("invalid tile id"))?;
        if tile.destroyed {
            return Err(RuntimeError::new("tile destroyed"));
        }
        if tile.content.is_some() {
            return Err(RuntimeError::new("tile already has content; unload it before installing a replacement"));
        }
        tile.content = Some(content);
        tile.state = Cesium3DTileContentState::Processing;
        self.processing.push(id);
        self.statistics.number_of_tiles_processing += 1;
        Ok(())
    }
    pub fn poll_requests(&mut self) {
        self.poll_requests_with_waker(Waker::noop());
    }
    fn poll_requests_with_waker(&mut self, waker: &Waker) {
        let mut cx = Context::from_waker(waker);
        let mut i = 0;
        while i < self.pending.len() {
            let result = self.pending[i].future.as_mut().poll(&mut cx);
            if let Poll::Ready(result) = result {
                let pending = self.pending.remove(i);
                self.statistics.number_of_pending_requests =
                    self.statistics.number_of_pending_requests.saturating_sub(1);
                if self.is_destroyed() || self.tiles[pending.id].destroyed {
                    continue;
                }
                match result {
                    Ok(content) => {
                        let _ = self.install_content(pending.id, content);
                    }
                    Err(error) => {
                        self.tiles[pending.id].state = Cesium3DTileContentState::Failed;
                        self.emit(TilesetEvent::TileFailed {
                            tile: pending.id,
                            url: self.tiles[pending.id].content_resource.as_ref().map(Resource::url),
                            message: error.to_string(),
                        });
                    }
                }
            } else {
                i += 1;
            }
        }
    }
    pub fn pre_passes_update(&mut self, frame: &mut FrameState) -> Result<()> {
        if self.is_destroyed() {
            return Err(RuntimeError::new("tileset destroyed"));
        }
        if self.root.is_none() {
            return Ok(());
        }
        let waker = frame.render_request.as_ref().map(SceneRenderRequest::waker);
        self.poll_requests_with_waker(waker.as_ref().unwrap_or(Waker::noop()));
        self.processing
            .retain(|&id| !self.tiles[id].destroyed && self.tiles[id].state == Cesium3DTileContentState::Processing);
        let limit = self.options.cache_bytes.saturating_add(self.options.maximum_cache_overflow_bytes);
        let mut exceeded = false;
        for id in self.processing.clone() {
            if self.statistics.total_memory_usage_in_bytes() > limit {
                exceeded = true;
                break;
            }
            let tile = &mut self.tiles[id];
            let result = tile
                .content
                .as_mut()
                .ok_or_else(|| RuntimeError::new("processing tile has no content"))?
                .process(frame);
            match result {
                Ok(true) => {
                    tile.state = Cesium3DTileContentState::Ready;
                    let metrics = tile.content.as_ref().unwrap().metrics();
                    self.statistics.load(metrics, true);
                    self.statistics.number_of_tiles_processing =
                        self.statistics.number_of_tiles_processing.saturating_sub(1);
                    self.statistics.number_of_tiles_with_content_ready += 1;
                    self.statistics.number_of_loaded_tiles_total += 1;
                    self.cache.add(id);
                    self.emit(TilesetEvent::TileLoad(id));
                }
                Ok(false) => {}
                Err(error) => {
                    tile.state = Cesium3DTileContentState::Failed;
                    self.statistics.number_of_tiles_processing =
                        self.statistics.number_of_tiles_processing.saturating_sub(1);
                    self.emit(TilesetEvent::TileFailed {
                        tile: id,
                        url: self.tiles[id].content_resource.as_ref().map(Resource::url),
                        message: error.to_string(),
                    });
                }
            }
        }
        if self.statistics.total_memory_usage_in_bytes() < self.options.cache_bytes {
            self.memory_adjusted_screen_space_error =
                (self.memory_adjusted_screen_space_error / 1.02).max(self.options.maximum_screen_space_error);
        } else if exceeded && !self.processing.is_empty() {
            self.memory_adjusted_screen_space_error *= 1.02;
            self.processing.sort_by(|&a, &b| self.tiles[a].priority.total_cmp(&self.tiles[b].priority));
        }
        let start = *self.load_timestamp.get_or_insert(frame.time);
        self.time_since_load = (JulianDate::seconds_difference(&frame.time, &start) * 1000.).max(0.);
        if frame.new_frame {
            self.cache.reset();
        }
        self.update_transforms()?;
        if self.options.dynamic_screen_space_error {
            self.update_dynamic_screen_space_error(frame);
        }
        Ok(())
    }
    fn update_dynamic_screen_space_error(&mut self, frame: &FrameState) {
        let Some(root) = self.root else {
            return;
        };
        let tile = &self.tiles[root];
        let camera = &frame.uniform.camera;
        let region = tile.source_bounds.get("region").and_then(Value::as_array);
        let (up, direction, height, min, max) = if let Some(r) = region {
            (
                camera.position.normalize(),
                camera.direction,
                camera.cartographic.map_or(f64::NAN, |c| c.height),
                r[4].as_f64().unwrap_or(0.),
                r[5].as_f64().unwrap_or(0.),
            )
        } else {
            let inverse = tile.computed_transform.inverse_transformation();
            let center = inverse.multiply_by_point(tile.bounding_sphere.center);
            let ellipsoid = frame.uniform.map_projection.ellipsoid();
            if center.magnitude() > ellipsoid.minimum_radius {
                (
                    camera.position.normalize(),
                    camera.direction,
                    camera.cartographic.map_or(f64::NAN, |c| c.height),
                    0.,
                    ellipsoid.cartesian_to_cartographic(center).map_or(f64::NAN, |c| c.height * 2.),
                )
            } else {
                let position = inverse.multiply_by_point(camera.position);
                let direction = inverse.multiply_by_point_as_vector(camera.direction).normalize();
                let half = tile
                    .source_bounds
                    .get("box")
                    .and_then(|v| array::<12>(v).ok())
                    .map_or(tile.bounding_sphere.radius, |b| Cartesian3::new(b[9], b[10], b[11]).magnitude());
                (Cartesian3::UNIT_Z, direction, position.z, center.z - half, center.z + half)
            }
        };
        let close = min + (max - min) * self.options.dynamic_screen_space_error_height_falloff;
        let t = ((height - close) / (max - close)).clamp(0., 1.);
        self.dynamic_screen_space_error_computed_density =
            self.options.dynamic_screen_space_error_density * (1. - direction.dot(up).abs()) * (1. - t);
    }
    fn screen_space_error(&self, id: TileId, frame: &FrameState, error: f64) -> f64 {
        if error == 0. {
            return 0.;
        }
        let tile = &self.tiles[id];
        let error = error * tile.computed_transform.get_maximum_scale();
        let distance = (frame.uniform.camera.position.distance(tile.bounding_sphere.center)
            - tile.bounding_sphere.radius)
            .max(1e-7);
        let height = frame.drawing_buffer_height as f64;
        let perspective = match frame.uniform.camera.frustum {
            crate::renderer::UniformFrustum::Perspective(f) if frame.mode != SceneMode::Scene2D => {
                Some(f.sse_denominator())
            }
            crate::renderer::UniformFrustum::PerspectiveOffCenter(f) if frame.mode != SceneMode::Scene2D => {
                Some((f.top - f.bottom) / f.near)
            }
            _ => None,
        };
        let error = if let Some(denominator) = perspective {
            let dynamic = if self.options.dynamic_screen_space_error {
                crate::core::math::fog(distance, self.dynamic_screen_space_error_computed_density)
                    * self.options.dynamic_screen_space_error_factor
            } else {
                0.
            };
            error * height / (distance * denominator) - dynamic
        } else {
            let (_, _, _, planes) = frame.uniform.camera.frustum.parameters();
            let pixel_size =
                (planes[0] - planes[1]).max(planes[3] - planes[2]) / (frame.drawing_buffer_width as f64).max(height);
            error / pixel_size
        };
        error / frame.uniform.pixel_ratio
    }
    fn select(&mut self, id: TileId, frame: &FrameState, most_detailed: bool) -> bool {
        let tile = &mut self.tiles[id];
        if tile.destroyed || frame.culling_volume.compute_visibility(&tile.bounding_sphere) == Intersect::Outside {
            return true;
        }
        self.statistics.visited += 1;
        // Multiple passes in the same frame must not reorder the cache twice.
        if tile.touched_frame != frame.frame_number {
            tile.touched_frame = frame.frame_number;
            self.cache.touch(id);
        }
        tile.distance_to_camera =
            (frame.uniform.camera.position.distance(tile.bounding_sphere.center) - tile.bounding_sphere.radius).max(0.);
        let mut children = tile.children.clone();
        let refine = tile.refine;
        let ready = tile.content_ready();
        let empty = tile.has_empty_content();
        let error = tile.geometric_error;
        let can_request = matches!(tile.state, Cesium3DTileContentState::Unloaded | Cesium3DTileContentState::Expired);
        let sse = self.screen_space_error(id, frame, error);
        let refine_more = !children.is_empty() && (most_detailed || sse > self.memory_adjusted_screen_space_error);
        if !empty && can_request && (!most_detailed || !refine_more || refine == Cesium3DTileRefine::Add) {
            self.requested_tiles.push(id);
        }
        if !empty && ready && refine == Cesium3DTileRefine::Add {
            self.selected_tiles.push(id);
        }
        let selection_start = self.selected_tiles.len();
        let mut children_ready = true;
        if refine_more {
            if most_detailed {
                children.reverse();
            } else {
                let camera = frame.uniform.camera.position;
                children.sort_by(|&a, &b| {
                    self.tiles[a]
                        .bounding_sphere
                        .distance_squared_to(camera)
                        .total_cmp(&self.tiles[b].bounding_sphere.distance_squared_to(camera))
                });
            }
            for child in children {
                children_ready &= self.select(child, frame, most_detailed);
            }
        }
        if refine_more && refine == Cesium3DTileRefine::Replace && !children_ready && !empty && !most_detailed {
            self.selected_tiles.truncate(selection_start);
        }
        if !empty
            && ready
            && refine == Cesium3DTileRefine::Replace
            && (!refine_more || (!children_ready && !most_detailed))
        {
            self.selected_tiles.push(id);
        }
        if most_detailed {
            (!refine_more || children_ready) && (empty || refine == Cesium3DTileRefine::Replace && refine_more || ready)
        } else {
            empty || ready || (refine_more && children_ready)
        }
    }
    pub fn update_for_pass(&mut self, frame: &mut FrameState, state: &mut TilesetPassState) -> Result<()> {
        let pass = state.pass;
        if (pass == Cesium3DTilePass::Preload && (!self.options.preload_when_hidden || self.options.show))
            || (pass == Cesium3DTilePass::PreloadFlight
                && (!self.options.preload_flight_destinations
                    || (!self.options.show && !self.options.preload_when_hidden)))
            || (pass == Cesium3DTilePass::RequestRenderModeDeferCheck
                && ((!self.cull_requests_while_moving && self.options.foveated_time_delay <= 0.) || !self.options.show))
        {
            return Ok(());
        }
        state.ready = false;
        let options = pass.get_pass_options();
        if !self.options.show && !options.ignore_commands {
            return Ok(());
        }
        if frame.mode == SceneMode::Morphing || self.root.is_none() {
            return Ok(());
        }
        if self.traversal_kind(pass) == TilesetTraversalKind::Skip {
            return Err(RuntimeError::new("skip-LOD traversal is not ported"));
        }
        let original_camera =
            state.camera.as_ref().map(|camera| std::mem::replace(&mut frame.uniform.camera, camera.clone()));
        let original_culling =
            state.culling_volume.as_ref().map(|volume| std::mem::replace(&mut frame.culling_volume, volume.clone()));
        let command_start = frame.command_list.len();
        let result = (|| {
            self.statistics.clear();
            if self.updated_model_matrix_frame != Some(frame.frame_number) || self.previous_model_matrix.is_none() {
                self.updated_model_matrix_frame = Some(frame.frame_number);
                self.model_matrix_changed = self.previous_model_matrix != Some(self.model_matrix);
                if self.model_matrix_changed {
                    self.previous_model_matrix = Some(self.model_matrix);
                }
            }
            self.cull_requests_while_moving = self.options.cull_requests_while_moving && !self.model_matrix_changed;
            self.selected_tiles.clear();
            self.requested_tiles.clear();
            self.update_transforms()?;
            let root = self.root.unwrap();
            let most_detailed = self.traversal_kind(pass) == TilesetTraversalKind::MostDetailed;
            state.ready = if !most_detailed
                && self.screen_space_error(root, frame, self.geometric_error) <= self.memory_adjusted_screen_space_error
            {
                true
            } else {
                self.select(root, frame, most_detailed)
            };
            if options.request_tiles {
                let mut requested = self.requested_tiles.clone();
                requested.sort_by(|&a, &b| self.tiles[a].priority.total_cmp(&self.tiles[b].priority));
                for id in requested {
                    if matches!(
                        self.tiles[id].state,
                        Cesium3DTileContentState::Unloaded | Cesium3DTileContentState::Expired
                    ) {
                        if self.cull_requests_while_moving {
                            let delta = if frame.camera_position_delta != 0. {
                                frame.camera_position_delta
                            } else {
                                frame.camera_position_delta_last_frame
                            };
                            let diameter = (self.tiles[id].bounding_sphere.radius * 2.).max(1.);
                            if self.options.cull_requests_while_moving_multiplier * delta / diameter >= 1. {
                                continue;
                            }
                        }
                        if let (Some(loader), Some(resource)) = (&self.loader, self.tiles[id].content_resource.clone())
                        {
                            let cancelled = Rc::new(Cell::new(false));
                            let request = TileContentRequest {
                                tile: id,
                                resource,
                                cancelled: Rc::clone(&cancelled),
                                header: self.tiles[id].header.clone(),
                            };
                            if let Some(future) = loader.request(request) {
                                self.pending.push(PendingContent { id, future, cancelled });
                                self.tiles[id].state = Cesium3DTileContentState::Loading;
                                self.statistics.number_of_pending_requests += 1;
                            } else {
                                self.statistics.number_of_attempted_requests += 1;
                            }
                        } else {
                            self.statistics.number_of_attempted_requests += 1;
                        }
                    }
                }
            }
            for id in self.selected_tiles.clone() {
                if options.is_render {
                    self.emit(TilesetEvent::TileVisible(id));
                }
                let tile = &mut self.tiles[id];
                if let Some(content) = &mut tile.content {
                    content.update(frame)?;
                    let m = content.metrics();
                    self.statistics.number_of_features_selected += m.features;
                    self.statistics.number_of_points_selected += m.points;
                    self.statistics.number_of_triangles_selected += m.triangles;
                }
            }
            self.statistics.selected = self.selected_tiles.len() as u64;
            self.statistics.number_of_commands = (frame.command_list.len() - command_start) as u64;
            self.statistics_per_pass[pass.value() as usize] = self.statistics.clone();
            Ok(())
        })();
        if options.ignore_commands {
            frame.command_list.truncate(command_start);
        }
        if let Some(camera) = original_camera {
            frame.uniform.camera = camera;
        }
        if let Some(volume) = original_culling {
            frame.culling_volume = volume;
        }
        result
    }
    pub fn post_passes_update(&mut self, frame: &mut FrameState) -> Result<()> {
        let mut i = 0;
        while i < self.pending.len() {
            let id = self.pending[i].id;
            if frame.frame_number.saturating_sub(self.tiles[id].touched_frame) >= 1 {
                let pending = self.pending.remove(i);
                pending.cancelled.set(true);
                self.tiles[id].state = Cesium3DTileContentState::Unloaded;
                self.statistics.number_of_pending_requests =
                    self.statistics.number_of_pending_requests.saturating_sub(1);
            } else {
                i += 1;
            }
        }
        let progress = (self.statistics.number_of_pending_requests, self.statistics.number_of_tiles_processing);
        let changed = progress != self.last_progress;
        self.last_progress = progress;
        self.tiles_loaded = progress == (0, 0) && self.statistics.number_of_attempted_requests == 0;
        if changed {
            self.defer(frame, TilesetEvent::LoadProgress { pending: progress.0, processing: progress.1 });
            if self.tiles_loaded {
                self.defer(frame, TilesetEvent::AllTilesLoaded);
                if !self.initial_tiles_loaded {
                    self.initial_tiles_loaded = true;
                    self.defer(frame, TilesetEvent::InitialTilesLoaded);
                }
            }
        }
        let trim = self.cache.trim;
        self.cache.trim = false;
        while self.cache.sentinel > 0
            && (trim || self.statistics.total_memory_usage_in_bytes() > self.options.cache_bytes)
        {
            let id = self.cache.entries[0];
            self.unload_tile(id)?;
        }
        Ok(())
    }
    pub fn trim_loaded_tiles(&mut self) {
        self.cache.trim();
    }
    pub fn unload_tile(&mut self, id: TileId) -> Result<()> {
        if self.cache.remove(id) {
            self.emit(TilesetEvent::TileUnload(id));
            if let Some(mut content) = self.tiles[id].content.take() {
                self.statistics.load(content.metrics(), false);
                content.destroy()?;
            }
            self.tiles[id].state = Cesium3DTileContentState::Unloaded;
            self.statistics.number_of_tiles_with_content_ready =
                self.statistics.number_of_tiles_with_content_ready.saturating_sub(1);
        }
        Ok(())
    }
    pub fn pick(&self, ray: &Ray, frame: &FrameState, webgl2: bool) -> Result<Option<Cartesian3>> {
        if !webgl2 && !self.options.enable_pick {
            return Ok(None);
        }
        let mut candidates: Vec<_> = self
            .selected_tiles
            .iter()
            .copied()
            .filter(|&id| {
                self.tiles[id].content.is_some()
                    && intersection_tests::ray_sphere(ray, &self.tiles[id].bounding_sphere).is_some()
            })
            .collect();
        candidates.sort_by(|&a, &b| {
            self.tiles[a]
                .bounding_sphere
                .distance_squared_to(ray.origin)
                .total_cmp(&self.tiles[b].bounding_sphere.distance_squared_to(ray.origin))
        });
        for id in candidates {
            if let Some(hit) = self.tiles[id].content.as_ref().unwrap().pick(ray, frame)? {
                return Ok(Some(hit));
            }
        }
        Ok(None)
    }
    pub fn destroy(&mut self) -> Result<()> {
        if self.is_destroyed() {
            return Ok(());
        }
        self.alive.set(false);
        for pending in self.pending.drain(..) {
            pending.cancelled.set(true);
        }
        for tile in &mut self.tiles {
            if let Some(content) = &mut tile.content {
                content.destroy()?;
            }
            tile.content = None;
            tile.destroyed = true;
        }
        self.processing.clear();
        self.selected_tiles.clear();
        self.root = None;
        self.statistics.number_of_pending_requests = 0;
        Ok(())
    }
    pub fn credits(&self) -> &[Credit] {
        &self.credits
    }
}
impl ScenePrimitive for Cesium3DTileset {
    fn pre_passes_update(&mut self, frame: &mut FrameState) -> Result<()> {
        Cesium3DTileset::pre_passes_update(self, frame)
    }
    fn update(&mut self, frame: &mut FrameState) -> Result<()> {
        self.update_for_pass(frame, &mut TilesetPassState::new(Cesium3DTilePass::Render))
    }
    fn post_passes_update(&mut self, frame: &mut FrameState) -> Result<()> {
        Cesium3DTileset::post_passes_update(self, frame)
    }
    fn update_for_pass(&mut self, frame: &mut FrameState, pass: Cesium3DTilePass) -> Result<()> {
        Cesium3DTileset::update_for_pass(self, frame, &mut TilesetPassState::new(pass))
    }
    fn destroy(&mut self) -> Result<()> {
        Cesium3DTileset::destroy(self)
    }
}
