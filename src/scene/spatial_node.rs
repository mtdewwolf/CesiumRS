//! CesiumJS `SpatialNode`: octree bounds, screen error, keyframe lifecycle,
//! megatexture slot allocation, and temporal ancestor selection.
use super::{voxel_content::VoxelContent, voxel_shape::VoxelShape};
use crate::core::{Cartesian3, CullingVolume, OrientedBoundingBox, RuntimeError, TypedArray};
use std::cmp::Ordering;

/// A renderable keyframe entry used in temporal ancestor selection.
#[derive(Clone, Debug, PartialEq)]
pub struct RenderableKeyframe {
    pub level: u32,
    pub coords: [u32; 3],
    pub keyframe: f64,
}

/// Lifecycle state of a [`KeyframeNode`], matching CesiumJS `KeyframeNode.LoadState`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum KeyframeLoadState {
    Unloaded = 0,
    Receiving = 1,
    Processing = 2,
    Loaded = 3,
    Failed = 4,
    Unavailable = 5,
}

/// A single temporal keyframe attached to a [`SpatialNode`], matching CesiumJS `KeyframeNode`.
#[derive(Clone, Debug)]
pub struct KeyframeNode {
    pub level: u32,
    pub coords: [u32; 3],
    pub keyframe: f64,
    pub state: KeyframeLoadState,
    pub content: Option<VoxelContent>,
    pub megatexture_index: isize,
    pub priority: f64,
    pub high_priority_frame_number: i64,
}

impl KeyframeNode {
    pub fn new(level: u32, coords: [u32; 3], keyframe: f64) -> Self {
        Self {
            level,
            coords,
            keyframe,
            state: KeyframeLoadState::Unloaded,
            content: None,
            megatexture_index: -1,
            priority: -f64::MAX,
            high_priority_frame_number: -1,
        }
    }

    pub fn search_comparator(a: &Self, b: &Self) -> Ordering {
        a.keyframe.total_cmp(&b.keyframe)
    }

    pub fn unload(&mut self) {
        if let Some(mut content) = self.content.take() {
            content.destroy();
        }
        self.megatexture_index = -1;
        self.state = KeyframeLoadState::Unloaded;
        self.priority = -f64::MAX;
        self.high_priority_frame_number = -1;
    }

    /// `KeyframeNode.priorityComparator`, in Cesium's arithmetic form.
    pub fn priority_comparator(a: &Self, b: &Self) -> f64 {
        a.priority - b.priority
    }

    /// `KeyframeNode.searchComparator`.
    pub fn search_comparator_distance(a: &Self, b: &Self) -> f64 {
        a.keyframe - b.keyframe
    }
}

/// Minimal megatexture slot-allocation contract matching CesiumJS `Megatexture#add`/`remove`.
pub trait MegatextureSlots {
    fn add(&mut self, data: &TypedArray) -> isize;
    fn remove(&mut self, index: isize);
}

#[derive(Clone, Debug)]
pub struct SpatialNode {
    pub level: u32,
    pub coords: [u32; 3],
    pub dimensions: Cartesian3,
    pub oriented_bounding_box: OrientedBoundingBox,
    pub approximate_voxel_size: f64,
    pub screen_space_error: f64,
    pub visited_frame_number: i64,
    pub keyframes: Vec<f64>,
    pub keyframe_nodes: Vec<KeyframeNode>,
    pub renderable_keyframes: Vec<RenderableKeyframe>,
    /// Megatexture slots parallel to `renderable_keyframes`. It is separate so
    /// `RenderableKeyframe` remains the public value used by ancestor selection.
    pub renderable_megatexture_indices: Vec<isize>,
    pub previous: Option<RenderableKeyframe>,
    pub next: Option<RenderableKeyframe>,
    /// Megatexture index of the selected previous/next renderable keyframe.
    pub previous_megatexture_index: isize,
    pub next_megatexture_index: isize,
    /// Megatexture index of the owner's parent previous selection, used by
    /// `generateOctree`'s parent level blend.
    pub previous_parent_megatexture_index: isize,
    pub next_parent_megatexture_index: isize,
    pub lerp: f64,
    /// Constructed octree children, or `None` when pruned.
    pub children: Option<Vec<SpatialNode>>,
    /// Parent level and screen-space error, used for level blending.
    pub parent_level: Option<u32>,
    pub parent_screen_space_error: f64,
}

