//! CesiumJS `VoxelTraversal`: tile selection, provider requests, megatexture
//! allocation and the GPU octree texture generation for a voxel primitive.
use super::{
    megatexture::Megatexture,
    spatial_node::{KeyframeLoadState, SpatialNode},
    voxel_provider::{VoxelFuture, VoxelProvider, VoxelRequest},
    voxel_shape::VoxelShape,
    Cesium3DTilesetStatistics, FrameState,
};
use crate::{
    core::{Cartesian2, Cartesian3, CullingVolume, Event, PixelFormat, RuntimeError},
    renderer::{Context, Gl, Sampler, Texture, TextureOptions, TextureRegion},
};
use std::{
    future::Future,
    pin::Pin,
    task::{Context as TaskContext, Poll, Waker},
};
use std::rc::Rc;
type Result<T> = std::result::Result<T, RuntimeError>;

/// The maximum number of provider requests in flight at once.
pub const SIMULTANEOUS_REQUEST_COUNT_MAXIMUM: usize = 50;

/// Primitive-owned events the traversal raises.
pub struct VoxelTraversalEvents<'a> {
    pub tile_load: &'a mut Event<()>,
    pub tile_visible: &'a mut Event<()>,
    pub tile_failed: &'a mut Event<()>,
    pub tile_unload: &'a mut Event<()>,
    pub load_progress: &'a mut Event<(usize, usize)>,
    pub all_tiles_loaded: &'a mut Event<()>,
    pub initial_tiles_loaded: &'a mut Event<()>,
}

/// Values the primitive supplies to each traversal update.
#[derive(Clone, Copy, Debug)]
pub struct VoxelTraversalSettings {
    pub level_blend_factor: f64,
    pub screen_space_error: f64,
    pub calculate_statistics: bool,
    pub pause_update: bool,
    pub recompute_bounding_volumes: bool,
    pub frame_number: u32,
    pub camera_position: Cartesian3,
    pub drawing_buffer_height: f64,
    pub pixel_ratio: f64,
    pub sse_denominator: f64,
}

/// A keyframe node selected by the traversal, identified by its octree path.
#[derive(Clone, Debug, PartialEq)]
pub struct QueuedKeyframe {
    pub path: Vec<u8>,
    pub keyframe: f64,
    pub priority: f64,
    pub high_priority_frame_number: i64,
}
impl Eq for QueuedKeyframe {}
impl PartialOrd for QueuedKeyframe {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl Ord for QueuedKeyframe {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.priority.total_cmp(&other.priority)
    }
}

type PriorityQueue = crate::core::DoubleEndedPriorityQueue<
    QueuedKeyframe,
    fn(&QueuedKeyframe, &QueuedKeyframe) -> f64,
>;

fn priority_comparator(a: &QueuedKeyframe, b: &QueuedKeyframe) -> f64 {
    a.priority - b.priority
}

