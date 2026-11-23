//! CesiumJS `GaussianSplatPrimitive`: aggregates selected tiles' splats, packs
//! them into integer textures, sorts them by view depth and draws instanced
//! screen-space quads.
//!
//! The pinned module owns two asynchronous state machines (snapshot building and
//! steady-state re-sorting) plus texture retirement. Both are ported here over
//! the existing native `GaussianSplatSorter` and `GaussianSplatTextureGenerator`
//! coordinators. Tasks advance once per `update`, matching promise settlement
//! between frames. The tileset is supplied as a per-frame
//! [`GaussianSplatTilesetState`] view, and tile content through the
//! [`GaussianSplatTileContent`] trait, because the 3D Tiles core port owns the
//! tileset.

use super::{
    gaussian_splat_render_resources::GaussianSplatRenderResources,
    gaussian_splat_sorter::GaussianSplatSorter,
    gaussian_splat_texture_generator::GaussianSplatTextureGenerator,
    scene::{Result, SceneCommand, SceneDrawOptions},
    split_direction::SplitDirection,
    FrameState,
};
use crate::{
    core::{
        BoundingSphere, Cartesian3, ComponentDatatype, Geometry, GeometryAttribute, GeometryAttributes, Matrix4,
        PixelFormat, PrimitiveType, Quaternion, RuntimeError,
    },
    renderer::{
        render_state::{Cull, DepthTest},
        Buffer, BufferData, BufferKind, BufferUsage, Context, Gl, PixelDatatype, Sampler, ShaderDestination, ShaderProgramOptions, Texture, TextureOptions, TextureRegion,
        UniformValue, VertexAttribute, VertexArray,
    },
    shaders::root::{PRIMITIVE_GAUSSIAN_SPLAT_FS, PRIMITIVE_GAUSSIAN_SPLAT_VS},
    workers::gaussian_splats::SplatTextureData,
};
use serde_json::{json, Value};
use std::{
    cell::Cell,
    collections::HashSet,
    rc::Rc,
    task::{Context as TaskContext, Poll, Waker},
};

/// A selected tile's splat data.
pub trait GaussianSplatTileContent {
    fn points_length(&self) -> usize;
    fn positions(&self) -> &[f32];
    fn rotations(&self) -> &[f32];
    fn scales(&self) -> &[f32];
    fn colors(&self) -> &[u8];
    fn spherical_harmonics_degree(&self) -> u32;
    fn spherical_harmonics_coefficient_count(&self) -> usize;
    fn packed_spherical_harmonics_data(&self) -> Option<&[u32]>;
    fn is_ready(&self) -> bool {
        true
    }
}

/// One selected tile, identified by its content pointer for change detection.
#[derive(Clone)]
pub struct GaussianSplatTile {
    pub content: Rc<dyn GaussianSplatTileContent>,
}

impl GaussianSplatTile {
    pub fn new(content: Rc<dyn GaussianSplatTileContent>) -> Self {
        Self { content }
    }
    fn identity(&self) -> usize {
        Rc::as_ptr(&self.content) as *const () as usize
    }
}

/// The tileset values `GaussianSplatPrimitive.update` reads.
pub struct GaussianSplatTilesetState<'a> {
    pub show: bool,
    pub model_matrix: Matrix4,
    pub model_matrix_changed: bool,
    pub split_direction: SplitDirection,
    pub bounding_sphere: BoundingSphere,
    pub debug_show_bounding_volume: bool,
    pub selected_tiles: &'a [GaussianSplatTile],
}

impl<'a> GaussianSplatTilesetState<'a> {
    pub fn new(bounding_sphere: BoundingSphere, selected_tiles: &'a [GaussianSplatTile]) -> Self {
        Self {
            show: true,
            model_matrix: Matrix4::IDENTITY,
            model_matrix_changed: false,
            split_direction: SplitDirection::None,
            bounding_sphere,
            debug_show_bounding_volume: false,
            selected_tiles,
        }
    }
}

/// `GaussianSplatPrimitive.GaussianSplatSortingState`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GaussianSplatSortingState {
    Idle,
    Waiting,
    Sorting,
    Sorted,
    Error,
}

/// `GaussianSplatPrimitive.SnapshotState`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SnapshotState {
    Building,
    TexturePending,
    TextureReady,
    Sorting,
    Ready,
}

pub const DEFAULT_STABLE_FRAMES: u32 = 2;
pub const DEFAULT_MAX_SNAPSHOT_STALL_FRAMES: u32 = 30;
pub const DEFAULT_SORT_MIN_FRAME_INTERVAL: u32 = 3;
pub const DEFAULT_SORT_MIN_ANGLE_RADIANS: f64 = 0.008726646259971648;
pub const DEFAULT_SORT_MIN_POSITION_DELTA: f64 = 1.0;

struct Snapshot<G: Gl + 'static> {
    generation: u64,
    positions: Vec<f32>,
    rotations: Vec<f32>,
    scales: Vec<f32>,
    colors: Vec<u8>,
    sh_data: Option<Vec<u32>>,
    spherical_harmonics_degree: u32,
    sh_coefficient_count: usize,
    num_splats: usize,
    indexes: Option<Vec<u32>>,
    gaussian_splat_texture: Option<Rc<Texture<G>>>,
    spherical_harmonics_texture: Option<Rc<Texture<G>>>,
    last_texture_width: u32,
    last_texture_height: u32,
    state: SnapshotState,
}

type SortTask = crate::core::task_processor::ProcessedTask<Value, Value>;
type TextureTask = super::gaussian_splat_texture_generator::TextureGeneratorTask;

/// Structural information about the most recently built splat command.
#[derive(Clone, Debug, PartialEq)]
pub struct GaussianSplatCommandInfo {
    pub pass: crate::renderer::Pass,
    pub count: usize,
    pub instance_count: u32,
    pub primitive_type: PrimitiveType,
    pub has_splat_texture: bool,
    pub has_spherical_harmonics_texture: bool,
    pub spherical_harmonics_degree: u32,
    pub model_matrix: Matrix4,
}

/// Constructor options.
pub struct GaussianSplatPrimitiveOptions {
    pub debug_show_bounding_volume: bool,
    pub hardware_concurrency: usize,
    pub sorter: Option<Rc<GaussianSplatSorter>>,
    pub texture_generator: Option<Rc<GaussianSplatTextureGenerator>>,
    pub use_log_depth: bool,
}

impl Default for GaussianSplatPrimitiveOptions {
    fn default() -> Self {
        Self {
            debug_show_bounding_volume: false,
            hardware_concurrency: 4,
            sorter: None,
            texture_generator: None,
            use_log_depth: false,
        }
    }
}

