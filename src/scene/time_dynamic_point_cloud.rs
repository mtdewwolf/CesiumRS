//! CesiumJS `TimeDynamicPointCloud`: plays back a time series of pnts frames,
//! prefetching by clock speed and unloading frames past the memory budget.
use super::{
    point_cloud::{PointCloud, PointCloudCallbacks, PointCloudOptions},
    point_cloud_eye_dome_lighting::PointCloudEyeDomeLighting,
    point_cloud_shading::{PointCloudShading, PointCloudShadingOptions},
    scene_mode::SceneMode,
    shadow_mode::ShadowMode,
    ClippingPlaneCollection, FrameState,
};
use crate::{
    core::{
        clock::get_timestamp, BoundingSphere, Clock, Event, JulianDate, Matrix4, Resource, ResourceClient,
        ResourceFuture, RuntimeError, TimeIntervalCollection,
    },
    renderer::{Context, Gl, Pass, PickId, UniformValue},
};
use std::{
    cell::RefCell,
    collections::HashMap,
    rc::Rc,
    task::{Context as TaskContext, Poll, Waker},
};
type Result<T> = std::result::Result<T, RuntimeError>;

/// `intervals.data` for one point cloud frame.
#[derive(Clone, Debug, PartialEq)]
pub struct TimeDynamicPointCloudInterval {
    pub uri: String,
    pub transform: Option<Matrix4>,
}

/// `new TimeDynamicPointCloud(options)`.
pub struct TimeDynamicPointCloudOptions {
    pub clock: Clock,
    pub intervals: TimeIntervalCollection<TimeDynamicPointCloudInterval>,
    pub show: bool,
    pub model_matrix: Matrix4,
    pub shadows: ShadowMode,
    pub maximum_memory_usage: f64,
    pub shading: PointCloudShadingOptions,
    pub clipping_planes: Option<Rc<RefCell<ClippingPlaneCollection>>>,
}

struct Frame<G: Gl + 'static> {
    point_cloud: Option<PointCloud<G>>,
    transform: Option<Matrix4>,
    timestamp: f64,
    sequential: bool,
    ready: bool,
    touched_frame_number: u32,
    uri: String,
    request: Option<ResourceFuture<Vec<u8>>>,
}

struct TimeDynamicPickObject;

/// A time-dynamic point cloud primitive.
pub struct TimeDynamicPointCloud<G: Gl + 'static> {
    pub show: bool,
    pub model_matrix: Matrix4,
    pub shadows: ShadowMode,
    pub maximum_memory_usage: f64,
    pub shading: PointCloudShading,
    pub style: Option<Rc<RefCell<super::Cesium3DTileStyle>>>,
    pub frame_failed: Event<(String, String)>,
    pub frame_changed: Event<()>,
    pub clock: Clock,
    intervals: TimeIntervalCollection<TimeDynamicPointCloudInterval>,
    clipping_planes: Option<Rc<RefCell<ClippingPlaneCollection>>>,
    eye_dome_lighting: PointCloudEyeDomeLighting<G>,
    load_timestamp: Option<JulianDate>,
    clipping_planes_state: u64,
    style_dirty: bool,
    pick_id: Option<Rc<PickId>>,
    total_memory_usage_in_bytes: usize,
    frames: HashMap<usize, Frame<G>>,
    previous_interval: Option<usize>,
    next_interval: Option<usize>,
    last_rendered_frame: Option<usize>,
    clock_multiplier: f64,
    running_sum: f64,
    running_length: usize,
    running_index: usize,
    running_samples: [f64; 5],
    running_average: f64,
    client: Option<ResourceClient>,
    destroyed: bool,
}

impl<G: Gl> TimeDynamicPointCloud<G> {
    pub fn new(options: TimeDynamicPointCloudOptions) -> Self {
        Self {
            show: options.show,
            model_matrix: options.model_matrix,
            shadows: options.shadows,
            maximum_memory_usage: options.maximum_memory_usage,
            shading: PointCloudShading::new(options.shading),
            style: None,
            frame_failed: Event::new(),
            frame_changed: Event::new(),
            clock: options.clock,
            intervals: options.intervals,
            clipping_planes: options.clipping_planes,
            eye_dome_lighting: PointCloudEyeDomeLighting::new(),
            load_timestamp: None,
            clipping_planes_state: 0,
            style_dirty: false,
            pick_id: None,
            total_memory_usage_in_bytes: 0,
            frames: HashMap::new(),
            previous_interval: None,
            next_interval: None,
            last_rendered_frame: None,
            clock_multiplier: 0.0,
            running_sum: 0.0,
            running_length: 0,
            running_index: 0,
            running_samples: [0.0; 5],
            running_average: 0.0,
            client: None,
            destroyed: false,
        }
    }