struct PendingRequest {
    path: Vec<u8>,
    keyframe: f64,
    future: VoxelFuture,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum GpuOctreeFlag {
    Internal = 0,
    Leaf = 1,
    PackedLeafFromParent = 2,
}

/// Per-primitive voxel traversal state and GPU resources.
pub struct VoxelTraversal<G: Gl> {
    pub texture_memory_byte_length: usize,
    pub megatextures: Vec<Megatexture<G>>,
    pub simultaneous_request_count: usize,
    pub debug_print: bool,
    pub calculate_statistics: bool,
    pub frame_number: u32,
    pub root_node: SpatialNode,
    pub high_priority_keyframe_nodes: Vec<QueuedKeyframe>,
    pub high_priority_keyframe_node_count: usize,
    pub keyframe_nodes_in_megatexture: Vec<QueuedKeyframe>,
    pub keyframe_count: u32,
    pub sample_count: Option<usize>,
    pub keyframe_location: f64,
    pub binary_tree_keyframe_weighting: Vec<f64>,
    pub initial_tiles_loaded: bool,
    pub internal_node_texture: Rc<Texture<G>>,
    pub internal_node_tiles_per_row: usize,
    pub internal_node_texel_size_uv: Cartesian2,
    pub leaf_node_texture: Option<Rc<Texture<G>>>,
    pub leaf_node_tiles_per_row: Option<usize>,
    pub leaf_node_texel_size_uv: Cartesian2,
    pending: Vec<PendingRequest>,
    destroyed: bool,
}

impl<G: Gl + 'static> VoxelTraversal<G> {
    /// Long-lived resources (megatextures and the internal-node texture) are
    /// created here, matching the constructor's GPU work.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        context: &Context<G>,
        dimensions: Cartesian3,
        input_dimensions: Cartesian3,
        names_len: usize,
        types: &[super::MetadataType],
        component_types: &[super::MetadataComponentType],
        shape: &dyn VoxelShape,
        keyframe_count: u32,
        maximum_texture_memory_byte_length: Option<usize>,
        provider_tile_count: Option<usize>,
        calculate_statistics: bool,
    ) -> Result<Self> {
        let maximum_texture_memory_byte_length = maximum_texture_memory_byte_length
            .map_or(128 * 1024 * 1024, |value| value.clamp(0, 512 * 1024 * 1024));
        if types.len() != component_types.len() || types.len() != names_len {
            return Err(RuntimeError::new("Voxel provider metadata lengths do not match"));
        }
        let mut megatextures = Vec::with_capacity(types.len());
        let mut texture_memory_byte_length = 0;
        for (index, (metadata_type, component_type)) in types.iter().zip(component_types).enumerate() {
            let megatexture = Megatexture::new(
                context,
                input_dimensions,
                metadata_type.get_component_count(),
                *component_type,
                maximum_texture_memory_byte_length,
                provider_tile_count,
            )
            .map_err(|error| RuntimeError::new(format!("Megatexture {index}: {error}")))?;
            texture_memory_byte_length += megatexture.texture_memory_byte_length;
            megatextures.push(megatexture);
        }
        if megatextures.is_empty() {
            return Err(RuntimeError::new("Voxel provider must declare at least one metadata type"));
        }
        let maximum_tile_count = megatextures[0].maximum_tile_count;

        let internal_node_texels = 9usize;
        let internal_node_texture_width = 2048usize;
        let internal_node_tiles_per_row = internal_node_texture_width / internal_node_texels;
        let internal_node_texture_height = maximum_tile_count.div_ceil(internal_node_tiles_per_row).max(1);
        let mut options = TextureOptions::rgba(internal_node_texture_width as u32, internal_node_texture_height as u32);
        options.sampler = Sampler::NEAREST;
        options.flip_y = false;
        let internal_node_texture = Rc::new(Texture::new(context, options, None)?);

        let mut root_node = SpatialNode::new(0, [0, 0, 0], shape, dimensions);
        root_node.children = None;

        let mut binary_tree_keyframe_weighting = vec![0.0; keyframe_count as usize];
        if keyframe_count > 1 {
            binary_tree_weighting_recursive(&mut binary_tree_keyframe_weighting, 1, keyframe_count as i64 - 2, 0);
        }

        Ok(Self {
            texture_memory_byte_length,
            megatextures,
            simultaneous_request_count: 0,
            debug_print: false,
            calculate_statistics,
            frame_number: 0,
            root_node,
            high_priority_keyframe_nodes: Vec::new(),
            high_priority_keyframe_node_count: 0,
            keyframe_nodes_in_megatexture: Vec::new(),
            keyframe_count,
            sample_count: None,
            keyframe_location: 0.0,
            binary_tree_keyframe_weighting,
            initial_tiles_loaded: false,
            internal_node_texture,
            internal_node_tiles_per_row,
            internal_node_texel_size_uv: Cartesian2::new(
                1.0 / internal_node_texture_width as f64,
                1.0 / internal_node_texture_height as f64,
            ),
            leaf_node_texture: None,
            leaf_node_tiles_per_row: None,
            leaf_node_texel_size_uv: Cartesian2::ZERO,
            pending: Vec::new(),
            destroyed: false,
        })
    }

    /// The largest megatexture tile count, used to size the internal texture.
    pub fn maximum_tile_count(&self) -> usize {
        self.megatextures[0].maximum_tile_count
    }

    pub fn find_keyframe_node(&self, megatexture_index: isize) -> Option<&super::spatial_node::KeyframeNode> {
        fn find(node: &SpatialNode, index: isize) -> Option<&super::spatial_node::KeyframeNode> {
            if let Some(found) = node.keyframe_nodes.iter().find(|k| k.megatexture_index == index) {
                return Some(found);
            }
            node.children.as_ref()?.iter().find_map(|child| find(child, index))
        }
        find(&self.root_node, megatexture_index)
    }

    /// Selects tiles, resolves requests, allocates megatexture slots and
    /// regenerates the octree textures.
    #[allow(clippy::too_many_arguments)]
    pub fn update(
        &mut self,
        frame: &mut FrameState,
        culling_volume: &CullingVolume,
        provider: &mut dyn VoxelProvider,
        shape: &dyn VoxelShape,
        settings: VoxelTraversalSettings,
        statistics: &mut Cesium3DTilesetStatistics,
        events: &mut VoxelTraversalEvents<'_>,
        waker: Option<Waker>,
    ) -> Result<()> {
        if self.destroyed {
            return Err(RuntimeError::new("VoxelTraversal was destroyed"));
        }
        let context = frame
            .context
            .as_ref()
            .and_then(|resource| resource.get::<Context<G>>())
            .ok_or_else(|| RuntimeError::new("VoxelTraversal requires the frame's renderer context"))?;
        let has_level_blend = settings.level_blend_factor > 0.0;
        let has_keyframes = self.keyframe_count > 1;
        let sample_count = (if has_level_blend { 2 } else { 1 }) * (if has_keyframes { 2 } else { 1 });
        self.sample_count = Some(sample_count);

        let use_leaf_nodes = sample_count >= 2;
        if use_leaf_nodes && self.leaf_node_texture.is_none() {
            let leaf_texels = 2usize;
            let leaf_width = 1024usize;
            let tiles_per_row = leaf_width / leaf_texels;
            let height = self.maximum_tile_count().div_ceil(tiles_per_row).max(1);
            let mut options = TextureOptions::rgba(leaf_width as u32, height as u32);
            options.sampler = Sampler::NEAREST;
            options.flip_y = false;
            let texture = Rc::new(Texture::new(context, options, None)?);
            self.leaf_node_texture = Some(texture);
            self.leaf_node_tiles_per_row = Some(tiles_per_row);
            self.leaf_node_texel_size_uv = Cartesian2::new(1.0 / leaf_width as f64, 1.0 / height as f64);
        } else if !use_leaf_nodes {
            if let Some(texture) = self.leaf_node_texture.take() {
                texture.destroy()?;
            }
            self.leaf_node_tiles_per_row = None;
        }

        if self.keyframe_count == 0 {
            return Err(RuntimeError::new("Voxel traversal requires at least one keyframe"));
        }
        self.keyframe_location = self.keyframe_location.clamp(0.0, self.keyframe_count as f64 - 1.0);

        if settings.recompute_bounding_volumes {
            recompute_bounding_volumes_recursive(&mut self.root_node, shape);
        }
        if settings.pause_update {
            return Ok(());
        }

        self.frame_number = settings.frame_number;
        self.resolve_pending_requests(events);
        self.select_keyframe_nodes(provider, shape, settings, culling_volume);
        self.update_keyframe_nodes(frame, provider, shape, settings, statistics, events)?;
        self.generate_octree(shape, settings, events)?;
        self.post_passes_update(statistics, settings, events);
        let _ = waker;
        Ok(())
    }

    /// Sets the clamped keyframe location before `update`.
    pub fn set_keyframe_location(&mut self, location: f64) {
        self.keyframe_location = location.clamp(0.0, (self.keyframe_count as f64 - 1.0).max(0.0));
    }

    fn resolve_pending_requests(&mut self, events: &mut VoxelTraversalEvents<'_>) {
        let mut remaining = Vec::new();
        let mut ready = Vec::new();
        let waker = noop_waker();
        let mut cx = TaskContext::from_waker(&waker);
        for mut pending in std::mem::take(&mut self.pending) {
            match Pin::new(&mut pending.future).poll(&mut cx) {
                Poll::Ready(result) => {
                    self.simultaneous_request_count = self.simultaneous_request_count.saturating_sub(1);
                    ready.push((pending.path, pending.keyframe, result));
                }
                Poll::Pending => remaining.push(pending),
            }
        }
        self.pending = remaining;
        for (path, keyframe, result) in ready {
            match result {
                Ok(content) => {
                    if let Some(node) = node_mut(&mut self.root_node, &path) {
                        if let Ok(index) = node.keyframe_nodes.binary_search_by(|n| n.keyframe.total_cmp(&keyframe)) {
                            node.keyframe_nodes[index].content = Some(content);
                            node.keyframe_nodes[index].state = KeyframeLoadState::Processing;
                        }
                    }
                }
                Err(_) => {
                    if let Some(node) = node_mut(&mut self.root_node, &path) {
                        if let Ok(index) = node.keyframe_nodes.binary_search_by(|n| n.keyframe.total_cmp(&keyframe)) {
                            node.keyframe_nodes[index].content = None;
                            node.keyframe_nodes[index].state = KeyframeLoadState::Failed;
                        }
                    }
                    events.tile_failed.raise_event(&());
                }
            }
        }
    }

    fn select_keyframe_nodes(
        &mut self,
        _provider: &dyn VoxelProvider,
        shape: &dyn VoxelShape,
        settings: VoxelTraversalSettings,
        culling_volume: &CullingVolume,
    ) {
        let frame_number = self.frame_number as i64;
        let mut queue: PriorityQueue = crate::core::DoubleEndedPriorityQueue::new(
            priority_comparator as fn(&QueuedKeyframe, &QueuedKeyframe) -> f64,
            Some(self.maximum_tile_count()),
        );
        let mut root = std::mem::replace(
            &mut self.root_node,
            SpatialNode::new(0, [0, 0, 0], shape, Cartesian3::new(1., 1., 1.)),
        );
        let frame = TraversalFrame {
            frame_number,
            camera_position: settings.camera_position,
            screen_height: settings.drawing_buffer_height / settings.pixel_ratio,
            sse_denominator: settings.sse_denominator,
            culling_volume,
            keyframe_count: self.keyframe_count,
            keyframe_location: self.keyframe_location,
            screen_space_error: settings.screen_space_error,
            binary_tree_keyframe_weighting: self.binary_tree_keyframe_weighting.clone(),
        };
        add_to_queue_recursive(
            &mut root,
            &[],
            CullingVolume::MASK_INDETERMINATE,
            &frame,
            &mut queue,
            shape,
        );
        self.root_node = root;

        let mut high = Vec::new();
        while let Some(mut item) = queue.remove_maximum() {
            item.high_priority_frame_number = frame_number;
            if let Some(node) = node_mut(&mut self.root_node, &item.path) {
                if let Ok(index) = node.keyframe_nodes.binary_search_by(|n| n.keyframe.total_cmp(&item.keyframe)) {
                    node.keyframe_nodes[index].high_priority_frame_number = frame_number;
                }
            }
            high.push(item);
        }
        self.high_priority_keyframe_node_count = high.len();
        self.high_priority_keyframe_nodes = high;
    }

    fn update_keyframe_nodes(
        &mut self,
        frame: &mut FrameState,
        provider: &mut dyn VoxelProvider,
        shape: &dyn VoxelShape,
        settings: VoxelTraversalSettings,
        _statistics: &mut Cesium3DTilesetStatistics,
        events: &mut VoxelTraversalEvents<'_>,
    ) -> Result<()> {
        let mut in_megatexture = std::mem::take(&mut self.keyframe_nodes_in_megatexture);
        let occupied_count = self.megatextures[0].occupied_count;
        in_megatexture.sort_by(|a, b| {
            b.high_priority_frame_number
                .cmp(&a.high_priority_frame_number)
                .then_with(|| b.priority.total_cmp(&a.priority))
        });
        in_megatexture.truncate(occupied_count);

        let high = std::mem::take(&mut self.high_priority_keyframe_nodes);
        let high_count = self.high_priority_keyframe_node_count;
        let mut destroyed_count = 0usize;
        let mut added_count = 0usize;
        let mut tile_failed = false;

        for item in high.iter().take(high_count) {
            let Some((keyframe_index, state)) = node(&self.root_node, &item.path).and_then(|spatial| {
                let index = spatial
                    .keyframe_nodes
                    .iter()
                    .position(|k| k.keyframe == item.keyframe)?;
                Some((index, spatial.keyframe_nodes[index].state))
            }) else {
                continue;
            };
            if state == KeyframeLoadState::Loaded {
                continue;
            }
            if state == KeyframeLoadState::Unloaded {
                self.request_data(provider, item);
            }
            let state = node(&self.root_node, &item.path)
                .and_then(|n| n.keyframe_nodes.get(keyframe_index))
                .map_or(KeyframeLoadState::Unavailable, |k| k.state);
            if state != KeyframeLoadState::Processing {
                continue;
            }
            let ready = node_mut(&mut self.root_node, &item.path)
                .and_then(|n| n.keyframe_nodes.get_mut(keyframe_index))
                .and_then(|k| k.content.as_mut())
                .map(|content| content.update(frame).map(|_| content.ready()));
            let ready = match ready {
                Some(Ok(ready)) => ready,
                Some(Err(error)) => return Err(error),
                None => continue,
            };
            if !ready {
                continue;
            }
            if !validate_metadata(self, &item.path, item.keyframe, self.megatextures.len()) {
                if let Some(node) = node_mut(&mut self.root_node, &item.path) {
                    if let Some(keyframe) = node.keyframe_nodes.get_mut(keyframe_index) {
                        keyframe.content = None;
                        keyframe.state = KeyframeLoadState::Failed;
                    }
                }
                tile_failed = true;
                continue;
            }

            let add_index;
            if self.megatextures.iter().any(|m| m.is_full()) {
                add_index = in_megatexture.len().saturating_sub(1 + destroyed_count);
                destroyed_count += 1;
                events.tile_unload.raise_event(&());
                if let Some(discard) = in_megatexture.get(add_index).cloned() {
                    if let Some(node) = node_mut(&mut self.root_node, &discard.path) {
                        let _ = node.destroy_keyframe_node(discard.keyframe, &mut self.megatextures);
                    }
                }
                if add_index >= in_megatexture.len() {
                    in_megatexture.push(item.clone());
                } else {
                    in_megatexture[add_index] = item.clone();
                }
            } else {
                add_index = in_megatexture.len() + added_count;
                added_count += 1;
                if add_index >= in_megatexture.len() {
                    in_megatexture.push(item.clone());
                } else {
                    in_megatexture[add_index] = item.clone();
                }
            }

            if let Some(node) = node_mut(&mut self.root_node, &item.path) {
                if node
                    .add_keyframe_node_to_megatextures(item.keyframe, &mut self.megatextures)
                    .is_ok()
                {
                    if let Some(keyframe) = node.keyframe_nodes.get_mut(keyframe_index) {
                        keyframe.state = KeyframeLoadState::Loaded;
                    }
                }
            }
            events.tile_load.raise_event(&());
        }
        self.high_priority_keyframe_nodes = high;
        self.keyframe_nodes_in_megatexture = in_megatexture;
        if tile_failed {
            events.tile_failed.raise_event(&());
        }
        let _ = shape;
        let _ = settings;
        Ok(())
    }

    fn request_data(&mut self, provider: &mut dyn VoxelProvider, item: &QueuedKeyframe) {
        if self.simultaneous_request_count >= SIMULTANEOUS_REQUEST_COUNT_MAXIMUM {
            return;
        }
        let request = {
            let Some(spatial) = node(&self.root_node, &item.path) else { return };
            if !spatial.keyframe_nodes.iter().any(|k| k.keyframe == item.keyframe) {
                return;
            }
            if provider.available_levels().is_some_and(|levels| spatial.level >= levels) {
                return;
            }
            VoxelRequest {
                tile_level: spatial.level,
                tile_x: spatial.coords[0],
                tile_y: spatial.coords[1],
                tile_z: spatial.coords[2],
                keyframe: item.keyframe as u32,
            }
        };
        if let Some(future) = provider.request_data(request) {
            self.simultaneous_request_count += 1;
            if let Some(spatial) = node_mut(&mut self.root_node, &item.path) {
                if let Ok(index) = spatial.keyframe_nodes.binary_search_by(|n| n.keyframe.total_cmp(&item.keyframe)) {
                    spatial.keyframe_nodes[index].state = KeyframeLoadState::Receiving;
                }
            }
            self.pending.push(PendingRequest { path: item.path.clone(), keyframe: item.keyframe, future });
        }
    }

    fn generate_octree(
        &mut self,
        shape: &dyn VoxelShape,
        settings: VoxelTraversalSettings,
        events: &mut VoxelTraversalEvents<'_>,
    ) -> Result<()> {
        let target_sse = settings.screen_space_error;
        let keyframe_location = self.keyframe_location;
        let frame_number = self.frame_number as i64;
        let sample_count = self.sample_count.unwrap_or(1);
        let use_leaf_nodes = sample_count >= 2;
        let level_blend_factor = settings.level_blend_factor;

        let mut internal_node_count = 0usize;
        let mut leaf_node_count = 0usize;
        let mut internal_data: Vec<u32> = Vec::new();
        let mut leaf_data: Vec<f64> = Vec::new();

        let mut root = std::mem::replace(
            &mut self.root_node,
            SpatialNode::new(0, [0, 0, 0], shape, Cartesian3::new(1., 1., 1.)),
        );
        compute_surrounding_recursive(&mut root, keyframe_location, &[]);
        let mut tile_visible = 0usize;
        if root.is_renderable(frame_number) {
            let mut state = OctreeBuildState {
                internal_node_count: &mut internal_node_count,
                leaf_node_count: &mut leaf_node_count,
                internal_data: &mut internal_data,
                leaf_data: &mut leaf_data,
                use_leaf_nodes,
                target_sse,
                level_blend_factor,
                tile_visible: &mut tile_visible,
                frame_number,
            };
            build_octree_recursive(
                &root,
                shape,
                &mut state,
                OctreeIndices {
                    child_octree_index: 0,
                    child_entry_index: 0,
                    parent_octree_index: 0,
                    parent_entry_index: 0,
                },
            );
        }
        self.root_node = root;
        for _ in 0..tile_visible {
            events.tile_visible.raise_event(&());
        }

        copy_to_internal_node_texture(
            &internal_data,
            9,
            self.internal_node_tiles_per_row,
            &self.internal_node_texture,
        )?;
        if use_leaf_nodes {
            if let (Some(texture), Some(tiles_per_row)) = (&self.leaf_node_texture, self.leaf_node_tiles_per_row) {
                copy_to_leaf_node_texture(&leaf_data, 2, tiles_per_row, texture)?;
            }
        }
        Ok(())
    }

    fn post_passes_update(
        &mut self,
        statistics: &mut Cesium3DTilesetStatistics,
        settings: VoxelTraversalSettings,
        events: &mut VoxelTraversalEvents<'_>,
    ) {
        let load_state_count = 6usize;
        let mut load_states_by_keyframe = vec![vec![0usize; self.keyframe_count as usize]; load_state_count];
        let mut load_state_by_count = vec![0usize; load_state_count];
        let mut node_count_total = 0usize;
        fn traverse(
            node: &SpatialNode,
            by_keyframe: &mut [Vec<usize>],
            by_count: &mut [usize],
            total: &mut usize,
        ) {
            for keyframe in &node.keyframe_nodes {
                let state = keyframe.state as usize;
                by_keyframe[state][keyframe.keyframe as usize] += 1;
                by_count[state] += 1;
                *total += 1;
            }
            if let Some(children) = &node.children {
                for child in children {
                    traverse(child, by_keyframe, by_count, total);
                }
            }
        }
        traverse(
            &self.root_node,
            &mut load_states_by_keyframe,
            &mut load_state_by_count,
            &mut node_count_total,
        );
        statistics.number_of_tiles_with_content_ready = load_state_by_count[KeyframeLoadState::Loaded as usize];
        statistics.visited = node_count_total;
        let pending = load_state_by_count[KeyframeLoadState::Receiving as usize];
        let processing = load_state_by_count[KeyframeLoadState::Processing as usize];
        let progress_changed =
            pending != statistics.number_of_pending_requests || processing != statistics.number_of_tiles_processing;
        if progress_changed {
            events.load_progress.raise_event(&(pending, processing));
        }
        statistics.number_of_pending_requests = pending;
        statistics.number_of_tiles_processing = processing;
        let tiles_loaded = pending == 0 && processing == 0;
        if progress_changed && tiles_loaded {
            events.all_tiles_loaded.raise_event(&());
            if !self.initial_tiles_loaded {
                self.initial_tiles_loaded = true;
                events.initial_tiles_loaded.raise_event(&());
            }
        }
        let _ = settings;
    }

    pub fn is_renderable(&self, tile: &SpatialNode) -> bool {
        tile.is_renderable(self.frame_number as i64)
    }

    pub fn is_destroyed(&self) -> bool {
        self.destroyed
    }

    pub fn destroy(&mut self) -> Result<()> {
        for megatexture in &mut self.megatextures {
            megatexture.destroy()?;
        }
        self.texture_memory_byte_length = 0;
        self.internal_node_texture.destroy()?;
        if let Some(texture) = self.leaf_node_texture.take() {
            texture.destroy()?;
        }
        self.pending.clear();
        self.destroyed = true;
        Ok(())
    }
}