impl SpatialNode {
    pub fn new(level: u32, coords: [u32; 3], shape: &dyn VoxelShape, dimensions: Cartesian3) -> Self {
        let mut node = Self {
            level,
            coords,
            dimensions,
            oriented_bounding_box: OrientedBoundingBox::default(),
            approximate_voxel_size: 0.0,
            screen_space_error: 0.0,
            visited_frame_number: -1,
            keyframes: vec![],
            keyframe_nodes: vec![],
            renderable_keyframes: vec![],
            renderable_megatexture_indices: vec![],
            previous: None,
            next: None,
            previous_megatexture_index: -1,
            next_megatexture_index: -1,
            previous_parent_megatexture_index: -1,
            next_parent_megatexture_index: -1,
            lerp: 0.0,
            children: None,
            parent_level: None,
            parent_screen_space_error: 0.0,
        };
        node.compute_bounding_volumes(shape);
        node
    }

    pub fn compute_bounding_volumes(&mut self, shape: &dyn VoxelShape) {
        let obb = shape.tile_obb(self.level, self.coords);
        let scale = obb.half_axes.get_scale();
        self.oriented_bounding_box = obb;
        self.approximate_voxel_size =
            2.0 * scale.x.max(scale.y).max(scale.z) / self.dimensions.x.min(self.dimensions.y).min(self.dimensions.z);
    }

    pub fn construct_child_nodes(&self, shape: &dyn VoxelShape) -> Vec<Self> {
        (0..8)
            .map(|i| {
                Self::new(
                    self.level + 1,
                    [
                        self.coords[0] * 2 + (i & 1),
                        self.coords[1] * 2 + ((i >> 1) & 1),
                        self.coords[2] * 2 + ((i >> 2) & 1),
                    ],
                    shape,
                    self.dimensions,
                )
            })
            .collect()
    }

    pub fn compute_screen_space_error(&mut self, camera: Cartesian3, multiplier: f64) {
        let distance = self.oriented_bounding_box.distance_squared_to(camera).sqrt().max(1e-7);
        self.screen_space_error = multiplier * (self.approximate_voxel_size / distance);
    }

    pub fn is_visited(&self, frame: i64) -> bool {
        self.visited_frame_number == frame
    }

    pub fn visibility(&self, culling_volume: &CullingVolume, parent_plane_mask: u32) -> u32 {
        culling_volume.compute_visibility_with_plane_mask(&self.oriented_bounding_box, parent_plane_mask)
    }

    pub fn is_renderable(&self, frame: i64) -> bool {
        self.previous
            .as_ref()
            .zip(self.next.as_ref())
            .is_some_and(|(a, b)| a.level == self.level || b.level == self.level)
            && self.is_visited(frame)
    }

    pub fn create_keyframe_node(&mut self, keyframe: f64) {
        if let Err(i) = self.keyframes.binary_search_by(|v| v.total_cmp(&keyframe)) {
            self.keyframes.insert(i, keyframe);
            self.keyframe_nodes.insert(i, KeyframeNode::new(self.level, self.coords, keyframe));
        }
    }

    /// Uploads a keyframe node's metadata to `megatextures` and registers it as renderable.
    pub fn add_keyframe_node_to_megatextures<M: MegatextureSlots>(
        &mut self,
        keyframe: f64,
        megatextures: &mut [M],
    ) -> Result<isize, RuntimeError> {
        let idx = self
            .keyframe_nodes
            .binary_search_by(|n| n.keyframe.total_cmp(&keyframe))
            .map_err(|_| RuntimeError::new("Keyframe node does not exist."))?;
        let node = &mut self.keyframe_nodes[idx];
        let metadata = node
            .content
            .as_ref()
            .map(VoxelContent::metadata)
            .ok_or_else(|| RuntimeError::new("Keyframe node cannot be added to megatexture"))?;
        if node.megatexture_index != -1 || metadata.len() != megatextures.len() {
            return Err(RuntimeError::new("Keyframe node cannot be added to megatexture"));
        }
        let mut slot = -1;
        for (mt, data) in megatextures.iter_mut().zip(metadata.iter()) {
            slot = mt.add(data);
        }
        node.megatexture_index = slot;
        match self.renderable_keyframes.binary_search_by(|f| f.keyframe.total_cmp(&keyframe)) {
            Ok(_) => return Err(RuntimeError::new("Keyframe already renderable")),
            Err(pos) => {
                self.renderable_keyframes.insert(
                    pos,
                    RenderableKeyframe { level: self.level, coords: self.coords, keyframe },
                );
                self.renderable_megatexture_indices.insert(pos, slot);
            }
        }
        Ok(slot)
    }

