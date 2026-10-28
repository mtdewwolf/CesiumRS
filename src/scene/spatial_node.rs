//! CesiumJS `SpatialNode` CPU octree bounds, screen error and temporal selection.
//! Megatexture allocation and keyframe loading are not implemented. Ancestor
//! snapshots are supplied nearest-first, avoiding cyclic ownership of nodes.
use super::voxel_box_shape::VoxelBoxShape;
use crate::core::{Cartesian3, OrientedBoundingBox};
#[derive(Clone, Debug, PartialEq)]
pub struct RenderableKeyframe {
    pub level: u32,
    pub coords: [u32; 3],
    pub keyframe: f64,
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
    pub renderable_keyframes: Vec<RenderableKeyframe>,
    pub previous: Option<RenderableKeyframe>,
    pub next: Option<RenderableKeyframe>,
    pub lerp: f64,
}
impl SpatialNode {
    pub fn new(level: u32, coords: [u32; 3], shape: &VoxelBoxShape, dimensions: Cartesian3) -> Self {
        let obb = shape.tile_obb(level, coords);
        let scale = obb.half_axes.get_scale();
        Self {
            level,
            coords,
            dimensions,
            oriented_bounding_box: obb,
            approximate_voxel_size: 2.0 * scale.x.max(scale.y).max(scale.z)
                / dimensions.x.min(dimensions.y).min(dimensions.z),
            screen_space_error: 0.0,
            visited_frame_number: -1,
            keyframes: vec![],
            renderable_keyframes: vec![],
            previous: None,
            next: None,
            lerp: 0.0,
        }
    }
    pub fn construct_child_nodes(&self, shape: &VoxelBoxShape) -> Vec<Self> {
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
        }
    }
    pub fn compute_surrounding_renderable_keyframe_nodes(&mut self, location: f64, ancestors: &[&SpatialNode]) {
        let prev = location.floor();
        let next = location.ceil();
        let mut best_prev = None;
        let mut best_next = None;
        let mut min_prev = f64::MAX;
        let mut min_next = f64::MAX;
        for node in std::iter::once(&*self).chain(ancestors.iter().copied()) {
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
            }
            if weight(db) < min_next {
                min_next = weight(db);
                best_next = Some(b.clone());
            }
            if da == 0.0 && db == 0.0 {
                break;
            }
        }
        self.previous = best_prev;
        self.next = best_next;
        if let Some((a, b)) = self.previous.as_ref().zip(self.next.as_ref()) {
            self.lerp = if a.keyframe == b.keyframe {
                0.0
            } else {
                ((location - a.keyframe) / (b.keyframe - a.keyframe)).clamp(0.0, 1.0)
            };
        }
    }
}