struct TraversalFrame<'a> {
    frame_number: i64,
    camera_position: Cartesian3,
    screen_height: f64,
    sse_denominator: f64,
    culling_volume: &'a CullingVolume,
    keyframe_count: u32,
    keyframe_location: f64,
    screen_space_error: f64,
    binary_tree_keyframe_weighting: Vec<f64>,
}

fn node<'a>(root: &'a SpatialNode, path: &[u8]) -> Option<&'a SpatialNode> {
    let mut current = root;
    for child in path {
        current = current.children.as_ref()?.get(*child as usize)?;
    }
    Some(current)
}

fn node_mut<'a>(root: &'a mut SpatialNode, path: &[u8]) -> Option<&'a mut SpatialNode> {
    let mut current = root;
    for child in path {
        current = current.children.as_mut()?.get_mut(*child as usize)?;
    }
    Some(current)
}

fn recompute_bounding_volumes_recursive(node: &mut SpatialNode, shape: &dyn VoxelShape) {
    node.compute_bounding_volumes(shape);
    if let Some(children) = &mut node.children {
        for child in children {
            recompute_bounding_volumes_recursive(child, shape);
        }
    }
}

fn compute_surrounding_recursive(node: &mut SpatialNode, location: f64, ancestors: &[&SpatialNode]) {
    node.compute_surrounding_renderable_keyframe_nodes(location, ancestors);
    // Ancestor selections are read-only while descendants are updated; each
    // child sees the parent chain computed above it.
    let snapshot = node.clone();
    let mut chain: Vec<&SpatialNode> = ancestors.to_vec();
    chain.insert(0, &snapshot);
    if let Some(children) = node.children.as_mut() {
        for child in children {
            compute_surrounding_recursive(child, location, &chain);
        }
    }
}

