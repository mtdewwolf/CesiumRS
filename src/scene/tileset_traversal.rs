//! 3D Tiles volume, visibility, request priority and traversal algorithms.
use super::{
    cesium_3d_tileset::{array, Cesium3DTileset, TileId, TilesetTraversalKind},
    *,
};
use crate::{core::*, renderer::UniformFrustum};
use serde_json::Value;
use std::rc::Rc;

#[derive(Clone, Debug, PartialEq)]
pub enum TilesetVolume {
    Sphere(TileBoundingSphere),
    Box(TileOrientedBoundingBox),
    Region(Rc<TileBoundingRegion>),
    S2(Rc<TileBoundingS2Cell>),
}
impl TilesetVolume {
    pub(super) fn from_header(
        v: &Value,
        transform: Matrix4,
        initial: Matrix4,
        exaggeration: f64,
        relative: f64,
    ) -> Result<Self, RuntimeError> {
        if let Some(s) = v.pointer("/extensions/3DTILES_bounding_volume_S2") {
            return TileBoundingS2Cell::new(tile_bounding_s2_cell::TileBoundingS2CellOptions {
                token: s["token"].as_str().ok_or_else(|| RuntimeError::new("S2 token is required"))?.into(),
                minimum_height: s["minimumHeight"].as_f64(),
                maximum_height: s["maximumHeight"].as_f64(),
                ellipsoid: None,
            })
            .map(|v| Self::S2(Rc::new(v)))
            .ok_or_else(|| RuntimeError::new("invalid S2 bounding volume"));
        }
        let mut volume = if let Some(b) = v.get("box") {
            let a = array::<12>(b)?;
            Self::Box(TileOrientedBoundingBox::new(
                transform.multiply_by_point(Cartesian3::new(a[0], a[1], a[2])),
                transform.get_matrix3().multiply(&Matrix3::from_array(&a[3..], 0)),
            ))
        } else if let Some(r) = v.get("region") {
            let [w, s, e, n, min, max] = array::<6>(r)?;
            let rectangle = Rectangle::new(w, s, e, n);
            if !transform.equals_epsilon(&initial, 1e-8) {
                let obb = OrientedBoundingBox::from_rectangle(&rectangle, min, max, &Ellipsoid::WGS84)
                    .ok_or_else(|| RuntimeError::new("invalid tile region"))?;
                let delta = transform.multiply_transformation(&initial.inverse_transformation());
                Self::Box(TileOrientedBoundingBox::new(
                    delta.multiply_by_point(obb.center),
                    delta.get_matrix3().multiply(&obb.half_axes),
                ))
            } else {
                let mut options = tile_bounding_region::TileBoundingRegionOptions::new(rectangle);
                options.minimum_height = Some(vertical_exaggeration::get_height(min, exaggeration, relative));
                options.maximum_height = Some(vertical_exaggeration::get_height(max, exaggeration, relative));
                return Ok(Self::Region(Rc::new(TileBoundingRegion::new(options))));
            }
        } else if let Some(s) = v.get("sphere") {
            let [x, y, z, r] = array::<4>(s)?;
            Self::Sphere(TileBoundingSphere::new(
                transform.multiply_by_point(Cartesian3::new(x, y, z)),
                r * transform.get_maximum_scale(),
            ))
        } else {
            return Err(RuntimeError::new("boundingVolume must contain a sphere, region, or box"));
        };
        if exaggeration != 1. {
            match &mut volume {
                Self::Sphere(s) => s.update(
                    vertical_exaggeration::get_position(s.center(), &Ellipsoid::WGS84, exaggeration, relative),
                    s.radius() * exaggeration,
                ),
                Self::Box(b) => {
                    let points = b
                        .bounding_volume()
                        .compute_corners()
                        .map(|p| vertical_exaggeration::get_position(p, &Ellipsoid::WGS84, exaggeration, relative));
                    let obb = OrientedBoundingBox::from_points(&points);
                    b.update(obb.center, obb.half_axes);
                }
                _ => {}
            }
        }
        Ok(volume)
    }
    pub fn oriented_box(&self) -> Option<&OrientedBoundingBox> {
        match self {
            Self::Box(b) => Some(b.bounding_volume()),
            Self::Region(r) => r.bounding_volume(),
            _ => None,
        }
    }
    pub fn sphere(&self) -> &BoundingSphere {
        self.as_volume().bounding_sphere()
    }
    pub fn as_volume(&self) -> &dyn TileBoundingVolume {
        match self {
            Self::Sphere(v) => v,
            Self::Box(v) => v,
            Self::Region(v) => v.as_ref(),
            Self::S2(v) => v.as_ref(),
        }
    }
    pub fn for_frame(&self, frame: &FrameState) -> Self {
        if frame.mode == SceneMode::Scene3D {
            return self.clone();
        }
        let s = self.sphere().project_to_2d(Some(frame.uniform.map_projection.as_ref()));
        Self::Sphere(TileBoundingSphere::new(s.center, s.radius))
    }
    pub fn distance(&self, frame: &FrameState) -> f64 {
        let f = tile_bounding_volume::TileBoundingVolumeFrame {
            mode: frame.mode,
            camera_position_wc: frame.uniform.camera.position,
            camera_position_cartographic: frame.uniform.camera.cartographic.unwrap_or(Cartographic::new(
                f64::NAN,
                f64::NAN,
                f64::NAN,
            )),
            map_projection: frame.uniform.map_projection.as_ref(),
        };
        self.as_volume().distance_to_camera(&f)
    }
}
impl crate::core::culling_volume::BoundingVolume for TilesetVolume {
    fn intersect_plane(&self, plane: &Plane) -> Intersect {
        self.as_volume().intersect_plane(plane)
    }
}