fn poll_once<F: std::future::Future + Unpin>(future: &mut F) -> Poll<F::Output> {
    let mut cx = TaskContext::from_waker(Waker::noop());
    std::pin::Pin::new(future).poll(&mut cx)
}

fn texture_bytes(data: &[u32]) -> Vec<u8> {
    data.iter().flat_map(|v| v.to_le_bytes()).collect()
}

fn create_gaussian_splat_texture<G: Gl + 'static>(
    context: &Context<G>,
    data: &SplatTextureData,
) -> Result<Rc<Texture<G>>> {
    let options = TextureOptions {
        width: data.width,
        height: data.height,
        pixel_format: PixelFormat::RgbaInteger,
        pixel_datatype: PixelDatatype::UnsignedInt,
        flip_y: false,
        sampler: Sampler::NEAREST,
        pre_multiply_alpha: false,
        skip_color_space_conversion: true,
        id: None,
    };
    Ok(Rc::new(Texture::new(context, options, Some(&texture_bytes(&data.data)))?))
}

fn create_spherical_harmonics_texture<G: Gl + 'static>(
    context: &Context<G>,
    width: u32,
    height: u32,
    data: &[u32],
) -> Result<Rc<Texture<G>>> {
    let options = TextureOptions {
        width,
        height,
        pixel_format: PixelFormat::RgInteger,
        pixel_datatype: PixelDatatype::UnsignedInt,
        flip_y: false,
        sampler: Sampler::NEAREST,
        pre_multiply_alpha: false,
        skip_color_space_conversion: true,
        id: None,
    };
    Ok(Rc::new(Texture::new(context, options, Some(&texture_bytes(data)))?))
}

/// A primitive that renders Gaussian splats.
pub struct GaussianSplatPrimitive<G: Gl + 'static> {
    positions: Option<Vec<f32>>,
    rotations: Option<Vec<f32>>,
    scales: Option<Vec<f32>>,
    colors: Option<Vec<u8>>,
    indexes: Option<Vec<u32>>,
    num_splats: usize,
    snapshot: Option<Snapshot<G>>,
    pending_snapshot: Option<Snapshot<G>>,
    retired_textures: Vec<(Rc<Texture<G>>, u32)>,
    selected_tiles_stable_frames: u32,
    needs_snapshot_rebuild: bool,
    snapshot_rebuild_stall_frames: u32,
    prev_view_matrix: Matrix4,
    debug_show_bounding_volume: bool,
    pub gaussian_splat_texture: Option<Rc<Texture<G>>>,
    pub spherical_harmonics_texture: Option<Rc<Texture<G>>>,
    last_texture_width: u32,
    last_texture_height: u32,
    vertex_array: Option<Rc<VertexArray<G>>>,
    vertex_array_len: isize,
    split_direction: SplitDirection,
    split_direction_cell: Rc<Cell<SplitDirection>>,
    dirty: bool,
    root_transform: Option<Matrix4>,
    sorter_state: GaussianSplatSortingState,
    sorter_promise: Option<SortTask>,
    pending_sort_promise: Option<SortTask>,
    active_sort: Option<(u64, u64, usize)>,
    pending_sort: Option<(u64, u64, usize)>,
    texture_task: Option<TextureTask>,
    texture_task_snapshot_ready: bool,
    splat_data_generation: u64,
    sort_request_id: u64,
    last_steady_sort_frame_number: Option<u32>,
    last_steady_sort_camera_position: Cartesian3,
    has_last_steady_sort_camera_position: bool,
    last_steady_sort_camera_direction: Cartesian3,
    has_last_steady_sort_camera_direction: bool,
    sorter_error: Option<RuntimeError>,
    sorter: Rc<GaussianSplatSorter>,
    texture_generator: Rc<GaussianSplatTextureGenerator>,
    context: Option<Context<G>>,
    draw_command: Option<SceneCommand>,
    draw_command_model_matrix: Matrix4,
    selected_tile_length: usize,
    selected_tile_set: HashSet<usize>,
    ready: bool,
    has_gaussian_splat_texture: bool,
    gaussian_splat_texture_pending: bool,
    is_destroyed_flag: bool,
    spherical_harmonics_degree: u32,
    spherical_harmonics_coefficient_count: usize,
    scratch_aggregate_sh_buffer: Option<Vec<u32>>,
    use_log_depth: bool,
    pub model_matrix: Matrix4,
    pub update_calls: u32,
    camera_position: Option<Cartesian3>,
    bounding_sphere: BoundingSphere,
    pub last_command_info: Option<GaussianSplatCommandInfo>,
}

/// The pinned `GaussianSplatPrimitive.transformTile` attribute transform, for a
/// caller that owns the content buffers.
#[allow(clippy::too_many_arguments)]
pub fn transform_splat_attributes(
    positions: &mut [f32],
    rotations: &mut [f32],
    scales: &mut [f32],
    computed_transform: &Matrix4,
    world_transform: &Matrix4,
    tileset_model_matrix: &Matrix4,
    root_transform: &Matrix4,
    axis_correction: &Matrix4,
) {
    let computed_model_matrix = computed_transform.multiply_transformation(axis_correction);
    let computed_model_matrix = computed_model_matrix.multiply_transformation(world_transform);
    let to_global = tileset_model_matrix.multiply(root_transform);
    let Ok(to_local) = to_global.inverse() else {
        return;
    };
    let transform = to_local.multiply_transformation(&computed_model_matrix);
    let count = positions.len() / 3;
    for i in 0..count {
        let position = Cartesian3::new(
            positions[i * 3] as f64,
            positions[i * 3 + 1] as f64,
            positions[i * 3 + 2] as f64,
        );
        let rotation = Quaternion::new(
            rotations[i * 4] as f64,
            rotations[i * 4 + 1] as f64,
            rotations[i * 4 + 2] as f64,
            rotations[i * 4 + 3] as f64,
        );
        let scale = Cartesian3::new(scales[i * 3] as f64, scales[i * 3 + 1] as f64, scales[i * 3 + 2] as f64);
        let local = Matrix4::from_translation_quaternion_rotation_scale(position, &rotation, scale);
        let transformed = transform.multiply_transformation(&local);
        let position = transformed.get_translation();
        let rotation = Quaternion::from_rotation_matrix(&transformed.get_rotation());
        let scale = transformed.get_scale();
        positions[i * 3] = position.x as f32;
        positions[i * 3 + 1] = position.y as f32;
        positions[i * 3 + 2] = position.z as f32;
        rotations[i * 4] = rotation.x as f32;
        rotations[i * 4 + 1] = rotation.y as f32;
        rotations[i * 4 + 2] = rotation.z as f32;
        rotations[i * 4 + 3] = rotation.w as f32;
        scales[i * 3] = scale.x as f32;
        scales[i * 3 + 1] = scale.y as f32;
        scales[i * 3 + 2] = scale.z as f32;
    }
}

