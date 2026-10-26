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
#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(untagged)]
pub enum FeatureIdLabel {
    Label(String),
    Index(f64),
}
impl FeatureIdLabel {
    fn label(&self, prefix: &str) -> String {
        match self {
            Self::Label(label) => label.clone(),
            Self::Index(index) => format!("{prefix}_{}", math::js_number_to_string(*index)),
        }
    }
}
impl From<String> for FeatureIdLabel {
    fn from(value: String) -> Self {
        Self::Label(value)
    }
}
impl From<&str> for FeatureIdLabel {
    fn from(value: &str) -> Self {
        Self::Label(value.into())
    }
}
impl From<f64> for FeatureIdLabel {
    fn from(value: f64) -> Self {
        Self::Index(value)
    }
}
impl From<u32> for FeatureIdLabel {
    fn from(value: u32) -> Self {
        Self::Index(value as f64)
    }
}
impl From<i32> for FeatureIdLabel {
    fn from(value: i32) -> Self {
        Self::Index(value as f64)
    }
}

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
    fn configure_model(&mut self, _state: &TileContentModelState) {}
    /// Mutable processing lets model content defer readiness until after the
    /// real frame renders, just like a standalone model's ready event.
    fn process_with_frame(&mut self, frame: &mut FrameState) -> Result<bool> {
        self.process(frame)
    }
    fn is_renderable(&self) -> bool {
        true
    }
    fn take_failures(&mut self) -> Vec<TileContentFailure> {
        Vec::new()
    }
    fn metrics(&self) -> TileContentMetrics;
    fn process(&mut self, frame: &FrameState) -> Result<bool>;
    fn update(&mut self, frame: &mut FrameState) -> Result<()>;
    fn pick(&self, ray: &Ray, frame: &FrameState) -> Result<Option<Cartesian3>>;
    fn destroy(&mut self) -> Result<()>;
    /// Model contents override this with stable GPU texture identities. Other
    /// content owns its texture bytes independently.
    fn texture_allocations(&self) -> Vec<(Option<String>, u64)> {
        vec![(None, self.metrics().texture_bytes)]
    }
    fn take_expansions(&mut self) -> Vec<TileExpansion> {
        Vec::new()
    }
}
#[derive(Clone, Debug)]
pub struct TileContentModelState {
    pub model_matrix: Matrix4,
    pub classification_type: Option<ClassificationType>,
    pub feature_id_label: String,
    pub instance_feature_id_label: String,
    pub enable_pick: bool,
    pub show_credits_on_screen: bool,
}
pub type TileContentFuture = Pin<Box<dyn Future<Output = Result<Box<dyn TilesetContent>>>>>;
#[derive(Clone)]
pub struct TileContentRequest {
    pub tile: TileId,
    pub resource: Resource,
    pub base_resource: Resource,
    pub implicit: Option<ImplicitContentRequest>,
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
    pub debug_freeze_frame: bool,
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
            debug_freeze_frame: false,
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
    pub number_of_tiles_styled: u64,
    pub number_of_features_styled: u64,
    pub number_of_tiles_culled_with_children_union: u64,
    pub number_of_points_loaded: u64,
    pub geometry_byte_length: u64,
    pub textures_byte_length: u64,
    pub batch_table_byte_length: u64,
    pub textures_reference_counter_by_id: std::collections::BTreeMap<String, u64>,
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
        self.number_of_tiles_styled = 0;
        self.number_of_features_styled = 0;
        self.number_of_tiles_culled_with_children_union = 0;
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
    fn load_content(&mut self, content: &dyn TilesetContent, add: bool) {
        let mut metrics = content.metrics();
        metrics.texture_bytes = 0;
        self.load(metrics, add);
        for (id, bytes) in content.texture_allocations() {
            if let Some(id) = id {
                let count = self.textures_reference_counter_by_id.get(&id).copied().unwrap_or(0);
                if add {
                    if count == 0 {
                        self.textures_byte_length = self.textures_byte_length.saturating_add(bytes);
                    }
                    self.textures_reference_counter_by_id.insert(id, count + 1);
                } else if count <= 1 {
                    self.textures_reference_counter_by_id.remove(&id);
                    self.textures_byte_length = self.textures_byte_length.saturating_sub(bytes);
                } else {
                    self.textures_reference_counter_by_id.insert(id, count - 1);
                }
            } else if add {
                self.textures_byte_length = self.textures_byte_length.saturating_add(bytes);
            } else {
                self.textures_byte_length = self.textures_byte_length.saturating_sub(bytes);
            }
        }
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
    pub base_resource: Resource,
    pub implicit_coordinates: Option<ImplicitTileCoordinates>,
    pub implicit_subtree: Option<Rc<ImplicitSubtree>>,
    pub implicit_metadata: Option<ImplicitMetadataView>,
    pub state: Cesium3DTileContentState,
    pub content: Option<Box<dyn TilesetContent>>,
    pub priority: f64,
    pub touched_frame: u32,
    pub distance_to_camera: f64,
    pub implicit_tileset: Option<ImplicitTileset>,
    pub destroyed: bool,
    pub volume: TilesetVolume,
    pub content_volume: Option<TilesetVolume>,
    pub viewer_request_volume: Option<TilesetVolume>,
    pub traversal: TileTraversalState,
    pub has_external_content: bool,
    pub has_implicit_content: bool,
    pub has_renderable_content: bool,
    pub initial_transform: Matrix4,
    pub expire_duration: Option<f64>,
    pub expire_date: Option<JulianDate>,
    pub expired_content: Option<Box<dyn TilesetContent>>,
    pub content_counted: bool,
    pub metadata: Option<TileMetadata>,
    pub(super) source_bounds: Value,
    volume_exaggeration: [f64; 2],
}
impl TilesetTile {
    pub fn has_empty_content(&self) -> bool {
        self.content_resource.is_none()
    }
    pub fn content_ready(&self) -> bool {
        self.state == Cesium3DTileContentState::Ready
    }
}
pub(super) struct PendingContent {
    pub(super) id: TileId,
    future: TileContentFuture,
    pub(super) cancelled: Rc<Cell<bool>>,
    previous_state: Cesium3DTileContentState,
    remaining: Rc<Cell<u64>>,
    pub(super) counted: u64,
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
    pub ready: Option<bool>,
    pub command_list: Option<Vec<SceneCommand>>,
    pub camera: Option<crate::renderer::UniformCamera>,
    pub culling_volume: Option<CullingVolume>,
}
impl TilesetPassState {
    pub fn new(pass: Cesium3DTilePass) -> Self {
        Self { pass, ready: Some(false), command_list: None, camera: None, culling_volume: None }
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
    pub(super) pending: Vec<PendingContent>,
    processing: Vec<TileId>,
    pub(super) memory_adjusted_screen_space_error: f64,
    load_timestamp: Option<JulianDate>,
    previous_model_matrix: Option<Matrix4>,
    updated_model_matrix_frame: Option<u32>,
    model_matrix_changed: bool,
    pub(super) cull_requests_while_moving: bool,
    tiles_loaded: bool,
    initial_tiles_loaded: bool,
    last_progress: (u64, u64),
    initial_clipping_planes_origin_matrix: Matrix4,
    alive: Rc<Cell<bool>>,
    credits: Vec<Credit>,
    pub(super) visibility_frame: u64,
    pub(super) current_pass: Cesium3DTilePass,
    pub(super) minimum_priority: super::tileset_traversal::TilePriority,
    pub(super) maximum_priority: super::tileset_traversal::TilePriority,
    pub foveated_interpolation: Rc<dyn Fn(f64, f64, f64) -> f64>,
    pub selected_tiles_to_style: Vec<TileId>,
    pub empty_tiles: Vec<TileId>,
    pub has_mixed_content: bool,
    pub(super) clipping_planes: Option<Rc<RefCell<ClippingPlaneCollection>>>,
    pub(super) height_requests: Vec<super::tileset_content::TileHeightRequest>,
    exaggeration: [f64; 2],
}
fn number(v: &Value, key: &str, default: f64) -> f64 {
    v.get(key).and_then(Value::as_f64).unwrap_or(default)
}
pub(super) fn array<const N: usize>(v: &Value) -> Result<[f64; N]> {
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
            visibility_frame: 0,
            current_pass: Cesium3DTilePass::Render,
            minimum_priority: super::tileset_traversal::TilePriority::minimum(),
            maximum_priority: super::tileset_traversal::TilePriority::maximum(),
            foveated_interpolation: Rc::new(math::lerp),
            selected_tiles_to_style: Vec::new(),
            empty_tiles: Vec::new(),
            has_mixed_content: false,
            clipping_planes: None,
            height_requests: Vec::new(),
            exaggeration: [1., 0.],
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
        let loader = loader.or_else(|| {
            Some(Rc::new(TilesetResourceLoader::new(client.clone(), None)) as Rc<dyn TilesetContentLoader>)
        });
        Self::from_json(resource, &json, options, loader)
    }
    pub async fn from_ion_asset_id(
        asset_id: u64,
        ion: &Ion,
        ion_options: crate::core::ion_resource::IonAssetOptions,
        client: &ResourceClient,
        options: TilesetOptions,
        loader: Option<Rc<dyn TilesetContentLoader>>,
    ) -> Result<Self> {
        let ion_resource = IonResource::from_asset_id(asset_id, ion, ion_options, client)
            .await
            .map_err(|e| RuntimeError::new(e.to_string()))?;
        let resource = ion_resource.as_resource().clone();
        resource.options_mut().credits = Some(ion_resource.credits());
        Self::from_url(resource, client, options, loader).await
    }
    pub fn metadata(&self) -> Option<&TilesetMetadata> {
        self.metadata_extension.as_ref().and_then(|m| m.tileset())
    }
    pub fn schema(&self) -> Option<&Arc<MetadataSchema>> {
        self.metadata_extension.as_ref().map(|m| m.schema())
    }
    pub fn total_memory_usage_in_bytes(&self) -> u64 {
        self.statistics.total_memory_usage_in_bytes()
    }
    pub fn set_foveated_cone_size(&mut self, value: f64) -> Result<()> {
        if !(0. ..=1.).contains(&value) {
            return Err(RuntimeError::new("foveatedConeSize must be between zero and one"));
        }
        self.options.foveated_cone_size = value;
        Ok(())
    }
    pub fn set_foveated_minimum_screen_space_error_relaxation(&mut self, value: f64) -> Result<()> {
        if value < 0. || value > self.options.maximum_screen_space_error {
            return Err(RuntimeError::new(
                "foveatedMinimumScreenSpaceErrorRelaxation must be between zero and maximumScreenSpaceError",
            ));
        }
        self.options.foveated_minimum_screen_space_error_relaxation = value;
        Ok(())
    }
    pub fn set_feature_id_label(&mut self, value: impl Into<FeatureIdLabel>) {
        self.feature_id_label = value.into().label("featureId");
    }
    pub fn set_instance_feature_id_label(&mut self, value: impl Into<FeatureIdLabel>) {
        self.instance_feature_id_label = value.into().label("instanceFeatureId");
    }
    pub fn set_show_credits_on_screen(&mut self, value: bool) {
        self.options.show_credits_on_screen = value;
        self.credits = self.resource.options().credits.clone().unwrap_or_default();
        if let Some(credits) = self.asset.pointer("/extras/cesium/credits").and_then(Value::as_array) {
            for credit in credits {
                if let Some(html) = credit["html"].as_str() {
                    self.credits.push(Credit::new(html, false));
                }
            }
        }
        for credit in &mut self.credits {
            credit.show_on_screen |= value;
        }
    }
    pub fn get_height(
        &self,
        cartographic: Cartographic,
        frame: &FrameState,
        ellipsoid: Option<&Ellipsoid>,
    ) -> Result<Option<f64>> {
        let ellipsoid = ellipsoid.unwrap_or(&Ellipsoid::WGS84);
        let direction = ellipsoid.cartographic_to_cartesian(&cartographic).normalize().negate();
        let ray = Ray::new(direction.multiply_by_scalar(-2. * ellipsoid.maximum_radius), direction);
        Ok(self
            .pick(&ray, frame, frame.context_capabilities.webgl2)?
            .and_then(|p| ellipsoid.cartesian_to_cartographic(p))
            .map(|c| c.height))
    }
    pub fn update_height(
        &mut self,
        position: Cartographic,
        callback: impl FnMut() + 'static,
        ellipsoid: Option<Ellipsoid>,
    ) -> HeightSubscription {
        let active = Rc::new(Cell::new(true));
        self.height_requests.push(super::tileset_content::TileHeightRequest {
            position,
            ellipsoid: ellipsoid.unwrap_or(Ellipsoid::WGS84),
            callback: Rc::new(RefCell::new(callback)),
            active: Rc::clone(&active),
            invoked: Rc::new(Cell::new(false)),
        });
        HeightSubscription(active)
    }
    fn process_update_height(&mut self, id: TileId, frame: &mut FrameState) {
        if !self.options.enable_collision || !self.options.show || self.tiles[id].traversal.was_selected_last_frame {
            return;
        }
        let sphere = self.tiles[id].bounding_sphere;
        for request in &self.height_requests {
            if !request.active.get() || request.invoked.get() {
                continue;
            }
            let mut position = request.position;
            if let Some(center) = request.ellipsoid.cartesian_to_cartographic(sphere.center) {
                position.height = center.height;
            }
            if request.ellipsoid.cartographic_to_cartesian(&position).distance(sphere.center) > sphere.radius {
                continue;
            }
            request.invoked.set(true);
            let callback = Rc::clone(&request.callback);
            let active = Rc::clone(&request.active);
            let invoked = Rc::clone(&request.invoked);
            frame.after_render.push(Box::new(move || {
                if active.get() {
                    (callback.borrow_mut())();
                }
                invoked.set(false);
                Ok(false)
            }));
        }
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
        let start = self.tiles.len();
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
        if self.options.cull_with_children_bounds {
            for id in start..self.tiles.len() {
                self.compute_children_hint(id);
            }
        }
        Ok(root)
    }
    pub(super) fn make_tile(&mut self, resource: &Resource, header: &Value, parent: Option<TileId>) -> Result<TileId> {
        let description = if header.get("implicitTiling").is_some()
            || header.pointer("/extensions/3DTILES_implicit_tiling").is_some()
        {
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
        let coordinates = description
            .as_ref()
            .map(|d| ImplicitTileCoordinates::new(d.subdivision_scheme, d.subtree_levels, 0, 0, 0, Some(0)))
            .transpose()
            .map_err(|e| RuntimeError::new(e.to_string()))?;
        let mut stored_header = header.clone();
        if let (Some(d), Some(c)) = (&description, coordinates) {
            let subtree = d
                .subtree_uri_template
                .get_derived_resource(DerivedResourceOptions {
                    template_values: c
                        .get_template_values()
                        .into_iter()
                        .map(|(k, v)| (k.into(), v.to_string()))
                        .collect(),
                    ..Default::default()
                })
                .ok_or_else(|| RuntimeError::new("invalid subtree URI"))?;
            stored_header["contents"] = serde_json::json!([{"uri":subtree.url()}]);
            let h = stored_header.as_object_mut().ok_or_else(|| RuntimeError::new("invalid tile header"))?;
            h.remove("content");
            h.remove("extensions");
        }
        let header = &stored_header;
        let transform = header.get("transform").map(array::<16>).transpose()?.map(Matrix4).unwrap_or(Matrix4::IDENTITY);
        let computed_transform =
            parent.map_or(self.model_matrix, |p| self.tiles[p].computed_transform).multiply(&transform);
        let initial_transform =
            parent.map_or(Matrix4::IDENTITY, |p| self.tiles[p].initial_transform).multiply(&transform);
        let refine = match header["refine"].as_str() {
            Some("ADD" | "add") => Cesium3DTileRefine::Add,
            Some("REPLACE" | "replace") => Cesium3DTileRefine::Replace,
            _ => parent.map_or(Cesium3DTileRefine::Replace, |p| self.tiles[p].refine),
        };
        let geometric_error =
            number(header, "geometricError", parent.map_or(self.geometric_error, |p| self.tiles[p].geometric_error));
        let metadata = super::find_tile_metadata::find_tile_metadata(
            self.metadata_extension.as_ref().map(|m| m.schema().as_ref()),
            header,
        )
        .map_err(|e| RuntimeError::new(e.to_string()))?;
        let source_bounds = metadata
            .as_ref()
            .map(|m| {
                super::tileset_expansion::semantic_volume(m, bounding_volume_semantics::BoundingVolumePrefix::Tile)
            })
            .transpose()?
            .flatten()
            .or_else(|| header.get("boundingVolume").cloned())
            .ok_or_else(|| RuntimeError::new("tile boundingVolume is required"))?;
        let volume = TilesetVolume::from_header(&source_bounds, computed_transform, initial_transform, 1., 0.)?;
        let bounding_sphere = *volume.sphere();
        let contents = super::tileset_content::content_headers(header);
        let content_volume = if super::tileset_content::is_multiple(header) {
            None
        } else {
            contents
                .first()
                .and_then(|c| c.get("boundingVolume"))
                .map(|v| TilesetVolume::from_header(v, computed_transform, initial_transform, 1., 0.))
                .transpose()?
        };
        let viewer_request_volume = header
            .get("viewerRequestVolume")
            .map(|v| TilesetVolume::from_header(v, computed_transform, initial_transform, 1., 0.))
            .transpose()?;
        let content_resource = if super::tileset_content::is_multiple(header) {
            Some(resource.clone())
        } else {
            contents.first().and_then(|c| super::tileset_content::content_resource(resource, c))
        };
        let state = if content_resource.is_none() {
            Cesium3DTileContentState::Ready
        } else {
            Cesium3DTileContentState::Unloaded
        };
        let expire_duration = header.pointer("/expire/duration").and_then(Value::as_f64);
        let expire_date = header
            .pointer("/expire/date")
            .and_then(Value::as_str)
            .map(JulianDate::from_iso8601)
            .transpose()
            .map_err(|e| RuntimeError::new(e.to_string()))?;
        let id = self.tiles.len();
        let depth = parent.map_or(0, |p| self.tiles[p].depth + 1);
        let has_renderable_content = content_resource.is_some() && description.is_none();
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
            base_resource: resource.clone(),
            implicit_coordinates: coordinates,
            implicit_subtree: None,
            implicit_metadata: None,
            state,
            content: None,
            priority: 0.,
            touched_frame: 0,
            distance_to_camera: 0.,
            implicit_tileset: description,
            destroyed: false,
            source_bounds,
            volume,
            content_volume,
            viewer_request_volume,
            traversal: TileTraversalState::new(id),
            has_external_content: false,
            has_implicit_content: false,
            has_renderable_content,
            initial_transform,
            expire_duration,
            expire_date,
            expired_content: None,
            content_counted: false,
            metadata,
            volume_exaggeration: [1., 0.],
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
    pub fn pick_identity(&self) -> super::PickedObject {
        self.alive.clone()
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
            if self.tiles[i].computed_transform == transform && self.tiles[i].volume_exaggeration == self.exaggeration {
                continue;
            }
            self.tiles[i].computed_transform = transform;
            self.tiles[i].volume_exaggeration = self.exaggeration;
            self.tiles[i].volume = TilesetVolume::from_header(
                &self.tiles[i].source_bounds,
                transform,
                self.tiles[i].initial_transform,
                self.exaggeration[0],
                self.exaggeration[1],
            )?;
            self.tiles[i].bounding_sphere = *self.tiles[i].volume.sphere();
            let headers = super::tileset_content::content_headers(&self.tiles[i].header);
            self.tiles[i].content_volume = if super::tileset_content::is_multiple(&self.tiles[i].header) {
                None
            } else {
                headers
                    .first()
                    .and_then(|h| h.get("boundingVolume"))
                    .map(|v| {
                        TilesetVolume::from_header(
                            v,
                            transform,
                            self.tiles[i].initial_transform,
                            self.exaggeration[0],
                            self.exaggeration[1],
                        )
                    })
                    .transpose()?
            };
            self.tiles[i].viewer_request_volume = self.tiles[i]
                .header
                .get("viewerRequestVolume")
                .map(|v| {
                    TilesetVolume::from_header(
                        v,
                        transform,
                        self.tiles[i].initial_transform,
                        self.exaggeration[0],
                        self.exaggeration[1],
                    )
                })
                .transpose()?;
        }
        Ok(())
    }
    pub fn clipping_planes(&self) -> Option<&Rc<RefCell<ClippingPlaneCollection>>> {
        self.clipping_planes.as_ref()
    }
    pub fn set_clipping_planes(&mut self, planes: Option<Rc<RefCell<ClippingPlaneCollection>>>) -> Result<()> {
        if self.clipping_planes.as_ref().zip(planes.as_ref()).is_some_and(|(a, b)| Rc::ptr_eq(a, b)) {
            return Ok(());
        }
        if let Some(previous) = self.clipping_planes.take() {
            previous.borrow_mut().destroy()?;
        }
        if let Some(planes) = planes {
            planes.borrow_mut().assign_owner(Rc::as_ptr(&self.alive) as usize)?;
            self.clipping_planes = Some(planes);
        }
        Ok(())
    }
    pub(super) fn clip_volume(&self, volume: &TilesetVolume) -> Result<Intersect> {
        let Some(planes) = &self.clipping_planes else { return Ok(Intersect::Inside) };
        let planes = planes.borrow();
        if !planes.enabled {
            return Ok(Intersect::Inside);
        }
        let transform = self.root.map_or(Matrix4::IDENTITY, |root| {
            self.tiles[root].computed_transform.multiply(&self.initial_clipping_planes_origin_matrix)
        });
        planes.compute_intersection(volume, Some(&transform))
    }
    fn emit(&self, event: TilesetEvent) {
        if self.alive.get() {
            self.events.borrow_mut().raise_event(&event);
        }
    }
    pub(super) fn update_expiration(&mut self, id: TileId, now: JulianDate) -> Result<()> {
        let tile = &mut self.tiles[id];
        if tile.content_ready()
            && !tile.has_empty_content()
            && tile.expire_date.is_some_and(|date| JulianDate::less_than(&date, &now))
            && !super::tileset_content::is_multiple(&tile.header)
        {
            tile.state = Cesium3DTileContentState::Expired;
            tile.expired_content = tile.content.take();
        }
        Ok(())
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
    pub fn install_content(&mut self, id: TileId, mut content: Box<dyn TilesetContent>) -> Result<()> {
        if self.tiles.get(id).is_none_or(|t| t.destroyed || t.content.is_some()) {
            return Err(RuntimeError::new("invalid tile or tile already has content"));
        }
        for expansion in content.take_expansions() {
            self.expand_content(id, expansion)?;
        }
        self.tiles[id].has_renderable_content = content.is_renderable();
        for failure in content.take_failures() {
            self.emit(TilesetEvent::TileFailed {
                tile: id,
                url: Some(failure.resource.url()),
                message: failure.message,
            });
        }
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
            let remaining = if result.is_ready() { 0 } else { self.pending[i].remaining.get() };
            self.statistics.number_of_pending_requests = self
                .statistics
                .number_of_pending_requests
                .saturating_sub(self.pending[i].counted.saturating_sub(remaining));
            self.pending[i].counted = remaining;
            if let Poll::Ready(result) = result {
                let pending = self.pending.remove(i);
                if self.is_destroyed() || self.tiles[pending.id].destroyed {
                    continue;
                }
                match result {
                    Ok(content) => {
                        if pending.previous_state == Cesium3DTileContentState::Expired {
                            self.tiles[pending.id].expire_date = None;
                        }
                        if let Err(error) = self.install_content(pending.id, content) {
                            self.tiles[pending.id].state = Cesium3DTileContentState::Failed;
                            self.emit(TilesetEvent::TileFailed {
                                tile: pending.id,
                                url: self.tiles[pending.id].content_resource.as_ref().map(Resource::url),
                                message: error.message,
                            });
                        }
                    }
                    Err(error) => {
                        self.tiles[pending.id].state = Cesium3DTileContentState::Failed;
                        self.emit(TilesetEvent::TileFailed {
                            tile: pending.id,
                            url: self.tiles[pending.id].content_resource.as_ref().map(Resource::url),
                            message: error.message,
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
        self.height_requests.retain(|request| request.active.get());
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
            if let Some(content) = tile.content.as_mut() {
                content.configure_model(&TileContentModelState {
                    model_matrix: tile.computed_transform,
                    classification_type: self.classification_type,
                    feature_id_label: self.feature_id_label.clone(),
                    instance_feature_id_label: self.instance_feature_id_label.clone(),
                    enable_pick: self.options.enable_pick,
                    show_credits_on_screen: self.options.show_credits_on_screen,
                });
            }
            let result = tile
                .content
                .as_mut()
                .ok_or_else(|| RuntimeError::new("processing tile has no content"))?
                .process_with_frame(frame);
            match result {
                Ok(true) => {
                    tile.state = Cesium3DTileContentState::Ready;
                    if tile.has_renderable_content {
                        self.statistics.load_content(tile.content.as_ref().unwrap().as_ref(), true);
                        tile.content_counted = true;
                        self.statistics.number_of_tiles_with_content_ready += 1;
                        self.statistics.number_of_loaded_tiles_total += 1;
                        self.cache.add(id);
                    }
                    if let Some(duration) = tile.expire_duration {
                        let date = frame.time.add_seconds(duration);
                        if tile.expire_date.is_none_or(|old| JulianDate::less_than(&old, &date)) {
                            tile.expire_date = Some(date);
                        }
                    }
                    self.statistics.number_of_tiles_processing =
                        self.statistics.number_of_tiles_processing.saturating_sub(1);
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
                        message: error.message,
                    });
                }
            }
        }
        if self.statistics.total_memory_usage_in_bytes() < self.options.cache_bytes {
            self.memory_adjusted_screen_space_error =
                (self.memory_adjusted_screen_space_error / 1.02).max(self.options.maximum_screen_space_error);
        } else if exceeded && !self.processing.is_empty() {
            self.memory_adjusted_screen_space_error *= 1.02;
            for id in self.processing.clone() {
                self.update_priority(id);
            }
            self.processing.sort_by(|&a, &b| self.tiles[a].priority.total_cmp(&self.tiles[b].priority));
        }
        let start = *self.load_timestamp.get_or_insert(frame.time);
        if let Some(planes) = self.clipping_planes.as_ref().filter(|p| p.borrow().enabled) {
            frame.schedule_clipping_planes_update(Rc::clone(planes));
        }
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
        let volume = tile.content_volume.as_ref().unwrap_or(&tile.volume);
        let (up, direction, height, min, max) = if let TilesetVolume::Region(r) = volume {
            (
                camera.position.normalize(),
                camera.direction,
                camera.cartographic.map_or(f64::NAN, |c| c.height),
                r.minimum_height,
                r.maximum_height,
            )
        } else {
            let inverse = tile.computed_transform.inverse_transformation();
            let center = inverse.multiply_by_point(volume.sphere().center);
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
                let half = match volume {
                    TilesetVolume::Box(b) => b.bounding_volume().half_axes.get_column(2).magnitude(),
                    TilesetVolume::Sphere(s) => s.radius(),
                    _ => f64::NAN,
                };
                (Cartesian3::UNIT_Z, direction, position.z, center.z - half, center.z + half)
            }
        };
        let close = min + (max - min) * self.options.dynamic_screen_space_error_height_falloff;
        let t = ((height - close) / (max - close)).clamp(0., 1.);
        self.dynamic_screen_space_error_computed_density =
            self.options.dynamic_screen_space_error_density * (1. - direction.dot(up).abs()) * (1. - t);
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
        state.ready = Some(false);
        let options = pass.get_pass_options();
        if !self.options.show && !options.ignore_commands {
            return Ok(());
        }
        if frame.mode == SceneMode::Morphing || self.root.is_none() {
            return Ok(());
        }
        let original_camera =
            state.camera.as_ref().map(|camera| std::mem::replace(&mut frame.uniform.camera, camera.clone()));
        let original_culling =
            state.culling_volume.as_ref().map(|volume| std::mem::replace(&mut frame.culling_volume, volume.clone()));
        let original_commands =
            state.command_list.take().map(|commands| std::mem::replace(&mut frame.command_list, commands));
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
            self.exaggeration = [frame.vertical_exaggeration, frame.vertical_exaggeration_relative_height];
            self.update_transforms()?;
            self.visibility_frame = self.visibility_frame.wrapping_add(1);
            self.current_pass = pass;
            self.minimum_priority = super::tileset_traversal::TilePriority::minimum();
            self.maximum_priority = super::tileset_traversal::TilePriority::maximum();
            let kind = self.traversal_kind(pass);
            let ready = self.select_tiles(frame, kind)?;
            state.ready = if kind == TilesetTraversalKind::MostDetailed { Some(ready) } else { None };
            if options.request_tiles {
                let mut requested = self.requested_tiles.clone();
                requested.sort_by(|&a, &b| self.tiles[a].priority.total_cmp(&self.tiles[b].priority));
                for id in requested {
                    if matches!(
                        self.tiles[id].state,
                        Cesium3DTileContentState::Unloaded | Cesium3DTileContentState::Expired
                    ) {
                        if let (Some(loader), Some(resource)) = (&self.loader, self.tiles[id].content_resource.clone())
                        {
                            if self.tiles[id].state == Cesium3DTileContentState::Expired {
                                if let Some(date) = self.tiles[id].expire_date {
                                    resource.set_query_parameters(
                                        &vec![("expired".into(), Some(query::QueryValue::Single(date.to_string())))],
                                        false,
                                    );
                                }
                            }
                            let cancelled = Rc::new(Cell::new(false));
                            let request = TileContentRequest {
                                tile: id,
                                base_resource: self.tiles[id].base_resource.clone(),
                                implicit: self.tiles[id]
                                    .implicit_tileset
                                    .as_ref()
                                    .zip(self.tiles[id].implicit_coordinates)
                                    .map(|(d, c)| ImplicitContentRequest { description: d.clone(), coordinates: c }),
                                resource,
                                cancelled: Rc::clone(&cancelled),
                                header: self.tiles[id].header.clone(),
                            };
                            let remaining = Rc::new(Cell::new(1));
                            let future = if super::tileset_content::is_multiple(&request.header) {
                                super::tileset_content::request_multiple(loader.as_ref(), request, &remaining)
                            } else {
                                loader.request(request)
                            };
                            if let Some(future) = future {
                                let previous_state = self.tiles[id].state;
                                if previous_state == Cesium3DTileContentState::Expired
                                    && (self.tiles[id].has_external_content || self.tiles[id].has_implicit_content)
                                {
                                    self.destroy_subtree(id)?;
                                }
                                if previous_state == Cesium3DTileContentState::Expired && self.tiles[id].content_counted
                                {
                                    if let Some(content) = self.tiles[id].expired_content.as_ref() {
                                        self.statistics.load_content(content.as_ref(), false);
                                    }
                                    self.statistics.number_of_tiles_with_content_ready =
                                        self.statistics.number_of_tiles_with_content_ready.saturating_sub(1);
                                    self.tiles[id].content_counted = false;
                                }
                                let counted = remaining.get();
                                self.pending.push(PendingContent {
                                    id,
                                    future,
                                    cancelled,
                                    previous_state,
                                    remaining,
                                    counted,
                                });
                                self.tiles[id].state = Cesium3DTileContentState::Loading;
                                self.statistics.number_of_pending_requests += counted;
                            } else {
                                self.statistics.number_of_attempted_requests += 1;
                            }
                        } else {
                            self.statistics.number_of_attempted_requests += 1;
                        }
                    }
                }
            }
            let bivariate = self.is_skipping_level_of_detail()
                && self.has_mixed_content
                && frame.stencil_buffer
                && !self.selected_tiles.is_empty();
            let backface_start = frame.command_list.len() + usize::from(bivariate);
            let mut backfaces = Vec::new();
            if bivariate {
                let mut clear = SceneCommand::new(
                    "skip-lod-stencil-clear",
                    crate::renderer::Pass::Cesium3DTile,
                    Rc::new(|_| Ok(())),
                );
                clear.is_clear = true;
                clear.stencil_clear = Some(stencil_constants::SKIP_LOD_MASK as u32);
                frame.command_list.push(clear);
            }
            for id in self.selected_tiles.clone() {
                let clipping = if self.tiles[id].traversal.is_clipped {
                    self.clipping_planes.as_ref().filter(|p| p.borrow().enabled).map(|p| {
                        SceneClippingPlanes::new(
                            Rc::clone(p),
                            self.root.map_or(Matrix4::IDENTITY, |root| {
                                self.tiles[root]
                                    .computed_transform
                                    .multiply(&self.initial_clipping_planes_origin_matrix)
                            }),
                        )
                    })
                } else {
                    None
                };
                if options.is_render {
                    self.emit(TilesetEvent::TileVisible(id));
                }
                self.process_update_height(id, frame);
                let tile = &mut self.tiles[id];
                let command_start = frame.command_list.len();
                if tile.content_ready() {
                    if let Some(mut old) = tile.expired_content.take() {
                        old.destroy()?;
                    }
                }
                let expired = !tile.content_ready() && tile.expired_content.is_some();
                let content = if expired { tile.expired_content.as_mut() } else { tile.content.as_mut() };
                if let Some(content) = content {
                    content.configure_model(&TileContentModelState {
                        model_matrix: tile.computed_transform,
                        classification_type: self.classification_type,
                        feature_id_label: self.feature_id_label.clone(),
                        instance_feature_id_label: self.instance_feature_id_label.clone(),
                        enable_pick: self.options.enable_pick,
                        show_credits_on_screen: self.options.show_credits_on_screen,
                    });
                    if let Err(error) = content.update(frame) {
                        if !expired {
                            tile.state = Cesium3DTileContentState::Failed;
                            return Err(error);
                        }
                    }
                    let m = content.metrics();
                    self.statistics.number_of_features_selected += m.features;
                    self.statistics.number_of_points_selected += m.points;
                    self.statistics.number_of_triangles_selected += m.triangles;
                }
                for command in &mut frame.command_list[command_start..] {
                    command.clipping_planes = clipping.clone();
                    if command.pass == crate::renderer::Pass::Cesium3DTile && !command.is_clear {
                        command.tileset_draw = Some(if bivariate {
                            TilesetDrawKind::Stencil { selection_depth: tile.traversal.selection_depth }
                        } else {
                            TilesetDrawKind::Regular
                        });
                        if bivariate && !tile.traversal.final_resolution {
                            let mut backface = command.clone();
                            backface.id = format!("{}-skip-lod-backface", backface.id);
                            backface.tileset_draw = Some(TilesetDrawKind::Backface);
                            backface.metadata.cast_shadows = false;
                            backface.metadata.receive_shadows = false;
                            backfaces.push(backface);
                        }
                    }
                    command.metadata.depth_for_translucent_classification =
                        command.pass == crate::renderer::Pass::Translucent;
                }
            }
            if bivariate {
                frame.command_list.splice(backface_start..backface_start, backfaces);
            }
            self.statistics.selected = self.selected_tiles.len() as u64;
            self.statistics.number_of_commands = (frame.command_list.len() - command_start) as u64;
            self.statistics_per_pass[pass.value() as usize] = self.statistics.clone();
            if options.is_render && self.statistics.selected > 0 {
                frame.credits.extend(self.credits.iter().cloned());
            }
            Ok(())
        })();
        if options.ignore_commands {
            frame.command_list.truncate(command_start);
        }
        if let Some(commands) = original_commands {
            state.command_list = Some(std::mem::replace(&mut frame.command_list, commands));
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
                self.tiles[id].state = pending.previous_state;
                self.statistics.number_of_pending_requests =
                    self.statistics.number_of_pending_requests.saturating_sub(pending.counted);
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
            let tile = &mut self.tiles[id];
            if tile.content_counted {
                if let Some(content) = tile.content.as_ref().or(tile.expired_content.as_ref()) {
                    self.statistics.load_content(content.as_ref(), false);
                }
                self.statistics.number_of_tiles_with_content_ready =
                    self.statistics.number_of_tiles_with_content_ready.saturating_sub(1);
                tile.content_counted = false;
            }
            if let Some(mut content) = tile.content.take() {
                content.destroy()?;
            }
            if let Some(mut content) = tile.expired_content.take() {
                content.destroy()?;
            }
            tile.state = Cesium3DTileContentState::Unloaded;
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
                (self.tiles[id].content.is_some() || self.tiles[id].expired_content.is_some())
                    && intersection_tests::ray_sphere(
                        ray,
                        self.tiles[id].content_volume.as_ref().map_or(&self.tiles[id].bounding_sphere, |v| v.sphere()),
                    )
                    .is_some()
            })
            .collect();
        candidates.sort_by(|&a, &b| {
            self.tiles[a]
                .content_volume
                .as_ref()
                .map_or(&self.tiles[a].bounding_sphere, |v| v.sphere())
                .distance_squared_to(ray.origin)
                .total_cmp(
                    &self.tiles[b]
                        .content_volume
                        .as_ref()
                        .map_or(&self.tiles[b].bounding_sphere, |v| v.sphere())
                        .distance_squared_to(ray.origin),
                )
        });
        for id in candidates {
            if let Some(hit) =
                self.tiles[id].content.as_ref().or(self.tiles[id].expired_content.as_ref()).unwrap().pick(ray, frame)?
            {
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
        if let Some(planes) = self.clipping_planes.take() {
            planes.borrow_mut().destroy()?;
        }
        for request in self.height_requests.drain(..) {
            request.active.set(false);
        }
        for pending in self.pending.drain(..) {
            pending.cancelled.set(true);
        }
        for tile in &mut self.tiles {
            if let Some(content) = &mut tile.content {
                content.destroy()?;
            }
            if let Some(mut content) = tile.expired_content.take() {
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
    fn most_detailed_pick_targets(&self) -> Vec<super::PickedObject> {
        if self.options.show && !self.is_destroyed() {
            vec![self.alive.clone()]
        } else {
            Vec::new()
        }
    }
    fn update_most_detailed_pick(
        &mut self,
        frame: &mut FrameState,
        pass: &mut TilesetPassState,
        _targets: &[super::PickedObject],
    ) -> Result<bool> {
        self.update_for_pass(frame, pass)?;
        Ok(pass.ready.unwrap_or(false))
    }
    fn pre_passes_update(&mut self, frame: &mut FrameState) -> Result<()> {
        Cesium3DTileset::pre_passes_update(self, frame)
    }
    fn update(&mut self, frame: &mut FrameState) -> Result<()> {
        let mut pass = frame
            .tileset_pass_state
            .take()
            .ok_or_else(|| RuntimeError::new("frameState.tilesetPassState is required"))?;
        let result = self.update_for_pass(frame, &mut pass);
        frame.tileset_pass_state = Some(pass);
        result
    }
    fn post_passes_update(&mut self, frame: &mut FrameState) -> Result<()> {
        Cesium3DTileset::post_passes_update(self, frame)
    }
    fn update_for_pass(&mut self, frame: &mut FrameState, pass: Cesium3DTilePass) -> Result<()> {
        Cesium3DTileset::update_for_pass(self, frame, &mut TilesetPassState::new(pass))
    }
    fn height_at(&self, cartographic: &Cartographic, frame: &FrameState) -> Result<Option<f64>> {
        if !self.options.show || !self.options.enable_collision {
            return Ok(None);
        }
        self.get_height(*cartographic, frame, None)
    }
    fn destroy(&mut self) -> Result<()> {
        Cesium3DTileset::destroy(self)
    }
}
