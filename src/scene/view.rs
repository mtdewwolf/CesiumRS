//! Cesium View's potentially-visible set and multi-frustum fitting.
use super::{Camera, CameraFrustum, FrameState, SceneCommand, SceneMode, SceneOptions};
use crate::{core::*, renderer::Pass};
use std::collections::BTreeMap;

#[derive(Clone)]
pub struct FrustumCommands {
    pub near: f64,
    pub far: f64,
    pub commands: [Vec<SceneCommand>; Pass::NUMBER_OF_PASSES as usize],
}
impl FrustumCommands {
    pub fn new(near: f64, far: f64) -> Self {
        Self { near, far, commands: std::array::from_fn(|_| Vec::new()) }
    }
}
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct DebugFrustumStatistics {
    pub total_commands: usize,
    pub commands_in_frustums: BTreeMap<u32, usize>,
}
#[derive(Default)]
pub struct SceneView {
    pub frustum_commands_list: Vec<FrustumCommands>,
    pub debug_frustum_statistics: Option<DebugFrustumStatistics>,
    pub compute_commands: Vec<SceneCommand>,
    pub overlay_commands: Vec<SceneCommand>,
    camera_clone: Option<CameraPose>,
    camera_start_fired: bool,
    camera_moved_time: Option<f64>,
    resources: Option<super::FrameResource>,
    destroy_resources: Option<Box<dyn Fn() -> Result<(), RuntimeError>>>,
}
#[derive(Clone)]
struct CameraPose {
    position: Cartesian3,
    direction: Cartesian3,
    up: Cartesian3,
    right: Cartesian3,
    transform: Matrix4,
    frustum: CameraFrustum,
}
impl CameraPose {
    fn new(camera: &Camera) -> Self {
        Self {
            position: camera.position,
            direction: camera.direction,
            up: camera.up,
            right: camera.right,
            transform: camera.transform(),
            frustum: camera.frustum,
        }
    }
    fn equals(&self, other: &Self) -> bool {
        let epsilon = 1e-15;
        let scale = 1. / self.position.abs().maximum_component().max(other.position.abs().maximum_component()).max(1.);
        let frustum = match (self.frustum, other.frustum) {
            (CameraFrustum::Perspective(a), CameraFrustum::Perspective(b)) => a.equals_epsilon(&b, epsilon, epsilon),
            (CameraFrustum::Orthographic(a), CameraFrustum::Orthographic(b)) => a.equals_epsilon(&b, epsilon, epsilon),
            (CameraFrustum::OrthographicOffCenter(a), CameraFrustum::OrthographicOffCenter(b)) => {
                a.equals_epsilon(&b, epsilon, epsilon)
            }
            _ => false,
        };
        self.position.multiply_by_scalar(scale).equals_epsilon(other.position.multiply_by_scalar(scale), epsilon)
            && self.direction.equals_epsilon(other.direction, epsilon)
            && self.up.equals_epsilon(other.up, epsilon)
            && self.right.equals_epsilon(other.right, epsilon)
            && self.transform.equals_epsilon(&other.transform, epsilon)
            && frustum
    }
}
impl SceneCommand {
    pub(crate) fn plane_distances(&self, position: Cartesian3, direction: Cartesian3) -> Option<Interval> {
        if let Some(bounds) = &self.metadata.oriented_bounding_box {
            Some(bounds.compute_plane_distances(position, direction))
        } else {
            self.metadata.bounding_volume.as_ref().map(|b| b.compute_plane_distances(position, direction))
        }
    }
    pub(crate) fn visible_in(&self, volume: &CullingVolume, occluder: Option<&Occluder>) -> bool {
        if !self.metadata.cull {
            return true;
        }
        if let Some(bounds) = &self.metadata.oriented_bounding_box {
            volume.compute_visibility(bounds) != Intersect::Outside
                && !(self.metadata.occlude && occluder.is_some_and(|o| bounds.is_occluded(o)))
        } else if let Some(bounds) = &self.metadata.bounding_volume {
            volume.compute_visibility(bounds) != Intersect::Outside
                && !(self.metadata.occlude && occluder.is_some_and(|o| !o.is_bounding_sphere_visible(bounds)))
        } else {
            true
        }
    }
    pub(crate) fn distance_squared(&self, position: Cartesian3) -> f64 {
        self.metadata.oriented_bounding_box.as_ref().map_or_else(
            || self.metadata.bounding_volume.as_ref().map_or(0., |b| b.distance_squared_to(position)),
            |b| b.distance_squared_to(position),
        )
    }
}
impl SceneView {
    pub fn set_resources<G: crate::renderer::Gl + 'static>(
        &mut self,
        resources: std::rc::Rc<std::cell::RefCell<super::ViewResources<G>>>,
    ) -> Result<(), RuntimeError> {
        self.destroy()?;
        self.resources = Some(super::FrameResource::new(resources.clone()));
        self.destroy_resources = Some(Box::new(move || resources.borrow_mut().destroy()));
        Ok(())
    }
    pub fn resources<G: crate::renderer::Gl + 'static>(
        &self,
    ) -> Option<std::rc::Rc<std::cell::RefCell<super::ViewResources<G>>>> {
        self.resources.as_ref()?.get::<std::rc::Rc<std::cell::RefCell<super::ViewResources<G>>>>().cloned()
    }
    pub fn destroy(&mut self) -> Result<(), RuntimeError> {
        if let Some(destroy) = &self.destroy_resources {
            destroy()?;
        }
        self.resources = None;
        self.destroy_resources = None;
        Ok(())
    }
    pub fn initialize_camera(&mut self, camera: &Camera) {
        self.camera_clone = Some(CameraPose::new(camera));
        self.camera_start_fired = false;
        self.camera_moved_time = None;
    }
    /// Timestamp and wait duration are milliseconds, matching getTimestamp.
    pub fn check_for_camera_updates(&mut self, camera: &mut Camera, timestamp: f64, wait: f64) -> bool {
        let pose = CameraPose::new(camera);
        if self.camera_clone.as_ref().is_none_or(|c| !c.equals(&pose)) {
            if !self.camera_start_fired {
                camera.move_start.raise_event(&());
                self.camera_start_fired = true;
            }
            self.camera_moved_time = Some(timestamp);
            self.camera_clone = Some(pose);
            return true;
        }
        if self.camera_start_fired && self.camera_moved_time.is_some_and(|t| timestamp - t > wait) {
            camera.move_end.raise_event(&());
            self.camera_start_fired = false;
        }
        false
    }
    pub fn create_potentially_visible_set(
        &mut self,
        frame: &mut FrameState,
        options: &SceneOptions,
    ) -> Result<(), RuntimeError> {
        let camera = &frame.uniform.camera;
        let (_, _, range, _) = camera.frustum.parameters();
        let culling = CullingVolume::new(frame.culling_volume.planes.iter().take(5).copied().collect());
        let occluder = if frame.mode == SceneMode::Scene3D { frame.occluder.as_ref() } else { None };
        let mut extents = Vec::new();
        let (mut near, mut far) = (f64::MAX, -f64::MAX);
        let (mut shadow_near, mut shadow_far, mut closest) = (f64::MAX, -f64::MAX, f64::MAX);
        self.compute_commands.clear();
        self.overlay_commands.clear();
        for command in &frame.command_list {
            match command.pass {
                Pass::Compute => {
                    self.compute_commands.push(command.clone());
                    continue;
                }
                Pass::Overlay => {
                    self.overlay_commands.push(command.clone());
                    continue;
                }
                _ => {}
            }
            let (command_near, command_far) =
                if let Some(interval) = command.plane_distances(camera.position, camera.direction) {
                    if !command.visible_in(&culling, occluder) {
                        continue;
                    }
                    near = near.min(interval.start);
                    far = far.max(interval.stop);
                    if frame.shadow_state.shadows_enabled
                        && command.metadata.receive_shadows
                        && interval.start < 20000.
                        && !(command.pass == Pass::Globe && interval.start < -100. && interval.stop > 100.)
                    {
                        if command.pass != Pass::Globe && interval.start < 100. {
                            closest = closest.min(interval.stop - interval.start);
                        }
                        shadow_near = shadow_near.min(interval.start);
                        shadow_far = shadow_far.max(interval.stop);
                    }
                    (interval.start, interval.stop)
                } else {
                    if !command.is_clear {
                        near = near.min(range[0]);
                        far = far.max(range[1]);
                    }
                    (range[0], range[1])
                };
            extents.push((command.clone(), command_near, command_far));
        }
        if frame.shadow_state.shadows_enabled {
            shadow_near = shadow_near.max(range[0]).min(range[1]);
            shadow_far = shadow_far.min(range[1]).max(shadow_near);
            frame.shadow_state.near_plane = shadow_near;
            frame.shadow_state.far_plane = shadow_far;
            frame.shadow_state.closest_object_size = closest;
        }
        far *= 1.01;
        near = near.max(range[0]).min(range[1]);
        far = far.min(range[1]).max(near);
        let ratio =
            if frame.use_log_depth { options.logarithmic_depth_far_to_near_ratio } else { options.far_to_near_ratio };
        if !ratio.is_finite() || ratio <= 1. {
            return Err(RuntimeError::new("farToNearRatio must be finite and greater than one"));
        }
        let is_2d = frame.mode == SceneMode::Scene2D;
        let step = options.near_to_far_distance_2d;
        if !is_2d && (!range[0].is_finite() || range[0] <= 0. || !range[1].is_finite() || range[1] < range[0]) {
            return Err(RuntimeError::new("invalid camera frustum range"));
        }
        let count = if is_2d {
            if !step.is_finite() || step <= 0. {
                return Err(RuntimeError::new("nearToFarDistance2D must be positive and finite"));
            }
            far = far.min(frame.camera_position_local.z + step);
            near = near.min(far);
            ((far - near).max(1.) / step).ceil() as usize
        } else {
            ((far / near).ln() / ratio.ln()).ceil() as usize
        };
        self.frustum_commands_list.clear();
        for i in 0..count {
            let n =
                if is_2d { (far - step).min(near + i as f64 * step) } else { near.max(ratio.powi(i as i32) * near) };
            let f = if is_2d { far.min(n + step) } else { far.min(ratio * n) };
            self.frustum_commands_list.push(FrustumCommands::new(n, f));
        }
        let mut statistics = DebugFrustumStatistics::default();
        for (mut command, near, far) in extents {
            let mut overlap = 0u32;
            let mut bins = Vec::new();
            for (i, frustum) in self.frustum_commands_list.iter().enumerate() {
                if near > frustum.far {
                    continue;
                }
                if far < frustum.near {
                    break;
                }
                overlap |= 1u32.wrapping_shl(i as u32);
                bins.push(i);
                if command.metadata.execute_in_closest_frustum {
                    break;
                }
            }
            command.metadata.debug_overlapping_frustums = overlap;
            for i in bins {
                self.frustum_commands_list[i].commands[command.pass as usize].push(command.clone());
            }
            *statistics.commands_in_frustums.entry(overlap).or_default() += 1;
            statistics.total_commands += 1;
        }
        self.debug_frustum_statistics = options.debug_show_frustums.then_some(statistics);
        let first = frame.frustum_splits.first().copied().unwrap_or(f64::NAN);
        frame.frustum_splits = self.frustum_commands_list.iter().map(|f| f.near).collect();
        frame.frustum_splits.push(self.frustum_commands_list.last().map_or(first, |f| f.far));
        Ok(())
    }
}