    /// Removes a keyframe node from `megatextures`, unloads its content, and removes it from this node.
    pub fn destroy_keyframe_node<M: MegatextureSlots>(
        &mut self,
        keyframe: f64,
        megatextures: &mut [M],
    ) -> Result<(), RuntimeError> {
        let idx = self
            .keyframe_nodes
            .binary_search_by(|n| n.keyframe.total_cmp(&keyframe))
            .map_err(|_| RuntimeError::new("Keyframe node does not exist."))?;
        let mut node = self.keyframe_nodes.remove(idx);
        if let Ok(kidx) = self.keyframes.binary_search_by(|v| v.total_cmp(&keyframe)) {
            self.keyframes.remove(kidx);
        }
        if node.megatexture_index != -1 {
            for mt in megatextures.iter_mut() {
                mt.remove(node.megatexture_index);
            }
            let ridx = self
                .renderable_keyframes
                .binary_search_by(|f| f.keyframe.total_cmp(&keyframe))
                .map_err(|_| RuntimeError::new("Renderable keyframe node does not exist."))?;
            self.renderable_keyframes.remove(ridx);
            if ridx < self.renderable_megatexture_indices.len() {
                self.renderable_megatexture_indices.remove(ridx);
            }
        }
        node.unload();
        Ok(())
    }

    pub fn compute_surrounding_renderable_keyframe_nodes(&mut self, location: f64, ancestors: &[&SpatialNode]) {
        let prev = location.floor();
        let next = location.ceil();
        let mut best_prev = None;
        let mut best_next = None;
        let mut best_prev_index = -1;
        let mut best_next_index = -1;
        let mut best_prev_parent_index = -1;
        let mut best_next_parent_index = -1;
        let mut min_prev = f64::MAX;
        let mut min_next = f64::MAX;
        let nodes: Vec<&SpatialNode> = std::iter::once(&*self).chain(ancestors.iter().copied()).collect();
        for (node_index, node) in nodes.iter().enumerate() {
            let frames = &node.renderable_keyframes;
            if frames.is_empty() {
                continue;
            }
            let i = frames.partition_point(|f| f.keyframe <= prev).saturating_sub(1);
            let a = &frames[i];
            let j = if next == prev || prev < a.keyframe { i } else { (i + 1).min(frames.len() - 1) };
            let b = &frames[j];
            let da = prev - a.keyframe;
            let db = b.keyframe - next;
            let level = (self.level - node.level) as f64;
            let weight = |d: f64| level * (level * 4.0).exp() + d * if d >= 0.0 { 1.0 } else { -200.0 };
            if weight(da) < min_prev {
                min_prev = weight(da);
                best_prev = Some(a.clone());
                best_prev_index = node.renderable_megatexture_indices.get(i).copied().unwrap_or(-1);
                best_prev_parent_index = nodes
                    .get(node_index + 1)
                    .map_or(-1, |parent| parent.previous_megatexture_index);
            }
            if weight(db) < min_next {
                min_next = weight(db);
                best_next = Some(b.clone());
                best_next_index = node.renderable_megatexture_indices.get(j).copied().unwrap_or(-1);
                best_next_parent_index =
                    nodes.get(node_index + 1).map_or(-1, |parent| parent.next_megatexture_index);
            }
            if da == 0.0 && db == 0.0 {
                break;
            }
        }
        self.previous = best_prev;
        self.next = best_next;
        self.previous_megatexture_index = best_prev_index;
        self.next_megatexture_index = best_next_index;
        self.previous_parent_megatexture_index = best_prev_parent_index;
        self.next_parent_megatexture_index = best_next_parent_index;
        if let Some((a, b)) = self.previous.as_ref().zip(self.next.as_ref()) {
            self.lerp = if a.keyframe == b.keyframe {
                0.0
            } else {
                ((location - a.keyframe) / (b.keyframe - a.keyframe)).clamp(0.0, 1.0)
            };
        }
    }
}