fn add_to_queue_recursive(
    spatial_node: &mut SpatialNode,
    path: &[u8],
    visibility_plane_mask: u32,
    frame: &TraversalFrame<'_>,
    queue: &mut PriorityQueue,
    shape: &dyn VoxelShape,
) {
    let screen_space_error_multiplier = frame.screen_height / frame.sse_denominator;
    spatial_node.compute_screen_space_error(frame.camera_position, screen_space_error_multiplier);
    let mut mask = spatial_node.visibility(frame.culling_volume, visibility_plane_mask);
    if mask == CullingVolume::MASK_OUTSIDE {
        return;
    }
    spatial_node.visited_frame_number = frame.frame_number;
    // Reuse the parent's mask for the first requested child; all children get
    // the same value upstream.
    let _ = &mut mask;
    let _ = queue.len();

    let previous_keyframe = (frame.keyframe_location.floor() as i64)
        .clamp(0, (frame.keyframe_count as i64 - 2).max(0)) as f64;
    let next_keyframe = previous_keyframe + 1.0;
    if frame.keyframe_count == 1 {
        spatial_node.create_keyframe_node(0.0);
    } else if spatial_node.keyframe_nodes.len() != frame.keyframe_count as usize {
        for keyframe in 0..frame.keyframe_count {
            spatial_node.create_keyframe_node(keyframe as f64);
        }
    }
    let screen_space_error = spatial_node.screen_space_error;
    let sse_priority = map_infinite_range_to_zero_one(screen_space_error);
    let mut has_loaded_keyframe = false;
    for keyframe_node in &mut spatial_node.keyframe_nodes {
        keyframe_node.priority = 10.0 * sse_priority
            + keyframe_priority_from_weighting(
                previous_keyframe,
                keyframe_node.keyframe,
                next_keyframe,
                frame.keyframe_count,
                &frame.binary_tree_keyframe_weighting,
            );
        if keyframe_node.state != KeyframeLoadState::Unavailable
            && keyframe_node.state != KeyframeLoadState::Failed
            && keyframe_node.priority != -f64::MAX
        {
            queue.insert(QueuedKeyframe {
                path: path.to_vec(),
                keyframe: keyframe_node.keyframe,
                priority: keyframe_node.priority,
                high_priority_frame_number: -1,
            });
        }
        if keyframe_node.state == KeyframeLoadState::Loaded {
            has_loaded_keyframe = true;
        }
    }

    if screen_space_error < frame.screen_space_error || !has_loaded_keyframe {
        spatial_node.children = None;
        return;
    }
    if spatial_node.children.is_none() {
        let mut children = spatial_node.construct_child_nodes(shape);
        for child in &mut children {
            child.parent_level = Some(spatial_node.level);
            child.parent_screen_space_error = spatial_node.screen_space_error;
        }
        spatial_node.children = Some(children);
    }
    let children_len = spatial_node.children.as_ref().unwrap().len();
    for child_index in 0..children_len {
        let child = &mut spatial_node.children.as_mut().unwrap()[child_index];
        let mut child_path = path.to_vec();
        child_path.push(child_index as u8);
        add_to_queue_recursive(child, &child_path, visibility_plane_mask, frame, queue, shape);
    }
}