    pub fn set_resource_client(&mut self, client: ResourceClient) {
        self.client = Some(client);
    }

    pub fn clock(&self) -> &Clock {
        &self.clock
    }

    pub fn clock_mut(&mut self) -> &mut Clock {
        &mut self.clock
    }

    pub fn set_clock(&mut self, clock: Clock) {
        self.clock = clock;
    }

    pub fn clipping_planes(&self) -> Option<&Rc<RefCell<ClippingPlaneCollection>>> {
        self.clipping_planes.as_ref()
    }

    pub fn set_clipping_planes(&mut self, planes: Option<Rc<RefCell<ClippingPlaneCollection>>>) {
        self.clipping_planes = planes;
    }

    pub fn total_memory_usage_in_bytes(&self) -> usize {
        self.total_memory_usage_in_bytes
    }

    pub fn bounding_sphere(&self) -> Option<BoundingSphere> {
        let frame = self.frames.get(&self.last_rendered_frame?)?;
        frame.point_cloud.as_ref().and_then(PointCloud::bounding_sphere)
    }

    pub fn eye_dome_lighting(&self) -> &PointCloudEyeDomeLighting<G> {
        &self.eye_dome_lighting
    }

    pub fn is_destroyed(&self) -> bool {
        self.destroyed
    }

    pub fn last_rendered_interval(&self) -> Option<usize> {
        self.last_rendered_frame
    }

    pub fn next_prefetch_interval(&self) -> Option<usize> {
        self.next_interval
    }

    pub fn loaded_frame_count(&self) -> usize {
        self.frames.values().filter(|f| f.ready).count()
    }

    pub fn make_style_dirty(&mut self) {
        self.style_dirty = true;
    }

    /// `_getAverageLoadTime`.
    pub fn average_load_time(&self) -> f64 {
        if self.running_length == 0 {
            0.05
        } else {
            self.running_average
        }
    }

    fn clock_multiplier(&self) -> f64 {
        if self.clock.can_animate && self.clock.should_animate() {
            self.clock.multiplier()
        } else {
            0.0
        }
    }

    fn current_interval(&self) -> Option<usize> {
        let index = self.intervals.index_of(&self.clock.current_time());
        (index >= 0).then_some(index as usize)
    }

    fn next_interval(&self, current: usize) -> Option<usize> {
        let multiplier = self.clock_multiplier();
        if multiplier == 0.0 {
            return None;
        }
        let average = self.average_load_time();
        let time = self.clock.current_time().add_seconds(average * multiplier);
        let mut index = self.intervals.index_of(&time);
        let current_index = current as isize;
        if index == current_index {
            if multiplier >= 0.0 {
                index += 1;
            } else {
                index -= 1;
            }
        }
        (index >= 0 && (index as usize) < self.intervals.len()).then_some(index as usize)
    }

    fn reached_interval(&self, current: usize, next: usize) -> bool {
        if self.clock_multiplier() >= 0.0 {
            current >= next
        } else {
            current <= next
        }
    }

    fn point_cloud_callbacks(&self) -> PointCloudCallbacks<G> {
        let pick_id = self.pick_id.clone();
        PointCloudCallbacks {
            fragment_shader_loaded: Some(Rc::new(|fs: &str| format!("uniform vec4 czm_pickColor;\n{fs}"))),
            uniform_map_loaded: pick_id.map(|id| {
                Rc::new(move |mut uniforms: Vec<(String, UniformValue)>| {
                    uniforms.push((
                        "czm_pickColor".into(),
                        UniformValue::Float(vec![
                            id.color.red as f32,
                            id.color.green as f32,
                            id.color.blue as f32,
                            id.color.alpha as f32,
                        ]),
                    ));
                    uniforms
                }) as super::point_cloud::PointCloudUniformMapLoaded
            }),
            pick_id_loaded: Some(Rc::new(|| "czm_pickColor".to_string())),
            ..PointCloudCallbacks::default()
        }
    }