impl<G: Gl + 'static> GaussianSplatPrimitive<G> {
    pub fn new(options: GaussianSplatPrimitiveOptions) -> Self {
        let sorter = options
            .sorter
            .unwrap_or_else(|| Rc::new(GaussianSplatSorter::native(options.hardware_concurrency)));
        let texture_generator = options.texture_generator.unwrap_or_else(|| {
            Rc::new(GaussianSplatTextureGenerator::native(options.hardware_concurrency))
        });
        Self {
            positions: None,
            rotations: None,
            scales: None,
            colors: None,
            indexes: None,
            num_splats: 0,
            snapshot: None,
            pending_snapshot: None,
            retired_textures: Vec::new(),
            selected_tiles_stable_frames: 0,
            needs_snapshot_rebuild: false,
            snapshot_rebuild_stall_frames: 0,
            prev_view_matrix: Matrix4::IDENTITY,
            debug_show_bounding_volume: options.debug_show_bounding_volume,
            gaussian_splat_texture: None,
            spherical_harmonics_texture: None,
            last_texture_width: 0,
            last_texture_height: 0,
            vertex_array: None,
            vertex_array_len: -1,
            split_direction: SplitDirection::None,
            split_direction_cell: Rc::new(Cell::new(SplitDirection::None)),
            dirty: false,
            root_transform: None,
            sorter_state: GaussianSplatSortingState::Idle,
            sorter_promise: None,
            pending_sort_promise: None,
            active_sort: None,
            pending_sort: None,
            texture_task: None,
            texture_task_snapshot_ready: false,
            splat_data_generation: 0,
            sort_request_id: 0,
            last_steady_sort_frame_number: None,
            last_steady_sort_camera_position: Cartesian3::ZERO,
            has_last_steady_sort_camera_position: false,
            last_steady_sort_camera_direction: Cartesian3::ZERO,
            has_last_steady_sort_camera_direction: false,
            sorter_error: None,
            sorter,
            texture_generator,
            context: None,
            draw_command: None,
            draw_command_model_matrix: Matrix4::IDENTITY,
            selected_tile_length: 0,
            selected_tile_set: HashSet::new(),
            ready: false,
            has_gaussian_splat_texture: false,
            gaussian_splat_texture_pending: false,
            is_destroyed_flag: false,
            spherical_harmonics_degree: 0,
            spherical_harmonics_coefficient_count: 0,
            scratch_aggregate_sh_buffer: None,
            use_log_depth: options.use_log_depth,
            model_matrix: Matrix4::IDENTITY,
            update_calls: 0,
            camera_position: None,
            bounding_sphere: BoundingSphere::new(Cartesian3::ZERO, 0.0),
            last_command_info: None,
        }
    }

    pub fn ready(&self) -> bool {
        self.ready
    }
    pub fn is_stable(&self) -> bool {
        !self.dirty
            && (self.pending_snapshot.is_none()
                || self.pending_snapshot.as_ref().is_some_and(|s| s.state == SnapshotState::Ready))
    }
    pub fn split_direction(&self) -> SplitDirection {
        self.split_direction
    }
    pub fn set_split_direction(&mut self, value: SplitDirection) {
        if self.split_direction != value {
            self.split_direction = value;
            self.split_direction_cell.set(value);
            self.dirty = true;
        }
    }
    pub fn num_splats(&self) -> usize {
        self.num_splats
    }
    pub fn indexes(&self) -> Option<&[u32]> {
        self.indexes.as_deref()
    }
    pub fn sorting_state(&self) -> GaussianSplatSortingState {
        self.sorter_state
    }
    pub fn selected_tile_length(&self) -> usize {
        self.selected_tile_length
    }
    pub fn root_transform(&self) -> Option<Matrix4> {
        self.root_transform
    }
    pub fn set_root_transform(&mut self, transform: Matrix4) {
        self.root_transform = Some(transform);
    }
    pub fn draw_command(&self) -> Option<&SceneCommand> {
        self.draw_command.as_ref()
    }
    pub fn texture_bytes(&self) -> usize {
        self.gaussian_splat_texture.as_ref().map_or(0, |texture| texture.size_in_bytes())
            + self.spherical_harmonics_texture.as_ref().map_or(0, |texture| texture.size_in_bytes())
    }
    pub fn retired_texture_count(&self) -> usize {
        self.retired_textures.len()
    }

    pub fn is_destroyed(&self) -> bool {
        self.is_destroyed_flag
    }

    /// Cesium's `onTileLoad`.
    pub fn on_tile_load(&mut self) {
        self.dirty = true;
    }

    /// Cesium's `onTileVisible` (a no-op).
    pub fn on_tile_visible(&mut self) {}

    fn context_of(frame: &FrameState) -> Option<Context<G>> {
        frame.context.as_ref().and_then(|c| c.get::<Context<G>>()).cloned()
    }

    fn ensure_root_transform(&mut self, state: &GaussianSplatTilesetState<'_>) {
        if self.root_transform.is_none() {
            self.root_transform =
                Some(crate::core::Transforms::east_north_up_to_fixed_frame(state.bounding_sphere.center, &crate::core::Ellipsoid::WGS84));
        }
    }

    fn should_start_steady_sort(&self, frame: &FrameState) -> bool {
        let frames_since = self
            .last_steady_sort_frame_number
            .map(|frame_number| frame.frame_number.wrapping_sub(frame_number))
            .unwrap_or(u32::MAX);
        if self.last_steady_sort_frame_number.is_some() && frames_since < DEFAULT_SORT_MIN_FRAME_INTERVAL {
            return false;
        }
        let camera = &frame.uniform.camera;
        if !self.has_last_steady_sort_camera_position || !self.has_last_steady_sort_camera_direction {
            return true;
        }
        let position_delta = Cartesian3::distance(&camera.position, self.last_steady_sort_camera_position);
        if position_delta >= DEFAULT_SORT_MIN_POSITION_DELTA {
            return true;
        }
        let angle_delta = crate::core::Cartesian3::angle_between(&camera.direction, self.last_steady_sort_camera_direction);
        angle_delta >= DEFAULT_SORT_MIN_ANGLE_RADIANS
    }

    fn mark_steady_sort_start(&mut self, frame: &FrameState) {
        self.last_steady_sort_frame_number = Some(frame.frame_number);
        let camera = &frame.uniform.camera;
        self.last_steady_sort_camera_position = camera.position;
        self.has_last_steady_sort_camera_position = true;
        self.last_steady_sort_camera_direction = camera.direction;
        self.has_last_steady_sort_camera_direction = true;
    }

    fn have_selected_tiles_changed(&self, selected: &[GaussianSplatTile]) -> bool {
        if self.selected_tile_set.len() != selected.len() {
            return true;
        }
        selected.iter().any(|tile| !self.selected_tile_set.contains(&tile.identity()))
    }

    fn destroy_snapshot_textures(snapshot: &mut Option<Snapshot<G>>) {
        if let Some(snapshot) = snapshot.as_mut() {
            if let Some(texture) = snapshot.gaussian_splat_texture.take() {
                let _ = texture.destroy();
            }
            if let Some(texture) = snapshot.spherical_harmonics_texture.take() {
                let _ = texture.destroy();
            }
        }
    }

    fn retire_texture(&mut self, texture: Option<&Rc<Texture<G>>>, frame_number: u32) {
        if let Some(texture) = texture {
            self.retired_textures.push((texture.clone(), frame_number));
        }
    }

    fn release_retired_textures(&mut self, frame_number: u32) {
        let (expired, next): (Vec<_>, Vec<_>) =
            self.retired_textures.drain(..).partition(|(_, retired)| frame_number.wrapping_sub(*retired) > 0);
        for (texture, _) in expired {
            let _ = texture.destroy();
        }
        self.retired_textures = next;
    }

    fn commit_snapshot(&mut self, mut snapshot: Snapshot<G>, frame_number: u32) -> Result<()> {
        if snapshot.indexes.is_none() || snapshot.state != SnapshotState::Ready {
            return Err(RuntimeError::new("Committing snapshot before it is READY."));
        }
        let old_splat = self
            .snapshot
            .as_ref()
            .and_then(|current| current.gaussian_splat_texture.clone())
            .or_else(|| self.gaussian_splat_texture.clone());
        if old_splat.as_ref().is_some_and(|old| {
            snapshot
                .gaussian_splat_texture
                .as_ref()
                .is_none_or(|new| !Rc::ptr_eq(old, new))
        }) {
            self.retire_texture(old_splat.as_ref(), frame_number);
        }
        let old_sh = self
            .snapshot
            .as_ref()
            .and_then(|current| current.spherical_harmonics_texture.clone())
            .or_else(|| self.spherical_harmonics_texture.clone());
        if old_sh.as_ref().is_some_and(|old| {
            snapshot
                .spherical_harmonics_texture
                .as_ref()
                .is_none_or(|new| !Rc::ptr_eq(old, new))
        }) {
            self.retire_texture(old_sh.as_ref(), frame_number);
        }

        self.positions = Some(snapshot.positions.clone());
        self.rotations = Some(snapshot.rotations.clone());
        self.scales = Some(snapshot.scales.clone());
        self.colors = Some(snapshot.colors.clone());
        self.indexes = snapshot.indexes.clone();
        self.spherical_harmonics_degree = snapshot.spherical_harmonics_degree;
        self.spherical_harmonics_coefficient_count = snapshot.sh_coefficient_count;
        self.num_splats = snapshot.num_splats;
        self.gaussian_splat_texture = snapshot.gaussian_splat_texture.clone();
        self.spherical_harmonics_texture = snapshot.spherical_harmonics_texture.clone();
        self.last_texture_width = snapshot.last_texture_width;
        self.last_texture_height = snapshot.last_texture_height;
        self.has_gaussian_splat_texture = snapshot.gaussian_splat_texture.is_some();
        self.gaussian_splat_texture_pending = false;
        self.vertex_array = None;
        self.vertex_array_len = -1;
        self.draw_command = None;
        self.sorter_promise = None;
        self.active_sort = None;
        self.sorter_state = GaussianSplatSortingState::Idle;
        self.dirty = false;
        snapshot.state = SnapshotState::Ready;
        self.snapshot = Some(snapshot);
        Ok(())
    }

    fn aggregate(&mut self, tiles: &[GaussianSplatTile]) -> Snapshot<G> {
        let total_elements: usize = tiles.iter().map(|tile| tile.content.points_length()).sum();
        let mut positions = vec![0f32; total_elements * 3];
        let mut scales = vec![0f32; total_elements * 3];
        let mut rotations = vec![0f32; total_elements * 4];
        let mut colors = vec![0u8; total_elements * 4];
        let (mut po, mut so, mut ro, mut co) = (0usize, 0usize, 0usize, 0usize);
        for tile in tiles {
            let content = &tile.content;
            let take = |src: &[f32], dst: &mut Vec<f32>, offset: &mut usize| {
                let end = (*offset + src.len()).min(dst.len());
                dst[*offset..end].copy_from_slice(&src[..end - *offset]);
                *offset = end;
            };
            take(content.positions(), &mut positions, &mut po);
            take(content.scales(), &mut scales, &mut so);
            take(content.rotations(), &mut rotations, &mut ro);
            let color_src = content.colors();
            let end = (co + color_src.len()).min(colors.len());
            colors[co..end].copy_from_slice(&color_src[..end - co]);
            co = end;
        }

        // The pinned content takes the degree from the first selected tile, but
        // sizes the packed buffer from the first tile that has SH data.
        let degree = tiles
            .first()
            .map_or(0, |tile| tile.content.spherical_harmonics_degree());
        let coefficient_count = if degree > 0 {
            tiles
                .first()
                .map_or(0, |tile| tile.content.spherical_harmonics_coefficient_count())
        } else {
            0
        };
        let sh_data = if coefficient_count > 0 {
            let required_length = (total_elements as f64 * (coefficient_count as f64 * 2.0 / 3.0)) as usize;
            if self.scratch_aggregate_sh_buffer.as_ref().is_none_or(|b| b.len() < required_length) {
                self.scratch_aggregate_sh_buffer = Some(vec![0u32; required_length]);
            }
            let aggregate = self.scratch_aggregate_sh_buffer.as_mut().expect("buffer");
            let mut offset = 0usize;
            for tile in tiles {
                if tile.content.spherical_harmonics_degree() > 0 {
                    if let Some(data) = tile.content.packed_spherical_harmonics_data() {
                        let end = (offset + data.len()).min(aggregate.len());
                        aggregate[offset..end].copy_from_slice(&data[..end - offset]);
                        offset = end;
                    }
                }
            }
            Some(aggregate[..offset].to_vec())
        } else {
            None
        };

        self.splat_data_generation += 1;
        let generation = self.splat_data_generation;
        Snapshot {
            generation,
            positions,
            rotations,
            scales,
            colors,
            sh_data,
            spherical_harmonics_degree: degree,
            sh_coefficient_count: coefficient_count,
            num_splats: total_elements,
            indexes: None,
            gaussian_splat_texture: None,
            spherical_harmonics_texture: None,
            last_texture_width: 0,
            last_texture_height: 0,
            state: SnapshotState::Building,
        }
    }

    fn schedule_texture_generation(&mut self) {
        let Some(snapshot) = self.pending_snapshot.as_ref() else {
            return;
        };
        if snapshot.state != SnapshotState::Building {
            return;
        }
        let parameters = json!({
            "attributes": {
                "positions": &snapshot.positions,
                "scales": &snapshot.scales,
                "rotations": &snapshot.rotations,
                "colors": &snapshot.colors,
            },
            "count": snapshot.num_splats,
        });
        let mut cx = TaskContext::from_waker(Waker::noop());
        let _ = self.texture_generator.poll_initialization(&mut cx);
        match self.texture_generator.generate_from_attributes(parameters) {
            Ok(Some(task)) => {
                self.texture_task = Some(task);
                self.texture_task_snapshot_ready = false;
                if let Some(snapshot) = self.pending_snapshot.as_mut() {
                    snapshot.state = SnapshotState::TexturePending;
                }
            }
            Ok(None) => {}
            Err(error) => {
                let _ = error;
            }
        }
    }

    fn update_texture_generation(&mut self, context: &Context<G>) {
        let Some(task) = self.texture_task.as_mut() else {
            return;
        };
        match poll_once(task) {
            Poll::Pending => {}
            Poll::Ready(Err(_)) => {
                self.texture_task = None;
                if let Some(snapshot) = self.pending_snapshot.as_mut() {
                    snapshot.state = SnapshotState::Building;
                }
            }
            Poll::Ready(Ok(value)) => {
                self.texture_task = None;
                let Some(snapshot) = self.pending_snapshot.as_mut() else {
                    return;
                };
                let data: Vec<u32> = value
                    .get("data")
                    .and_then(Value::as_array)
                    .map(|values| values.iter().filter_map(Value::as_u64).map(|v| v as u32).collect())
                    .unwrap_or_default();
                let width = value.get("width").and_then(Value::as_u64).unwrap_or(0) as u32;
                let height = value.get("height").and_then(Value::as_u64).unwrap_or(0) as u32;
                let texture_data = SplatTextureData { data, width, height };
                let result = if snapshot.gaussian_splat_texture.is_none() {
                    create_gaussian_splat_texture(context, &texture_data)
                } else if snapshot.last_texture_width != width || snapshot.last_texture_height != height {
                    let previous = snapshot.gaussian_splat_texture.take();
                    let created = create_gaussian_splat_texture(context, &texture_data);
                    if let Some(previous) = previous {
                        let _ = previous.destroy();
                    }
                    created
                } else {
                    let texture = snapshot.gaussian_splat_texture.as_ref().expect("texture");
                    texture
                        .copy_from(
                            &texture_bytes(&texture_data.data),
                            TextureRegion { x: 0, y: 0, width, height },
                        )
                        .map(|_| texture.clone())
                };
                let Ok(texture) = result else {
                    snapshot.state = SnapshotState::Building;
                    return;
                };
                snapshot.gaussian_splat_texture = Some(texture);
                snapshot.last_texture_height = height;
                snapshot.last_texture_width = width;

                if let Some(sh) = snapshot.sh_data.clone().filter(|_| snapshot.spherical_harmonics_degree > 0) {
                    let old = snapshot.spherical_harmonics_texture.take();
                    let width = match context.call("getParameter", &[crate::core::webgl_constants::MAX_TEXTURE_SIZE.into()]) {
                        Ok(crate::renderer::GlValue::Number(number)) => (number as u32).min(4096),
                        _ => 4096,
                    };
                    let dims = snapshot.sh_coefficient_count as f64 / 3.0;
                    let splats_per_row = if dims > 0.0 { (width as f64 / dims).floor() as usize } else { 0 };
                    if splats_per_row > 0 {
                        let floats_per_row = splats_per_row * (dims as usize * 2);
                        let height = (snapshot.num_splats as f64 / splats_per_row as f64).ceil() as usize;
                        let mut tex_buf = vec![0u32; width as usize * height * 2];
                        let mut data_index = 0usize;
                        let mut i = 0usize;
                        while data_index < sh.len() {
                            let end = (data_index + floats_per_row).min(sh.len());
                            let destination = i;
                            let count = end - data_index;
                            if destination + count <= tex_buf.len() {
                                tex_buf[destination..destination + count].copy_from_slice(&sh[data_index..end]);
                            }
                            data_index += floats_per_row;
                            i += width as usize * 2;
                        }
                        if let Ok(texture) = create_spherical_harmonics_texture(context, width, height as u32, &tex_buf) {
                            snapshot.spherical_harmonics_texture = Some(texture);
                        }
                    }
                    if let Some(old) = old {
                        let _ = old.destroy();
                    }
                }
                snapshot.state = SnapshotState::TextureReady;
            }
        }
    }

    fn start_sort(&mut self, frame: &FrameState, pending: bool) -> bool {
        let Some(snapshot) = self.pending_snapshot.as_ref().or(self.snapshot.as_ref()) else {
            return false;
        };
        let positions = if pending {
            self.pending_snapshot.as_ref().map(|s| s.positions.clone()).unwrap_or_default()
        } else {
            self.positions.clone().unwrap_or_default()
        };
        let count = if pending { snapshot.num_splats } else { self.num_splats };
        let view = frame.uniform.camera.view;
        let root = self.root_transform.unwrap_or(Matrix4::IDENTITY);
        let model_view = view.multiply(&root);
        let parameters = json!({
            "primitive": {
                "positions": positions,
                "modelView": model_view.0.map(|v| v as f32).to_vec(),
                "count": count,
            },
            "sortType": "Index",
        });
        let mut cx = TaskContext::from_waker(Waker::noop());
        let _ = self.sorter.poll_initialization(&mut cx);
        match self.sorter.radix_sort_indexes(parameters) {
            Ok(Some(task)) => {
                self.sort_request_id += 1;
                let request_id = self.sort_request_id;
                if pending {
                    self.pending_sort = Some((request_id, self.splat_data_generation, count));
                    self.pending_sort_promise = Some(task);
                    if let Some(snapshot) = self.pending_snapshot.as_mut() {
                        snapshot.state = SnapshotState::Sorting;
                    }
                } else {
                    self.active_sort = Some((request_id, self.splat_data_generation, count));
                    self.sorter_promise = Some(task);
                    self.mark_steady_sort_start(frame);
                    self.sorter_state = GaussianSplatSortingState::Sorting;
                }
                true
            }
            Ok(None) => false,
            Err(error) => {
                self.sorter_error = Some(error);
                self.sorter_state = GaussianSplatSortingState::Error;
                false
            }
        }
    }

    fn resolve_pending_sort(&mut self, frame_number: u32) -> Result<()> {
        let Some(task) = self.pending_sort_promise.as_mut() else {
            return Ok(());
        };
        match poll_once(task) {
            Poll::Pending => Ok(()),
            Poll::Ready(result) => {
                let pending_sort = self.pending_sort.take();
                self.pending_sort_promise = None;
                let Some((request_id, generation, expected)) = pending_sort else {
                    return Ok(());
                };
                let pending_matches = self.pending_snapshot.as_ref().is_some_and(|s| {
                    s.generation == generation && s.state == SnapshotState::Sorting
                });
                if !pending_matches {
                    return Ok(());
                }
                match result {
                    Ok(value) => {
                        let sorted: Option<Vec<u32>> = value.as_array().map(|values| {
                            values.iter().filter_map(Value::as_u64).map(|v| v as u32).collect()
                        });
                        let matches = request_id == self.sort_request_id
                            && sorted.as_ref().is_some_and(|sorted| sorted.len() == expected);
                        if !matches {
                            if let Some(snapshot) = self.pending_snapshot.as_mut() {
                                if snapshot.state == SnapshotState::Sorting {
                                    snapshot.state = SnapshotState::TextureReady;
                                }
                            }
                            return Ok(());
                        }
                        let mut snapshot = self.pending_snapshot.take().expect("pending snapshot");
                        snapshot.indexes = sorted;
                        snapshot.state = SnapshotState::Ready;
                        self.pending_snapshot = None;
                        self.commit_snapshot(snapshot, frame_number)?;
                        let context = self.context.clone();
                        if let Some(context) = context {
                            self.build_draw_command(&context)?;
                        }
                    }
                    Err(error) => {
                        if let Some(snapshot) = self.pending_snapshot.as_mut() {
                            if snapshot.state == SnapshotState::Sorting {
                                snapshot.state = SnapshotState::TextureReady;
                            }
                        }
                        self.sorter_state = GaussianSplatSortingState::Error;
                        self.sorter_error = Some(error);
                    }
                }
                Ok(())
            }
        }
    }

    fn resolve_steady_sort(&mut self) {
        let Some(task) = self.sorter_promise.as_mut() else {
            return;
        };
        match poll_once(task) {
            Poll::Pending => {}
            Poll::Ready(result) => {
                let active = self.active_sort;
                let is_active = active.is_some_and(|(request_id, generation, _)| {
                    request_id == self.sort_request_id && generation == self.splat_data_generation
                });
                self.sorter_promise = None;
                if !is_active {
                    return;
                }
                match result {
                    Ok(value) => {
                        let sorted: Option<Vec<u32>> = value.as_array().map(|values| {
                            values.iter().filter_map(Value::as_u64).map(|v| v as u32).collect()
                        });
                        let expected = active.map_or(0, |(_, _, expected)| expected);
                        if sorted.as_ref().is_some_and(|sorted| sorted.len() == expected) {
                            self.indexes = sorted;
                            self.sorter_state = GaussianSplatSortingState::Sorted;
                        } else {
                            self.sorter_state = GaussianSplatSortingState::Idle;
                        }
                    }
                    Err(error) => {
                        self.sorter_state = GaussianSplatSortingState::Error;
                        self.sorter_error = Some(error);
                    }
                }
            }
        }
    }

    fn build_draw_command(&mut self, context: &Context<G>) -> Result<()> {
        let Some(texture) = self.gaussian_splat_texture.clone() else {
            return Ok(());
        };
        let mut resources = GaussianSplatRenderResources::new(self.use_log_depth);
        resources.alpha_options.pass = Some(crate::renderer::Pass::GaussianSplats);
        let builder = &mut resources.shader_builder;
        builder.set_position_attribute("vec2", "a_screenQuadPosition")?;
        builder.add_attribute("float", "a_splatIndex");
        builder.add_varying("vec4", "v_splatColor", None);
        builder.add_varying("vec2", "v_vertPos", None);
        builder.add_uniform("float", "u_splitDirection", ShaderDestination::VERTEX);
        builder.add_varying("float", "v_splitDirection", None);
        builder.add_uniform("highp usampler2D", "u_splatAttributeTexture", ShaderDestination::VERTEX);
        builder.add_uniform("float", "u_sphericalHarmonicsDegree", ShaderDestination::VERTEX);
        builder.add_uniform("float", "u_splatScale", ShaderDestination::VERTEX);
        builder.add_uniform("vec3", "u_cameraPositionWC", ShaderDestination::VERTEX);
        builder.add_uniform("mat3", "u_inverseModelRotation", ShaderDestination::VERTEX);
        if self.spherical_harmonics_degree > 0 {
            builder.add_define("HAS_SPHERICAL_HARMONICS", Some("1"), ShaderDestination::VERTEX);
            builder.add_uniform("highp usampler2D", "u_sphericalHarmonicsTexture", ShaderDestination::VERTEX);
        }
        builder.add_vertex_lines(&[PRIMITIVE_GAUSSIAN_SPLAT_VS.source]);
        builder.add_fragment_lines(&[PRIMITIVE_GAUSSIAN_SPLAT_FS.source]);
        let (vertex_source, fragment_source) = builder.sources();
        let attribute_locations = builder.attribute_locations().clone();
        let options = ShaderProgramOptions {
            vertex_shader_source: vertex_source,
            fragment_shader_source: fragment_source,
            attribute_locations: attribute_locations.clone(),
        };
        let program = context.with_shader_cache(|cache| cache.get_shader_program(&options))?;

        let index_count = self.indexes.as_ref().map_or(self.num_splats, Vec::len);
        let instance_count = index_count as u32;
        let needs_rebuild = self
            .vertex_array
            .as_ref()
            .is_none_or(|_| index_count as isize > self.vertex_array_len);
        if needs_rebuild {
            let index_data = self.indexes.clone().unwrap_or_default();
            let bytes = texture_bytes(&index_data);
            let buffer = Buffer::new(context, BufferKind::Vertex, BufferUsage::DynamicDraw, BufferData::Bytes(&bytes))?;
            let splat_location = attribute_locations.get("a_splatIndex").copied().unwrap_or(1);
            let splat_index = VertexAttribute {
                index: splat_location,
                enabled: true,
                source: crate::renderer::AttributeSource::Buffer(buffer),
                components: 1,
                datatype: ComponentDatatype::UnsignedInt,
                normalize: false,
                offset: 0,
                stride: 0,
                instance_divisor: 1,
            };
            let mut attributes = GeometryAttributes::default();
            attributes.others.push((
                "a_screenQuadPosition".into(),
                GeometryAttribute::floats(2, &[-1.0, -1.0, 1.0, -1.0, 1.0, 1.0, -1.0, 1.0]),
            ));
            let geometry = Geometry::new(attributes, None, PrimitiveType::TriangleStrip);
            let va = VertexArray::from_geometry(
                context,
                &geometry,
                crate::renderer::GeometryUploadOptions {
                    buffer_usage: BufferUsage::DynamicDraw,
                    attribute_locations: attribute_locations.clone().into_iter().collect(),
                    interleave: false,
                    vertex_array_attributes: vec![splat_index],
                },
            )?;
            self.vertex_array = Some(Rc::new(va));
        }
        self.vertex_array_len = index_count as isize;

        let state = state_for_gaussian_splats(resources.render_state.clone(), context.webgl2());
        let tileset_model_matrix = self.model_matrix;
        let root = self.root_transform.unwrap_or(Matrix4::IDENTITY);
        let model_matrix = tileset_model_matrix.multiply(&root);
        self.draw_command_model_matrix = model_matrix;

        let camera_position = self.camera_position.unwrap_or(Cartesian3::ZERO);
        let degrees = self.spherical_harmonics_degree;
        let split = self.split_direction_cell.clone();
        let inverse_rotation = {
            let Ok(inverse) = model_matrix.inverse() else {
                return Ok(());
            };
            inverse.get_rotation()
        };
        let inverse_values = inverse_rotation.0.iter().map(|&v| v as f32).collect::<Vec<_>>();
        let sh_texture = self.spherical_harmonics_texture.clone();
        let uniform_map: crate::renderer::UniformMap = Rc::new(move || {
            let mut uniforms = vec![
                (
                    "u_cameraPositionWC".into(),
                    UniformValue::Float(vec![
                        camera_position.x as f32,
                        camera_position.y as f32,
                        camera_position.z as f32,
                    ]),
                ),
                (
                    "u_inverseModelRotation".into(),
                    UniformValue::Matrix { dimension: 3, values: inverse_values.clone() },
                ),
                ("u_sphericalHarmonicsDegree".into(), UniformValue::Float(vec![degrees as f32])),
                (
                    "u_splitDirection".into(),
                    UniformValue::Float(vec![split.get().value() as f32]),
                ),
            ];
            uniforms.push(("u_splatScale".into(), UniformValue::Float(vec![1.0])));
            Ok(uniforms)
        });

        let metadata = crate::renderer::DrawMetadata {
            bounding_volume: Some(self.bounding_sphere),
            model_matrix: Some(model_matrix),
            debug_show_bounding_volume: self.debug_show_bounding_volume,
            ..Default::default()
        };
        let vertex_array = self.vertex_array.clone().expect("vertex array");
        let mut options = SceneDrawOptions::new(vertex_array, program);
        options.state = state;
        options.primitive_type = PrimitiveType::TriangleStrip;
        options.count = Some(4);
        options.instance_count = instance_count;
        options.uniform_map = Some(uniform_map);
        options.textures.push(("u_splatAttributeTexture".into(), texture));
        if let Some(sh_texture) = sh_texture {
            options.textures.push(("u_sphericalHarmonicsTexture".into(), sh_texture));
        }
        self.draw_command = Some(SceneCommand::from_draw(
            "gaussian-splats",
            context.clone(),
            crate::renderer::Pass::GaussianSplats,
            metadata,
            options,
        ));
        self.last_command_info = Some(GaussianSplatCommandInfo {
            pass: crate::renderer::Pass::GaussianSplats,
            count: 4,
            instance_count,
            primitive_type: PrimitiveType::TriangleStrip,
            has_splat_texture: self.gaussian_splat_texture.is_some(),
            has_spherical_harmonics_texture: self.spherical_harmonics_texture.is_some(),
            spherical_harmonics_degree: self.spherical_harmonics_degree,
            model_matrix,
        });
        Ok(())
    }

    /// Cesium's `update`.
    pub fn update(&mut self, frame: &mut FrameState, tileset: &GaussianSplatTilesetState<'_>) -> Result<()> {
        if self.is_destroyed_flag {
            return Err(RuntimeError::new("This object was destroyed, i.e., destroy() was called."));
        }
        self.update_calls += 1;
        self.context = Self::context_of(frame);
        self.ensure_root_transform(tileset);
        self.model_matrix = tileset.model_matrix;
        self.bounding_sphere = tileset.bounding_sphere;
        self.camera_position = Some(frame.uniform.camera.position);
        let frame_number = frame.frame_number;
        self.release_retired_textures(frame_number);

        if !tileset.show {
            return Ok(());
        }

        if let Some(command) = &self.draw_command {
            frame.command_list.push(command.clone());
        }

        if tileset.model_matrix_changed {
            self.dirty = true;
            return Ok(());
        }
        let has_root_transform = self.root_transform.is_some();
        if frame.passes.pick {
            return Ok(());
        }
        if self.split_direction != tileset.split_direction {
            self.set_split_direction(tileset.split_direction);
        }
        let camera = &frame.uniform.camera;
        let view_matrix = camera.view;

        // Advance asynchronous work started on earlier frames.
        self.resolve_pending_sort(frame_number)?;
        self.resolve_steady_sort();
        if self.texture_task.is_some() {
            if let Some(context) = self.context.clone() {
                self.update_texture_generation(&context);
            }
        }
        if let Some(error) = self.sorter_error.take_if(|_| self.sorter_state == GaussianSplatSortingState::Error) {
            return Err(error);
        }

        if self.sorter_state == GaussianSplatSortingState::Idle {
            let selected_tiles_changed =
                !tileset.selected_tiles.is_empty() && self.have_selected_tiles_changed(tileset.selected_tiles);
            if tileset.selected_tiles.is_empty() {
                self.selected_tiles_stable_frames = 0;
                self.needs_snapshot_rebuild = false;
                self.snapshot_rebuild_stall_frames = 0;
            } else if selected_tiles_changed {
                self.selected_tiles_stable_frames = 0;
            } else {
                self.selected_tiles_stable_frames += 1;
            }
            if selected_tiles_changed || self.dirty {
                self.needs_snapshot_rebuild = true;
            }
            let is_stable = self.selected_tiles_stable_frames >= DEFAULT_STABLE_FRAMES;
            let is_bootstrap = self.snapshot.is_none() && self.pending_snapshot.is_none() && self.draw_command.is_none();
            if self.needs_snapshot_rebuild && !tileset.selected_tiles.is_empty() {
                self.snapshot_rebuild_stall_frames += 1;
            } else {
                self.snapshot_rebuild_stall_frames = 0;
            }
            let allow_rebuild = is_stable
                || is_bootstrap
                || self.snapshot_rebuild_stall_frames >= DEFAULT_MAX_SNAPSHOT_STALL_FRAMES;
            let has_pending_work = self.dirty
                || self.needs_snapshot_rebuild
                || selected_tiles_changed
                || self.pending_snapshot.is_some()
                || self.pending_sort_promise.is_some()
                || self.draw_command.is_none();
            if !has_pending_work && view_matrix == self.prev_view_matrix {
                return Ok(());
            }

            if !tileset.selected_tiles.is_empty() && self.needs_snapshot_rebuild && allow_rebuild {
                self.splat_data_generation += 1;
                self.active_sort = None;
                self.sorter_promise = None;
                self.sorter_state = GaussianSplatSortingState::Idle;
                self.pending_sort_promise = None;
                self.pending_sort = None;
                Self::destroy_snapshot_textures(&mut self.pending_snapshot);
                let snapshot = self.aggregate(tileset.selected_tiles);
                self.pending_snapshot = Some(snapshot);
                self.selected_tile_length = tileset.selected_tiles.len();
                self.selected_tile_set = tileset.selected_tiles.iter().map(GaussianSplatTile::identity).collect();
                self.dirty = false;
                self.needs_snapshot_rebuild = false;
                self.snapshot_rebuild_stall_frames = 0;
            }

            if let Some(snapshot) = self.pending_snapshot.as_ref() {
                match snapshot.state {
                    SnapshotState::Building => {
                        self.schedule_texture_generation();
                        return Ok(());
                    }
                    SnapshotState::TexturePending => return Ok(()),
                    SnapshotState::TextureReady if snapshot.gaussian_splat_texture.is_none() => return Ok(()),
                    _ => {}
                }
                if !has_root_transform {
                    return Ok(());
                }
                self.prev_view_matrix = view_matrix;
                if snapshot.state == SnapshotState::TextureReady && self.pending_sort_promise.is_none() {
                    if !self.start_sort(frame, true) {
                        if let Some(snapshot) = self.pending_snapshot.as_mut() {
                            snapshot.state = SnapshotState::TextureReady;
                        }
                        return Ok(());
                    }
                    return Ok(());
                }
                if self.pending_sort_promise.is_none() {
                    if let Some(snapshot) = self.pending_snapshot.as_mut() {
                        if snapshot.state == SnapshotState::Sorting {
                            snapshot.state = SnapshotState::TextureReady;
                        }
                    }
                    return Ok(());
                }
                return Ok(());
            }

            if self.num_splats == 0 {
                return Ok(());
            }
            if !has_root_transform {
                return Ok(());
            }
            self.prev_view_matrix = view_matrix;
            if self.sorter_promise.is_none() {
                if !self.should_start_steady_sort(frame) {
                    return Ok(());
                }
                if self.start_sort(frame, false) {
                    return Ok(());
                }
            }
            if self.sorter_promise.is_none() {
                self.sorter_state = GaussianSplatSortingState::Waiting;
                return Ok(());
            }
            self.sorter_state = GaussianSplatSortingState::Sorting;
            return Ok(());
        } else if self.sorter_state == GaussianSplatSortingState::Waiting {
            if self.sorter_promise.is_none() && self.start_sort(frame, false) {
                return Ok(());
            }
            if self.sorter_promise.is_none() {
                self.sorter_state = GaussianSplatSortingState::Waiting;
                return Ok(());
            }
            self.sorter_state = GaussianSplatSortingState::Sorting;
            return Ok(());
        } else if self.sorter_state == GaussianSplatSortingState::Sorting {
            return Ok(());
        } else if self.sorter_state == GaussianSplatSortingState::Sorted {
            if let Some(context) = self.context.clone() {
                self.build_draw_command(&context)?;
            }
            self.sorter_state = GaussianSplatSortingState::Idle;
            self.dirty = false;
            self.sorter_promise = None;
            self.active_sort = None;
        } else if self.sorter_state == GaussianSplatSortingState::Error {
            if let Some(error) = self.sorter_error.take() {
                return Err(error);
            }
        }
        self.dirty = false;
        Ok(())
    }

    pub fn destroy(&mut self) -> Result<()> {
        if self.is_destroyed_flag {
            return Err(RuntimeError::new("This object was destroyed, i.e., destroy() was called."));
        }
        self.positions = None;
        self.rotations = None;
        self.scales = None;
        self.colors = None;
        self.indexes = None;
        Self::destroy_snapshot_textures(&mut self.pending_snapshot);
        Self::destroy_snapshot_textures(&mut self.snapshot);
        for (texture, _) in self.retired_textures.drain(..) {
            let _ = texture.destroy();
        }
        self.gaussian_splat_texture = None;
        self.spherical_harmonics_texture = None;
        self.vertex_array = None;
        self.draw_command = None;
        self.is_destroyed_flag = true;
        Ok(())
    }
}

fn state_for_gaussian_splats(mut state: crate::renderer::RenderState, _webgl2: bool) -> crate::renderer::RenderState {
    state.cull = Cull { enabled: false, face: crate::core::webgl_constants::BACK };
    state.depth_mask = false;
    state.depth_test = DepthTest { enabled: true, func: crate::core::webgl_constants::LEQUAL };
    state.blending = super::blending_state::pre_multiplied_alpha_blend();
    state
}

/// The `getCullFace` result for a splat command. Splats draw with culling off,
/// so this is informational and exposed for command validation.
pub fn gaussian_splat_cull_face(matrix: &Matrix4) -> u32 {
    if matrix.get_rotation().determinant() < 0.0 {
        crate::core::webgl_constants::FRONT
    } else {
        crate::core::webgl_constants::BACK
    }
}