fn map_infinite_range_to_zero_one(x: f64) -> f64 {
    x / (1.0 + x)
}

fn binary_tree_weighting_recursive(arr: &mut [f64], start: i64, end: i64, depth: i64) {
    if start > end {
        return;
    }
    let mid = (start + end).div_euclid(2);
    arr[mid as usize] = depth as f64;
    binary_tree_weighting_recursive(arr, start, mid - 1, depth + 1);
    binary_tree_weighting_recursive(arr, mid + 1, end, depth + 1);
}

fn keyframe_priority_from_weighting(
    previous_keyframe: f64,
    keyframe: f64,
    next_keyframe: f64,
    keyframe_count: u32,
    weighting: &[f64],
) -> f64 {
    let keyframe_difference = (keyframe - previous_keyframe).abs().min((keyframe - next_keyframe).abs());
    let max_keyframe_difference = previous_keyframe
        .max(keyframe_count as f64 - next_keyframe - 1.0)
        .max(1.0);
    let keyframe_factor = (1.0 - keyframe_difference / max_keyframe_difference).powf(4.0);
    let binary_tree_factor = (-weighting.get(keyframe as usize).copied().unwrap_or(0.0)).exp();
    binary_tree_factor + (keyframe_factor - binary_tree_factor) * (0.15 + 0.85 * keyframe_factor)
}