    fn request_frame(&mut self, interval: usize, frame_number: u32) -> Option<&mut Frame<G>> {
        if let std::collections::hash_map::Entry::Vacant(entry) = self.frames.entry(interval) {
            let data = self.intervals.get(interval)?.data.clone()?;
            let mut frame = Frame {
                point_cloud: None,
                transform: data.transform,
                timestamp: get_timestamp(),
                sequential: true,
                ready: false,
                touched_frame_number: frame_number,
                uri: data.uri.clone(),
                request: None,
            };
            if let Some(client) = &self.client {
                if let Some(resource) = Resource::new(&data.uri) {
                    if let Some(request) = client.fetch_array_buffer(&resource) {
                        frame.request = Some(request);
                    }
                }
            }
            entry.insert(frame);
        }
        self.frames.get_mut(&interval)
    }

    fn poll_request(&mut self, interval: usize, waker: Option<&Waker>) {
        let callbacks = self.point_cloud_callbacks();
        let Some(frame) = self.frames.get_mut(&interval) else { return };
        let Some(request) = &mut frame.request else { return };
        let waker = waker.cloned().unwrap_or_else(noop_waker);
        let mut context = TaskContext::from_waker(&waker);
        if let Poll::Ready(result) = std::future::Future::poll(request.as_mut(), &mut context) {
            match result {
                Ok(bytes) => {
                    match PointCloud::new(PointCloudOptions {
                        array_buffer: std::sync::Arc::new(bytes),
                        byte_offset: 0,
                        cull: true,
                        opaque_pass: Pass::Opaque,
                        split_direction: super::SplitDirection::None,
                        callbacks,
                    }) {
                        Ok(cloud) => frame.point_cloud = Some(cloud),
                        Err(error) => {
                            if self.frame_failed.number_of_listeners() > 0 {
                                self.frame_failed.raise_event(&(frame.uri.clone(), error.to_string()));
                            }
                        }
                    }
                }
                Err(error) => {
                    if self.frame_failed.number_of_listeners() > 0 {
                        self.frame_failed.raise_event(&(frame.uri.clone(), error.to_string()));
                    }
                }
            }
            frame.request = None;
        }
    }

    fn update_average_load_time(&mut self, load_time: f64) {
        self.running_sum += load_time;
        self.running_sum -= self.running_samples[self.running_index];
        self.running_samples[self.running_index] = load_time;
        self.running_length = (self.running_length + 1).min(self.running_samples.len());
        self.running_index = (self.running_index + 1) % self.running_samples.len();
        self.running_average = self.running_sum / self.running_length as f64;
    }

    fn prepare_frame(&mut self, interval: usize, update_state: &UpdateState, frame_state: &mut FrameState) -> bool {
        let Some(frame) = self.frames.get_mut(&interval) else { return false };
        if frame.touched_frame_number + 1 < frame_state.frame_number {
            frame.sequential = false;
        }
        if frame.ready {
            frame.touched_frame_number = frame_state.frame_number;
            return true;
        }
        if frame.request.is_some() || frame.point_cloud.is_none() {
            frame.touched_frame_number = frame_state.frame_number;
            return false;
        }
        let length_before = frame_state.command_list.len();
        let updated = self.render_frame(interval, update_state, frame_state);
        let Some(frame) = self.frames.get_mut(&interval) else { return false };
        let became_ready = frame.point_cloud.as_ref().is_some_and(PointCloud::ready);
        let now_ready = updated && became_ready;
        let byte_length = frame.point_cloud.as_ref().map_or(0, PointCloud::geometry_byte_length);
        let sequential = frame.sequential;
        let timestamp = frame.timestamp;
        frame.ready = now_ready;
        frame.touched_frame_number = frame_state.frame_number;
        if now_ready {
            self.total_memory_usage_in_bytes += byte_length;
            frame_state.command_list.truncate(length_before);
            if sequential {
                let load_time = (get_timestamp() - timestamp) / 1000.0;
                self.update_average_load_time(load_time);
            }
        }
        now_ready
    }

    fn maximum_attenuation(&self) -> f64 {
        self.shading.maximum_attenuation.unwrap_or(10.0)
    }