#[derive(Clone, Debug)]
pub struct TileTraversalState {
    pub visible: bool,
    pub in_request_volume: bool,
    pub visibility_plane_mask: u32,
    pub screen_space_error: f64,
    pub progressive_error: f64,
    pub center_z_depth: f64,
    pub foveated_factor: f64,
    pub priority_deferred: bool,
    pub progressive_priority: bool,
    pub progressive_leaf: bool,
    pub reverse_error: f64,
    pub priority_holder: TileId,
    pub was_min_priority_child: bool,
    pub updated_visibility: u64,
    pub visited_frame: u32,
    pub selected_frame: u32,
    pub requested_frame: Option<u32>,
    pub was_selected_last_frame: bool,
    pub refines: bool,
    pub should_select: bool,
    pub final_resolution: bool,
    pub selection_depth: usize,
    pub stack_length: usize,
    pub ancestor_with_content: Option<TileId>,
    pub ancestor_with_content_available: Option<TileId>,
    pub children_within_parent: bool,
    pub is_clipped: bool,
    pub is_clipped_by_polygon: bool,
}
impl TileTraversalState {
    pub fn new(id: TileId) -> Self {
        Self {
            visible: false,
            in_request_volume: false,
            visibility_plane_mask: 0,
            screen_space_error: 0.,
            progressive_error: 0.,
            center_z_depth: 0.,
            foveated_factor: 0.,
            priority_deferred: false,
            progressive_priority: false,
            progressive_leaf: false,
            reverse_error: 0.,
            priority_holder: id,
            was_min_priority_child: false,
            updated_visibility: 0,
            visited_frame: 0,
            selected_frame: 0,
            requested_frame: None,
            was_selected_last_frame: false,
            refines: false,
            should_select: false,
            final_resolution: true,
            selection_depth: 0,
            stack_length: 0,
            ancestor_with_content: None,
            ancestor_with_content_available: None,
            children_within_parent: false,
            is_clipped: true,
            is_clipped_by_polygon: false,
        }
    }
}
#[derive(Clone, Copy, Debug)]
pub(super) struct TilePriority {
    pub distance: f64,
    pub depth: f64,
    pub foveated_factor: f64,
    pub reverse_error: f64,
}
impl TilePriority {
    pub fn minimum() -> Self {
        Self { distance: f64::MAX, depth: f64::MAX, foveated_factor: f64::MAX, reverse_error: f64::MAX }
    }
    pub fn maximum() -> Self {
        Self { distance: -f64::MAX, depth: -f64::MAX, foveated_factor: -f64::MAX, reverse_error: -f64::MAX }
    }
}