fn validate_metadata(
    traversal: &VoxelTraversal<impl Gl>,
    path: &[u8],
    keyframe: f64,
    types_len: usize,
) -> bool {
    let Some(node) = node(&traversal.root_node, path) else { return false };
    let Some(keyframe_node) = node.keyframe_nodes.iter().find(|k| k.keyframe == keyframe) else {
        return false;
    };
    let Some(content) = keyframe_node.content.as_ref() else { return false };
    let metadata = content.metadata();
    if metadata.len() != types_len {
        return false;
    }
    for (index, megatexture) in traversal.megatextures.iter().enumerate() {
        let tile_voxel_count = megatexture.voxel_count_per_tile.x
            * megatexture.voxel_count_per_tile.y
            * megatexture.voxel_count_per_tile.z;
        let expected = tile_voxel_count as usize * megatexture.channel_count;
        if metadata[index].len() != expected {
            return false;
        }
    }
    true
}

struct OctreeIndices {
    child_octree_index: usize,
    child_entry_index: usize,
    parent_octree_index: usize,
    parent_entry_index: usize,
}

struct OctreeBuildState<'a> {
    internal_node_count: &'a mut usize,
    leaf_node_count: &'a mut usize,
    internal_data: &'a mut Vec<u32>,
    leaf_data: &'a mut Vec<f64>,
    use_leaf_nodes: bool,
    target_sse: f64,
    level_blend_factor: f64,
    tile_visible: &'a mut usize,
    frame_number: i64,
}