    fn render_frame(&mut self, interval: usize, update_state: &UpdateState, frame_state: &mut FrameState) -> bool {
        let shading = self.shading;
        let maximum_attenuation = self.maximum_attenuation();
        let model_matrix = self.model_matrix;
        let style = self.style.clone();
        let clipping_planes = self.clipping_planes.clone();
        let shadows = self.shadows;
        let Some(frame) = self.frames.get_mut(&interval) else { return false };
        let uri = frame.uri.clone();
        let Some(point_cloud) = frame.point_cloud.as_mut() else { return false };
        let transform = frame.transform.unwrap_or(Matrix4::IDENTITY);
        point_cloud.model_matrix = model_matrix.multiply_transformation(&transform);
        point_cloud.style = style;
        point_cloud.time = update_state.time_since_load;
        point_cloud.shadows = shadows;
        point_cloud.clipping_planes = clipping_planes;
        point_cloud.is_clipped = update_state.is_clipped;
        point_cloud.attenuation = shading.attenuation;
        point_cloud.back_face_culling = shading.back_face_culling;
        point_cloud.normal_shading = shading.normal_shading;
        point_cloud.geometric_error = if let Some(base) = shading.base_resolution {
            base
        } else if let Some(sphere) = point_cloud.bounding_sphere() {
            let volume = 4.0 / 3.0 * std::f64::consts::PI * sphere.radius.powi(3);
            (volume / point_cloud.points_length().max(1) as f64).cbrt()
        } else {
            0.0
        };
        point_cloud.geometric_error_scale = shading.geometric_error_scale;
        point_cloud.maximum_attenuation = maximum_attenuation;
        let ok = match point_cloud.update(frame_state) {
            Ok(()) => true,
            Err(error) => {
                if self.frame_failed.number_of_listeners() > 0 {
                    self.frame_failed.raise_event(&(uri, error.to_string()));
                }
                false
            }
        };
        if let Some(frame) = self.frames.get_mut(&interval) {
            frame.touched_frame_number = frame_state.frame_number;
        }
        ok
    }

    fn set_frames_dirty(&mut self, clipping_planes_dirty: bool, style_dirty: bool) {
        for frame in self.frames.values_mut() {
            if let Some(point_cloud) = frame.point_cloud.as_mut() {
                if clipping_planes_dirty {
                    point_cloud.clipping_planes_dirty = true;
                }
                if style_dirty {
                    point_cloud.style_dirty = true;
                }
            }
        }
    }

    fn unload_frames(&mut self, frame_number: Option<u32>) {
        let mut removed = Vec::new();
        for (index, frame) in &self.frames {
            if frame_number.is_none_or(|number| frame.touched_frame_number < number) {
                removed.push(*index);
            }
        }
        for index in removed {
            if let Some(frame) = self.frames.remove(&index) {
                if frame.ready {
                    self.total_memory_usage_in_bytes = self
                        .total_memory_usage_in_bytes
                        .saturating_sub(frame.point_cloud.as_ref().map_or(0, PointCloud::geometry_byte_length));
                }
                if let Some(mut cloud) = frame.point_cloud {
                    let _ = cloud.destroy();
                }
                if self.last_rendered_frame == Some(index) {
                    self.last_rendered_frame = None;
                }
            }
        }
    }

    fn nearest_ready_interval(
        &mut self,
        previous: usize,
        current: usize,
        update_state: &UpdateState,
        frame_state: &mut FrameState,
    ) -> usize {
        if current >= previous {
            for index in (previous..=current).rev() {
                if self.prepare_frame(index, update_state, frame_state) {
                    return index;
                }
            }
        } else {
            for index in current..=previous {
                if self.prepare_frame(index, update_state, frame_state) {
                    return index;
                }
            }
        }
        previous
    }