impl Cesium3DTileset {
    fn content_available(&self, id: TileId) -> bool {
        let t = &self.tiles[id];
        t.content_ready() && self.renderable(id)
            || t.expired_content.is_some() && t.state != Cesium3DTileContentState::Failed
    }
    fn renderable(&self, id: TileId) -> bool {
        self.tiles[id].has_renderable_content
    }
    fn can_traverse(&self, id: TileId, most: bool) -> bool {
        let t = &self.tiles[id];
        !t.children.is_empty()
            && (most
                || ((t.has_external_content || t.has_implicit_content) && t.state != Cesium3DTileContentState::Expired)
                || t.traversal.screen_space_error > self.memory_adjusted_screen_space_error)
    }
    pub(super) fn compute_children_hint(&mut self, id: TileId) {
        let Some(parent) = self.tiles[id].volume.oriented_box() else { return };
        let mut within = true;
        for &child in &self.tiles[id].children {
            let Some(child) = self.tiles[child].volume.oriented_box() else {
                within = false;
                break;
            };
            let delta = child.center.subtract(parent.center);
            let length = delta.magnitude();
            let axis = delta.divide_by_scalar(length);
            let projection = |b: &OrientedBoundingBox| {
                b.half_axes
                    .0
                    .iter()
                    .enumerate()
                    .map(|(i, value)| (*value * [axis.x, axis.y, axis.z][i % 3]).abs())
                    .sum::<f64>()
            };
            if projection(parent) <= projection(child) + length {
                within = false;
                break;
            }
        }
        self.tiles[id].traversal.children_within_parent = within;
    }
    pub(super) fn tile_error(&self, id: TileId, frame: &FrameState, parent_error: bool, height_fraction: f64) -> f64 {
        let t = &self.tiles[id];
        let error = if parent_error {
            t.parent.map_or(self.geometric_error * t.computed_transform.get_maximum_scale(), |p| {
                self.tiles[p].geometric_error * self.tiles[p].computed_transform.get_maximum_scale()
            })
        } else {
            t.geometric_error * t.computed_transform.get_maximum_scale()
        };
        if error == 0. {
            return 0.;
        }
        let height = frame.drawing_buffer_height as f64 * height_fraction;
        let error = if frame.mode == SceneMode::Scene2D
            || matches!(
                frame.uniform.camera.frustum,
                UniformFrustum::Orthographic(_) | UniformFrustum::OrthographicOffCenter(_)
            ) {
            let (_, _, _, p) = frame.uniform.camera.frustum.parameters();
            error / ((p[0] - p[1]).max(p[3] - p[2]) / (frame.drawing_buffer_width as f64).max(height))
        } else {
            let denominator = match frame.uniform.camera.frustum {
                UniformFrustum::Perspective(f) => f.sse_denominator(),
                _ => f64::NAN,
            };
            let distance = t.distance_to_camera.max(1e-7);
            error * height / (distance * denominator)
                - if self.options.dynamic_screen_space_error {
                    math::fog(distance, self.dynamic_screen_space_error_computed_density)
                        * self.options.dynamic_screen_space_error_factor
                } else {
                    0.
                }
        };
        error / frame.uniform.pixel_ratio
    }
    fn update_visibility(&mut self, id: TileId, frame: &FrameState) -> Result<(), RuntimeError> {
        if self.tiles[id].traversal.updated_visibility == self.visibility_frame {
            return Ok(());
        }
        let volume = self.tiles[id].volume.for_frame(frame);
        self.tiles[id].distance_to_camera = volume.distance(frame);
        let z = frame.uniform.camera.direction.dot(volume.sphere().center.subtract(frame.uniform.camera.position));
        let error = self.tile_error(id, frame, false, 1.);
        let progressive = self.tile_error(id, frame, false, self.options.progressive_resolution_height_fraction);
        let parent = self.tiles[id].parent;
        let mask = parent.map_or(CullingVolume::MASK_INDETERMINATE, |p| self.tiles[p].traversal.visibility_plane_mask);
        let clip = self.clip_volume(&volume)?;
        if self.clipping_planes.as_ref().is_some_and(|p| p.borrow().enabled) {
            self.tiles[id].traversal.is_clipped = clip != Intersect::Inside;
        }
        let visibility = if clip == Intersect::Outside {
            CullingVolume::MASK_OUTSIDE
        } else {
            frame.culling_volume.compute_visibility_with_plane_mask(&volume, mask)
        };
        let in_request_volume = self.tiles[id].viewer_request_volume.as_ref().is_none_or(|v| v.distance(frame) == 0.);
        let root_error =
            self.root.map_or(0., |r| if r == id { error } else { self.tiles[r].traversal.screen_space_error });
        let use_parent = parent.is_some_and(|p| {
            !self.is_skipping_level_of_detail()
                || error == 0.
                || self.tiles[p].has_external_content
                || self.tiles[p].has_implicit_content
        });
        let reverse_error =
            root_error - if use_parent { self.tiles[parent.unwrap()].traversal.screen_space_error } else { error };
        let mut progressive_priority = false;
        let mut progressive_leaf = false;
        if self.options.progressive_resolution_height_fraction > 0.
            && self.options.progressive_resolution_height_fraction <= 0.5
        {
            progressive_leaf = progressive <= self.memory_adjusted_screen_space_error
                && parent.is_some_and(|p| {
                    self.tiles[p].traversal.progressive_error > self.memory_adjusted_screen_space_error
                });
            progressive_priority = progressive > self.memory_adjusted_screen_space_error || progressive_leaf;
        }
        let camera = &frame.uniform.camera;
        let sphere = self.tiles[id].bounding_sphere;
        let to_line = camera.position.add(camera.direction.multiply_by_scalar(z)).subtract(sphere.center);
        let foveated = if to_line.magnitude() > sphere.radius {
            let closest = sphere
                .center
                .add(to_line.normalize().multiply_by_scalar(sphere.radius))
                .subtract(camera.position)
                .normalize();
            1. - camera.direction.dot(closest).abs()
        } else {
            0.
        };
        let replace = self.tiles[id].refine == Cesium3DTileRefine::Replace;
        let skip = self.is_skipping_level_of_detail();
        let defer = if (replace && !skip)
            || !self.options.foveated_screen_space_error
            || self.options.foveated_cone_size == 1.
            || (progressive_priority && replace && skip)
            || matches!(self.current_pass, Cesium3DTilePass::Preload | Cesium3DTilePass::PreloadFlight)
        {
            false
        } else {
            let max = 1. - (camera.frustum.fov().unwrap_or(f64::NAN) * 0.5).cos();
            let cone = self.options.foveated_cone_size * max;
            if foveated <= cone {
                false
            } else {
                let normalized = ((foveated - cone) / (max - cone)).clamp(0., 1.);
                let relaxation = (self.foveated_interpolation)(
                    self.options.foveated_minimum_screen_space_error_relaxation,
                    self.memory_adjusted_screen_space_error,
                    normalized,
                );
                let error = if error == 0. {
                    parent.map_or(error, |p| self.tiles[p].traversal.screen_space_error * 0.5)
                } else {
                    error
                };
                self.memory_adjusted_screen_space_error - relaxation <= error
            }
        };
        let s = &mut self.tiles[id].traversal;
        s.center_z_depth = z;
        s.screen_space_error = error;
        s.progressive_error = progressive;
        s.visibility_plane_mask = visibility;
        s.visible = visibility != CullingVolume::MASK_OUTSIDE;
        s.in_request_volume = in_request_volume;
        s.reverse_error = reverse_error;
        s.progressive_priority = progressive_priority;
        s.progressive_leaf = progressive_leaf;
        s.foveated_factor = foveated;
        s.priority_deferred = defer;
        s.updated_visibility = self.visibility_frame;
        Ok(())
    }
    fn update_tile(&mut self, id: TileId, frame: &FrameState) -> Result<(), RuntimeError> {
        self.update_visibility(id, frame)?;
        let children = self.tiles[id].children.clone();
        let parent = self.tiles[id].parent;
        if self.tiles[id].traversal.visible {
            if (self.tiles[id].has_external_content || self.tiles[id].has_implicit_content) && !children.is_empty() {
                self.update_tile(children[0], frame)?;
                self.tiles[id].traversal.visible = self.tiles[children[0]].traversal.visible;
            } else if parent.is_some_and(|p| {
                !self.tiles[p].has_external_content
                    && !self.tiles[p].has_implicit_content
                    && self.tiles[p].refine == Cesium3DTileRefine::Add
            }) && self.tile_error(id, frame, true, 1.) <= self.memory_adjusted_screen_space_error
            {
                self.tiles[id].traversal.visible = false;
            } else if self.tiles[id].refine == Cesium3DTileRefine::Replace
                && self.tiles[id].traversal.children_within_parent
                && !children.is_empty()
            {
                let mut any = false;
                for child in children {
                    self.update_visibility(child, frame)?;
                    any |= self.tiles[child].traversal.visible;
                }
                if !any {
                    self.statistics.number_of_tiles_culled_with_children_union += 1;
                    self.tiles[id].traversal.visible = false;
                }
            }
        }
        self.update_expiration(id, frame.time)?;
        let s = &mut self.tiles[id].traversal;
        s.was_min_priority_child = false;
        s.priority_holder = id;
        s.should_select = false;
        s.final_resolution = true;
        let t = &self.tiles[id];
        let p = TilePriority {
            distance: t.distance_to_camera,
            depth: t.depth as f64,
            foveated_factor: t.traversal.foveated_factor,
            reverse_error: t.traversal.reverse_error,
        };
        self.minimum_priority.distance = self.minimum_priority.distance.min(p.distance);
        self.maximum_priority.distance = self.maximum_priority.distance.max(p.distance);
        self.minimum_priority.depth = self.minimum_priority.depth.min(p.depth);
        self.maximum_priority.depth = self.maximum_priority.depth.max(p.depth);
        self.minimum_priority.foveated_factor = self.minimum_priority.foveated_factor.min(p.foveated_factor);
        self.maximum_priority.foveated_factor = self.maximum_priority.foveated_factor.max(p.foveated_factor);
        self.minimum_priority.reverse_error = self.minimum_priority.reverse_error.min(p.reverse_error);
        self.maximum_priority.reverse_error = self.maximum_priority.reverse_error.max(p.reverse_error);
        Ok(())
    }
    fn touch_tile(&mut self, id: TileId, frame: &FrameState) {
        if self.tiles[id].touched_frame != frame.frame_number {
            self.cache.touch(id);
            self.tiles[id].touched_frame = frame.frame_number;
        }
    }
    fn visit_tile(&mut self, id: TileId, frame: &FrameState) {
        self.statistics.visited += 1;
        self.tiles[id].traversal.visited_frame = frame.frame_number;
    }
    fn load_tile(&mut self, id: TileId, frame: &FrameState, most: bool) {
        let t = &self.tiles[id];
        if t.has_empty_content()
            || !matches!(t.state, Cesium3DTileContentState::Unloaded | Cesium3DTileContentState::Expired)
        {
            return;
        }
        if !most {
            if t.traversal.requested_frame == Some(frame.frame_number) {
                return;
            }
            if self.cull_requests_while_moving {
                let delta = if frame.camera_position_delta != 0. {
                    frame.camera_position_delta
                } else {
                    frame.camera_position_delta_last_frame
                };
                if self.options.cull_requests_while_moving_multiplier * delta / (t.bounding_sphere.radius * 2.).max(1.)
                    >= 1.
                {
                    return;
                }
            }
            if t.traversal.priority_deferred && frame.camera_time_since_moved < self.options.foveated_time_delay {
                return;
            }
            self.tiles[id].traversal.requested_frame = Some(frame.frame_number);
        } else {
            self.tiles[id].priority = 0.;
        }
        self.requested_tiles.push(id);
    }
    fn content_visibility(&mut self, id: TileId, frame: &FrameState) -> Result<Intersect, RuntimeError> {
        if self.tiles[id].traversal.visibility_plane_mask == CullingVolume::MASK_INSIDE {
            return Ok(Intersect::Inside);
        }
        let Some(volume) = self.tiles[id].content_volume.as_ref().map(|v| v.for_frame(frame)) else {
            return Ok(Intersect::Inside);
        };
        let clip = self.clip_volume(&volume)?;
        if self.clipping_planes.as_ref().is_some_and(|p| p.borrow().enabled) {
            self.tiles[id].traversal.is_clipped = clip != Intersect::Inside;
        }
        if clip == Intersect::Outside {
            return Ok(clip);
        }
        Ok(frame.culling_volume.compute_visibility(&volume))
    }
    fn select_tile(&mut self, id: TileId, frame: &FrameState) -> Result<(), RuntimeError> {
        if self.content_visibility(id, frame)? == Intersect::Outside {
            return Ok(());
        }
        let t = &mut self.tiles[id];
        t.traversal.was_selected_last_frame = true;
        if t.traversal.selected_frame < frame.frame_number.saturating_sub(1) {
            t.traversal.was_selected_last_frame = false;
            self.selected_tiles_to_style.push(id);
        }
        t.traversal.selected_frame = frame.frame_number;
        self.selected_tiles.push(id);
        Ok(())
    }
    fn update_children(
        &mut self,
        id: TileId,
        frame: &FrameState,
        stack: &mut Vec<TileId>,
        base: bool,
    ) -> Result<bool, RuntimeError> {
        let mut children = self.tiles[id].children.clone();
        for &child in &children {
            self.update_tile(child, frame)?;
        }
        children.sort_by(|&a, &b| {
            let a = &self.tiles[a];
            let b = &self.tiles[b];
            if a.distance_to_camera == 0. && b.distance_to_camera == 0. {
                b.traversal.center_z_depth.total_cmp(&a.traversal.center_z_depth)
            } else {
                b.distance_to_camera.total_cmp(&a.distance_to_camera)
            }
        });
        self.tiles[id].children = children.clone();
        let check = base && self.tiles[id].refine == Cesium3DTileRefine::Replace && self.renderable(id);
        let mut any = false;
        let mut refines = true;
        let mut min = None;
        let mut min_factor = f64::MAX;
        for child in children.clone() {
            if self.tiles[child].traversal.visible {
                stack.push(child);
                any = true;
                if self.tiles[child].traversal.foveated_factor < min_factor {
                    min = Some(child);
                    min_factor = self.tiles[child].traversal.foveated_factor;
                }
            } else if check || self.options.load_siblings {
                self.load_tile(child, frame, false);
                self.touch_tile(child, frame);
                if self.tiles[child].traversal.foveated_factor < min_factor {
                    min = Some(child);
                    min_factor = self.tiles[child].traversal.foveated_factor;
                }
            }
            if check {
                let child_refines = if !self.tiles[child].traversal.in_request_volume {
                    false
                } else if !self.renderable(child) {
                    self.empty_traversal(child, frame)?
                } else {
                    self.content_available(child)
                };
                refines &= child_refines;
            }
        }
        if let Some(min) = min.filter(|_| base && self.tiles[id].refine == Cesium3DTileRefine::Replace) {
            self.tiles[min].traversal.was_min_priority_child = true;
            let holder = if (self.tiles[id].traversal.was_min_priority_child || self.root == Some(id))
                && min_factor <= self.tiles[self.tiles[id].traversal.priority_holder].traversal.foveated_factor
            {
                self.tiles[id].traversal.priority_holder
            } else {
                id
            };
            self.tiles[holder].traversal.foveated_factor =
                self.tiles[holder].traversal.foveated_factor.min(self.tiles[min].traversal.foveated_factor);
            self.tiles[holder].distance_to_camera =
                self.tiles[holder].distance_to_camera.min(self.tiles[min].distance_to_camera);
            for child in children {
                self.tiles[child].traversal.priority_holder = holder;
            }
        }
        Ok(any && (!check || refines))
    }
    fn empty_traversal(&mut self, root: TileId, frame: &FrameState) -> Result<bool, RuntimeError> {
        let mut stack = vec![root];
        let mut loaded = true;
        while let Some(id) = stack.pop() {
            let traverse = !self.renderable(id) && self.can_traverse(id, false);
            if !traverse && !self.content_available(id) {
                loaded = false;
            }
            self.update_tile(id, frame)?;
            if !self.tiles[id].traversal.visible {
                self.load_tile(id, frame, false);
                self.touch_tile(id, frame);
            }
            if traverse {
                stack.extend(self.tiles[id].children.iter().copied());
            }
        }
        Ok(self.tiles[root].has_empty_content() || loaded)
    }
    fn select_desired_skip(&mut self, id: TileId, frame: &FrameState) -> Result<(), RuntimeError> {
        let loaded = if self.content_available(id) {
            Some(id)
        } else {
            self.tiles[id].traversal.ancestor_with_content_available
        };
        if let Some(loaded) = loaded {
            self.tiles[loaded].traversal.should_select = true;
        } else {
            let depth = self.tiles[id].depth;
            let mut stack = vec![id];
            while let Some(id) = stack.pop() {
                for child in self.tiles[id].children.clone() {
                    if self.tiles[child].traversal.visible {
                        if self.content_available(child) {
                            self.update_tile(child, frame)?;
                            self.touch_tile(child, frame);
                            self.select_tile(child, frame)?;
                        } else if (self.tiles[child].depth as i64 - depth as i64) < 2 {
                            stack.push(child);
                        }
                    }
                }
            }
        }
        Ok(())
    }
    pub(super) fn select_tiles(
        &mut self,
        frame: &FrameState,
        kind: TilesetTraversalKind,
    ) -> Result<bool, RuntimeError> {
        self.requested_tiles.clear();
        if self.options.debug_freeze_frame && kind != TilesetTraversalKind::MostDetailed {
            return Ok(true);
        }
        self.selected_tiles.clear();
        self.selected_tiles_to_style.clear();
        self.empty_tiles.clear();
        self.has_mixed_content = false;
        let Some(root) = self.root else { return Ok(false) };
        if kind == TilesetTraversalKind::MostDetailed {
            self.update_visibility(root, frame)?;
        } else {
            self.update_tile(root, frame)?;
        }
        if !self.tiles[root].traversal.visible {
            return Ok(true);
        }
        if kind != TilesetTraversalKind::MostDetailed
            && self.tile_error(root, frame, true, 1.) <= self.memory_adjusted_screen_space_error
        {
            return Ok(true);
        }
        let mut stack = vec![root];
        let mut ready = true;
        while let Some(id) = stack.pop() {
            if kind == TilesetTraversalKind::MostDetailed {
                let traverse = self.can_traverse(id, true);
                if traverse {
                    for child in self.tiles[id].children.clone() {
                        self.update_visibility(child, frame)?;
                        if self.tiles[child].traversal.visible {
                            stack.push(child);
                        }
                    }
                }
                if self.tiles[id].refine == Cesium3DTileRefine::Add || !traverse {
                    self.load_tile(id, frame, true);
                    self.touch_tile(id, frame);
                    if self.content_available(id) && self.content_visibility(id, frame)? != Intersect::Outside {
                        self.selected_tiles.push(id);
                    }
                    if self.renderable(id) && !self.content_available(id) {
                        ready = false;
                    }
                }
                self.visit_tile(id, frame);
                continue;
            }
            let skip = kind == TilesetTraversalKind::Skip;
            let parent = self.tiles[id].parent;
            if skip {
                let (potential, available) = parent.map_or((None, None), |p| {
                    let t = &self.tiles[p];
                    (
                        if t.state != Cesium3DTileContentState::Unloaded
                            || t.traversal.requested_frame == Some(frame.frame_number)
                        {
                            Some(p)
                        } else {
                            t.traversal.ancestor_with_content
                        },
                        if self.content_available(p) { Some(p) } else { t.traversal.ancestor_with_content_available },
                    )
                });
                self.tiles[id].traversal.ancestor_with_content = potential;
                self.tiles[id].traversal.ancestor_with_content_available = available;
            }
            let parent_refines = parent.is_none_or(|p| self.tiles[p].traversal.refines);
            let traverse = self.can_traverse(id, false);
            let refines =
                if traverse { self.update_children(id, frame, &mut stack, !skip)? && parent_refines } else { false };
            self.tiles[id].traversal.refines = refines;
            let stopped = !refines && parent_refines;
            if !self.renderable(id) {
                self.empty_tiles.push(id);
                self.load_tile(id, frame, false);
                if stopped {
                    if skip {
                        self.select_desired_skip(id, frame)?;
                    } else if self.content_available(id) {
                        self.select_tile(id, frame)?;
                    }
                }
            } else if self.tiles[id].refine == Cesium3DTileRefine::Add {
                if skip {
                    self.select_desired_skip(id, frame)?;
                } else if self.content_available(id) {
                    self.select_tile(id, frame)?;
                }
                self.load_tile(id, frame, false);
            } else if !skip {
                self.load_tile(id, frame, false);
                if stopped && self.content_available(id) {
                    self.select_tile(id, frame)?;
                }
            } else {
                let t = &self.tiles[id];
                let ancestor = t.traversal.ancestor_with_content;
                let base = self.options.base_screen_space_error.max(self.memory_adjusted_screen_space_error);
                let in_base = !self.options.immediately_load_desired_level_of_detail
                    && (ancestor.is_none()
                        || if t.traversal.screen_space_error == 0. {
                            parent.is_some_and(|p| self.tiles[p].traversal.screen_space_error > base)
                        } else {
                            t.traversal.screen_space_error > base
                        });
                let reached = !self.options.immediately_load_desired_level_of_detail
                    && (t.traversal.progressive_leaf
                        || ancestor.is_some_and(|a| {
                            t.traversal.screen_space_error
                                < self.tiles[a].traversal.screen_space_error
                                    / self.options.skip_screen_space_error_factor
                                && t.depth > self.tiles[a].depth + self.options.skip_levels
                        }));
                if in_base {
                    self.load_tile(id, frame, false);
                    if stopped {
                        self.select_desired_skip(id, frame)?;
                    }
                } else if stopped {
                    self.select_desired_skip(id, frame)?;
                    self.load_tile(id, frame, false);
                } else if reached {
                    self.load_tile(id, frame, false);
                }
            }
            self.visit_tile(id, frame);
            self.touch_tile(id, frame);
        }
        if kind == TilesetTraversalKind::Skip {
            self.traverse_skip_selection(root, frame)?;
        }
        if kind != TilesetTraversalKind::MostDetailed {
            for id in self.requested_tiles.clone() {
                self.update_priority(id);
            }
        }
        Ok(ready)
    }
    fn traverse_skip_selection(&mut self, root: TileId, frame: &FrameState) -> Result<(), RuntimeError> {
        let mut stack = vec![root];
        let mut ancestors: Vec<TileId> = vec![];
        let mut last = None;
        while !stack.is_empty() || !ancestors.is_empty() {
            if let Some(&waiting) = ancestors.last() {
                if self.tiles[waiting].traversal.stack_length == stack.len() {
                    ancestors.pop();
                    if Some(waiting) != last {
                        self.tiles[waiting].traversal.final_resolution = false;
                    }
                    self.select_tile(waiting, frame)?;
                    continue;
                }
            }
            let Some(id) = stack.pop() else { continue };
            let traverse = self.can_traverse(id, false);
            if self.tiles[id].traversal.should_select {
                if self.tiles[id].refine == Cesium3DTileRefine::Add {
                    self.select_tile(id, frame)?;
                } else {
                    self.tiles[id].traversal.selection_depth = ancestors.len();
                    self.has_mixed_content |= !ancestors.is_empty();
                    last = Some(id);
                    if !traverse {
                        self.select_tile(id, frame)?;
                        continue;
                    }
                    ancestors.push(id);
                    self.tiles[id].traversal.stack_length = stack.len();
                }
            }
            if traverse {
                for &child in &self.tiles[id].children {
                    if self.tiles[child].traversal.visible {
                        stack.push(child);
                    }
                }
            }
        }
        Ok(())
    }
    pub(super) fn update_priority(&mut self, id: TileId) {
        fn normalize(value: f64, min: f64, max: f64) -> f64 {
            (if max == min { 0. } else { ((value - min) / (max - min)).clamp(0., 1.) } - 1e-7).max(0.)
        }
        let t = &self.tiles[id];
        let holder = &self.tiles[t.traversal.priority_holder];
        let min = self.minimum_priority;
        let max = self.maximum_priority;
        let mut depth = normalize(t.depth as f64, min.depth, max.depth);
        if self.options.prefer_leaves {
            depth = 1. - depth;
        }
        let sorting = if !self.is_skipping_level_of_detail() && t.refine == Cesium3DTileRefine::Replace {
            normalize(holder.distance_to_camera, min.distance, max.distance)
        } else {
            normalize(t.traversal.reverse_error, min.reverse_error, max.reverse_error)
        };
        let foveated = normalize(holder.traversal.foveated_factor, min.foveated_factor, max.foveated_factor);
        self.tiles[id].priority = depth
            + (sorting * 1e4).trunc()
            + (foveated * 1e4).trunc() * 1e4
            + if t.traversal.progressive_priority { 0. } else { 1e8 }
            + if t.traversal.priority_deferred { 1e9 } else { 0. }
            + if self.current_pass == Cesium3DTilePass::PreloadFlight { 0. } else { 1e10 };
    }
}