fn build_octree_recursive(
    node: &SpatialNode,
    shape: &dyn VoxelShape,
    state: &mut OctreeBuildState<'_>,
    indices: OctreeIndices,
) {
    let mut has_renderable_children = false;
    if let Some(children) = &node.children {
        for child in children {
            if child.is_renderable(state.frame_number) {
                has_renderable_children = true;
            }
        }
    }
    let OctreeIndices {
        child_octree_index,
        child_entry_index,
        parent_octree_index,
        parent_entry_index,
    } = indices;
    if has_renderable_children {
        set_u32(state.internal_data, parent_entry_index, ((GpuOctreeFlag::Internal as u32) << 16) | child_octree_index as u32);
        set_u32(state.internal_data, child_entry_index, parent_octree_index as u32);
        *state.internal_node_count += 1;
        let next_parent_octree_index = child_octree_index;
        let next_parent_entry_index = next_parent_octree_index * 9 + 1;
        let children = node.children.as_ref().unwrap();
        for (child_index, child) in children.iter().enumerate() {
            build_octree_recursive(
                child,
                shape,
                state,
                OctreeIndices {
                    child_octree_index: *state.internal_node_count,
                    child_entry_index: *state.internal_node_count * 9,
                    parent_octree_index: next_parent_octree_index,
                    parent_entry_index: next_parent_entry_index + child_index,
                },
            );
        }
    } else {
        *state.tile_visible += 1;
        if state.use_leaf_nodes {
            let base = *state.leaf_node_count * 5;
            let Some(previous) = node.previous.as_ref() else { return };
            let level_difference = node.level as f64 - previous.level as f64;
            let parent_megatexture = if node.previous_parent_megatexture_index >= 0 {
                node.previous_parent_megatexture_index
            } else {
                node.previous_megatexture_index
            };
            let lod_lerp = get_lod_lerp(node, state.target_sse, state.level_blend_factor);
            set_f64(state.leaf_data, base, lod_lerp);
            set_f64(state.leaf_data, base + 1, level_difference);
            set_f64(state.leaf_data, base + 2, 1.0);
            set_f64(state.leaf_data, base + 3, node.previous_megatexture_index as f64);
            set_f64(state.leaf_data, base + 4, parent_megatexture as f64);
            set_u32(
                state.internal_data,
                parent_entry_index,
                ((GpuOctreeFlag::Leaf as u32) << 16) | *state.leaf_node_count as u32,
            );
        } else {
            let Some(previous) = node.previous.as_ref() else { return };
            let level_difference = node.level as i64 - previous.level as i64;
            let flag = if level_difference == 0 {
                GpuOctreeFlag::Leaf
            } else {
                GpuOctreeFlag::PackedLeafFromParent
            };
            set_u32(
                state.internal_data,
                parent_entry_index,
                ((flag as u32) << 16) | (node.previous_megatexture_index.max(0) as u32),
            );
        }
        *state.leaf_node_count += 1;
        let _ = shape;
    }
}