    pub fn update(&mut self, frame_state: &mut FrameState) -> Result<()> {
        if frame_state.mode == SceneMode::Morphing || !self.show {
            return Ok(());
        }
        let Some(context) = frame_state
            .context
            .as_ref()
            .and_then(|resource| resource.get::<Context<G>>())
            .cloned()
        else {
            return Err(RuntimeError::new("TimeDynamicPointCloud requires the frame's renderer context"));
        };
        if self.pick_id.is_none() {
            self.pick_id = Some(Rc::new(context.create_pick_id(Rc::new(TimeDynamicPickObject))?));
        }
        if self.load_timestamp.is_none() {
            self.load_timestamp = Some(frame_state.time);
        }
        let time_since_load = JulianDate::seconds_difference(&frame_state.time, self.load_timestamp.as_ref().unwrap())
            .mul_add(1000.0, 0.0)
            .max(0.0);

        let is_clipped = self.clipping_planes.as_ref().is_some_and(|planes| planes.borrow().enabled);
        let state = if is_clipped {
            if let Some(planes) = &self.clipping_planes {
                planes.borrow_mut().update(&context)?;
                planes.borrow().state() as u64
            } else {
                0
            }
        } else {
            0
        };
        let clipping_planes_dirty = self.clipping_planes_state != state;
        self.clipping_planes_state = state;
        let style_dirty = self.style_dirty;
        self.style_dirty = false;
        if clipping_planes_dirty || style_dirty {
            self.set_frames_dirty(clipping_planes_dirty, style_dirty);
        }

        let update_state = UpdateState { time_since_load, is_clipped };
        let length_before_update = frame_state.command_list.len();

        let Some(current) = self.current_interval() else {
            return Ok(());
        };
        let multiplier = self.clock_multiplier();
        let clock_paused = multiplier == 0.0;
        let clock_multiplier_changed = multiplier != self.clock_multiplier;
        self.clock_multiplier = multiplier;
        let mut previous = self.previous_interval.unwrap_or(current);
        if clock_paused {
            previous = current;
        }
        if self.next_interval.is_none()
            || clock_multiplier_changed
            || self.next_interval.is_some_and(|next| self.reached_interval(current, next))
        {
            self.next_interval = self.next_interval(current);
        }

        let waker = frame_state.render_request.as_ref().map(|request| request.waker());
        // Poll any in-flight requests across the range we might inspect.
        let (lo, hi) = if current >= previous { (previous, current) } else { (current, previous) };
        for index in lo..=hi {
            self.poll_request(index, waker.as_ref());
        }
        if let Some(next) = self.next_interval {
            self.poll_request(next, waker.as_ref());
        }

        let previous = self.nearest_ready_interval(previous, current, &update_state, frame_state);
        let mut frame = self.frames.get(&previous).filter(|frame| frame.ready).map(|_| previous);
        if frame.is_none() {
            self.request_frame(previous, frame_state.frame_number);
            self.poll_request(previous, waker.as_ref());
            if self.prepare_frame(previous, &update_state, frame_state) {
                frame = Some(previous);
            } else {
                frame = self.last_rendered_frame;
            }
        }
        if let Some(frame_idx) = frame {
            let _ = self.render_frame(frame_idx, &update_state, frame_state);
        }
        if let Some(next) = self.next_interval {
            self.request_frame(next, frame_state.frame_number);
            self.poll_request(next, waker.as_ref());
            let _ = self.prepare_frame(next, &update_state, frame_state);
        }

        let changed = frame.is_some() && frame != self.last_rendered_frame;
        if changed && self.frame_changed.number_of_listeners() > 0 {
            self.frame_changed.raise_event(&());
        }
        self.previous_interval = Some(previous);
        self.last_rendered_frame = frame;
        let maximum = self.maximum_memory_usage * 1024.0 * 1024.0;
        if self.total_memory_usage_in_bytes as f64 > maximum {
            self.unload_frames(Some(frame_state.frame_number));
        }

        let added_commands_length = frame_state.command_list.len().saturating_sub(length_before_update);
        let shading = self.shading;
        if shading.attenuation && shading.eye_dome_lighting && added_commands_length > 0 {
            self.eye_dome_lighting.update(
                frame_state,
                length_before_update,
                &shading,
                self.bounding_sphere(),
            )?;
        }
        Ok(())
    }

    pub fn destroy(&mut self) -> Result<()> {
        self.unload_frames(None);
        if let Some(planes) = self.clipping_planes.take() {
            planes.borrow_mut().destroy()?;
        }
        if let Some(pick_id) = self.pick_id.take() {
            pick_id.destroy();
        }
        self.eye_dome_lighting.destroy()?;
        self.frames.clear();
        self.destroyed = true;
        Ok(())
    }
}

struct UpdateState {
    time_since_load: f64,
    is_clipped: bool,
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
