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
    renderer::{Context, Gl, Pass},
};
use std::{
    collections::HashMap,
    rc::Rc,
    task::{Context as TaskContext, Poll, Waker},
    cell::RefCell,
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
    clock: Clock,
    intervals: TimeIntervalCollection<TimeDynamicPointCloudInterval>,
    clipping_planes: Option<Rc<RefCell<ClippingPlaneCollection>>>,
    eye_dome_lighting: PointCloudEyeDomeLighting<G>,
    load_timestamp: Option<JulianDate>,
    clipping_planes_state: u64,
    style_dirty: bool,
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

    pub fn total_memory_usage_in_bytes(&self) -> usize {
        self.total_memory_usage_in_bytes
    }

    pub fn bounding_sphere(&self) -> Option<BoundingSphere> {
        let frame = self.frames.get(&self.last_rendered_frame?)?;
        frame.point_cloud.as_ref().and_then(PointCloud::bounding_sphere)
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
        let time = self
            .clock
            .current_time()
            .add_seconds(average * multiplier);
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
        let Some(frame) = self.frames.get_mut(&interval) else { return };
        let Some(request) = &mut frame.request else { return };
        let waker = waker.cloned().unwrap_or_else(noop_waker);
        let mut context = TaskContext::from_waker(&waker);
        if let Poll::Ready(result) = std::future::Future::poll(request.as_mut(), &mut context) {
            match result {
                Ok(bytes) => {
                    frame.point_cloud = Some(PointCloud::new(PointCloudOptions {
                        array_buffer: std::sync::Arc::new(bytes),
                        byte_offset: 0,
                        cull: true,
                        opaque_pass: Pass::Cesium3DTile,
                        split_direction: super::SplitDirection::None,
                        callbacks: PointCloudCallbacks::default(),
                    }).ok().unwrap_or_else(|| panic!("point cloud creation failed")));
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

    fn prepare_frame(&mut self, interval: usize, frame_state: &mut FrameState) -> bool {
        let Some(frame) = self.frames.get_mut(&interval) else { return false };
        if frame.touched_frame_number + 1 < frame_state.frame_number {
            frame.sequential = false;
        }
        if frame.ready {
            frame.touched_frame_number = frame_state.frame_number;
            return true;
        }
        if frame.request.is_some() {
            frame.touched_frame_number = frame_state.frame_number;
            return false;
        }
        let updated = frame.point_cloud.as_mut().map(|cloud| cloud.update(frame_state).is_ok()).unwrap_or(false);
        let became_ready = frame.point_cloud.as_ref().is_some_and(PointCloud::ready);
        let now_ready = updated && became_ready;
        let byte_length = frame.point_cloud.as_ref().map_or(0, PointCloud::geometry_byte_length);
        let sequential = frame.sequential;
        let timestamp = frame.timestamp;
        frame.ready = now_ready;
        frame.touched_frame_number = frame_state.frame_number;
        if now_ready {
            self.total_memory_usage_in_bytes += byte_length;
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
        point_cloud.update(frame_state).is_ok()
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
        frame_state: &mut FrameState,
    ) -> usize {
        if current >= previous {
            for index in (previous..=current).rev() {
                if self.prepare_frame(index, frame_state) {
                    return index;
                }
            }
        } else {
            for index in current..=previous {
                if self.prepare_frame(index, frame_state) {
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
        if self.load_timestamp.is_none() {
            self.load_timestamp = Some(frame_state.time);
        }
        let time_since_load = JulianDate::seconds_difference(
            &frame_state.time,
            self.load_timestamp.as_ref().unwrap(),
        )
        .mul_add(1000.0, 0.0)
        .max(0.0);

        let is_clipped = self
            .clipping_planes
            .as_ref()
            .is_some_and(|planes| planes.borrow().enabled);
        if let Some(planes) = &self.clipping_planes {
            planes.borrow_mut().update(&context)?;
        }
        let state = self
            .clipping_planes
            .as_ref()
            .map_or(0, |planes| planes.borrow().state() as u64);
        let clipping_planes_dirty = self.clipping_planes_state != state;
        self.clipping_planes_state = state;
        let style_dirty = self.style_dirty;
        self.style_dirty = false;

        let current = self.current_interval();
        let Some(current) = current else { return Ok(()) };
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
            || self
                .next_interval
                .is_some_and(|next| self.reached_interval(current, next))
        {
            self.next_interval = self.next_interval(current);
        }
        let waker = frame_state.render_request.as_ref().map(|request| request.waker());
        self.poll_request(previous, waker.as_ref());
        if let Some(next) = self.next_interval {
            self.poll_request(next, waker.as_ref());
        }
        let previous = self.nearest_ready_interval(previous, current, frame_state);
        let mut frame = self.frames.get(&previous).filter(|frame| frame.ready).map(|_| previous);
        if frame.is_none() {
            self.request_frame(previous, frame_state.frame_number);
            frame = self.last_rendered_frame;
        }
        let update_state = UpdateState { time_since_load, is_clipped };
        if let Some(frame) = frame {
            let _ = self.render_frame(frame, &update_state, frame_state);
        }
        if let Some(next) = self.next_interval {
            self.request_frame(next, frame_state.frame_number);
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
        let shading = self.shading;
        if shading.attenuation && shading.eye_dome_lighting {
            let _ = self.eye_dome_lighting.update(
                frame_state,
                0,
                &shading,
                self.bounding_sphere(),
            );
        }
        let _ = (clipping_planes_dirty, style_dirty);
        Ok(())
    }

    pub fn destroy(&mut self) -> Result<()> {
        self.unload_frames(None);
        if let Some(planes) = self.clipping_planes.take() {
            planes.borrow_mut().destroy()?;
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