fn set_u32(data: &mut Vec<u32>, index: usize, value: u32) {
    if data.len() <= index {
        data.resize(index + 1, 0);
    }
    data[index] = value;
}
fn set_f64(data: &mut Vec<f64>, index: usize, value: f64) {
    if data.len() <= index {
        data.resize(index + 1, 0.0);
    }
    data[index] = value;
}

fn get_lod_lerp(node: &SpatialNode, target_sse: f64, level_blend_factor: f64) -> f64 {
    if node.parent_level.is_none() {
        return 0.0;
    }
    let sse = node.screen_space_error;
    let parent_sse = node.parent_screen_space_error;
    let lod_lerp = (target_sse - sse) / (parent_sse - sse);
    let blended = (lod_lerp + level_blend_factor - 1.0) / level_blend_factor;
    blended.clamp(0.0, 1.0)
}

fn copy_to_internal_node_texture<G: Gl>(
    data: &[u32],
    texels_per_tile: usize,
    tiles_per_row: usize,
    texture: &Texture<G>,
) -> Result<()> {
    let channel_count = PixelFormat::components_length(texture.pixel_format());
    let tile_count = data.len().div_ceil(texels_per_tile);
    let copy_width = (texels_per_tile * tile_count.min(tiles_per_row)).max(1);
    let copy_height = tile_count.div_ceil(tiles_per_row).max(1);
    let mut texture_data = vec![0u8; copy_width * copy_height * channel_count];
    for (index, value) in data.iter().enumerate() {
        let start = index * channel_count;
        for channel in 0..channel_count {
            if start + channel < texture_data.len() {
                texture_data[start + channel] = ((value >> (channel * 8)) & 0xff) as u8;
            }
        }
    }
    texture.copy_from(
        &texture_data,
        TextureRegion { x: 0, y: 0, width: copy_width as u32, height: copy_height as u32 },
    )
}

fn copy_to_leaf_node_texture<G: Gl>(
    data: &[f64],
    texels_per_tile: usize,
    tiles_per_row: usize,
    texture: &Texture<G>,
) -> Result<()> {
    let channel_count = PixelFormat::components_length(texture.pixel_format());
    let datas_per_tile = 5usize;
    let tile_count = data.len().div_ceil(datas_per_tile);
    let copy_width = (texels_per_tile * tile_count.min(tiles_per_row)).max(1);
    let copy_height = tile_count.div_ceil(tiles_per_row).max(1);
    let mut texture_data = vec![0u8; copy_width * copy_height * channel_count];
    for tile_index in 0..tile_count {
        let value = |offset: usize| data.get(tile_index * datas_per_tile + offset).copied().unwrap_or(0.0);
        let time_lerp = value(0);
        let previous_levels = value(1);
        let next_levels = value(2);
        let previous_index = value(3);
        let next_index = value(4);
        let compressed = if time_lerp.is_finite() {
            (65536.0 * time_lerp).floor().clamp(0.0, 65535.0) as u32
        } else {
            0
        };
        let base = tile_index * 8;
        texture_data[base] = (compressed & 0xff) as u8;
        texture_data[base + 1] = ((compressed >> 8) & 0xff) as u8;
        texture_data[base + 2] = (previous_levels as i64 & 0xff) as u8;
        texture_data[base + 3] = (next_levels as i64 & 0xff) as u8;
        texture_data[base + 4] = (previous_index as i64 & 0xff) as u8;
        texture_data[base + 5] = ((previous_index as i64 >> 8) & 0xff) as u8;
        texture_data[base + 6] = (next_index as i64 & 0xff) as u8;
        texture_data[base + 7] = ((next_index as i64 >> 8) & 0xff) as u8;
    }
    texture.copy_from(
        &texture_data,
        TextureRegion { x: 0, y: 0, width: copy_width as u32, height: copy_height as u32 },
    )
}

fn noop_waker() -> Waker {
    use std::task::{RawWaker, RawWakerVTable};
    fn clone(_: *const ()) -> RawWaker {
        RawWaker::new(std::ptr::null(), &VTABLE)
    }
    fn wake(_: *const ()) {}
    fn wake_by_ref(_: *const ()) {}
    fn drop(_: *const ()) {}
    static VTABLE: RawWakerVTable = RawWakerVTable::new(clone, wake, wake_by_ref, drop);
    unsafe { Waker::from_raw(RawWaker::new(std::ptr::null(), &VTABLE)) }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn binary_tree_weighting_matches_a_balanced_layout() {
        let mut weighting = vec![0.0; 7];
        binary_tree_weighting_recursive(&mut weighting, 1, 5, 0);
        assert_eq!(weighting, vec![0.0, 1.0, 2.0, 0.0, 1.0, 2.0, 0.0]);
    }

    #[test]
    fn keyframe_priority_prefers_temporal_neighbours() {
        let weighting = vec![0.0, 2.0, 1.0, 2.0, 0.0];
        let near = keyframe_priority_from_weighting(1.0, 1.0, 2.0, 5, &weighting);
        let far = keyframe_priority_from_weighting(1.0, 4.0, 2.0, 5, &weighting);
        assert!(near > far);
    }

    #[test]
    fn map_infinite_range_is_monotonic() {
        assert_eq!(map_infinite_range_to_zero_one(0.0), 0.0);
        assert!(map_infinite_range_to_zero_one(1.0e9) > 0.999999);
        assert!(map_infinite_range_to_zero_one(2.0) > 0.66);
    }
}
