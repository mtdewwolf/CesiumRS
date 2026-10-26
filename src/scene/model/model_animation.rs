//! CesiumJS `Scene/Model/ModelAnimation`: one active instance of a glTF
//! animation.
//!
//! An animation owns a channel per target and tracks its own play state. The
//! collection drives it with the scene time; the animation itself only knows
//! local animation time.

use super::{ModelAnimationChannel, ModelRuntimeNode};
use crate::{
    core::{Event, JulianDate, RuntimeError},
    renderer::Gl,
    scene::{model_components as mc, ModelAnimationLoop, ModelAnimationState},
};
use std::rc::Rc;

/// `ModelAnimation.AnimationTimeCallback`: computes the local animation time.
///
/// Arguments are the animation's original duration in seconds and the seconds
/// since it started, in scene time.
pub type AnimationTimeCallback = Rc<dyn Fn(f64, f64) -> f64>;

/// Arguments raised by an animation's `start`, `update` and `stop` events.
#[derive(Clone, Debug, PartialEq)]
pub struct ModelAnimationEvent {
    /// Index of the animation in its collection.
    pub animation: usize,
    pub name: Option<String>,
    /// The local animation time for `update`; `None` for `start` and `stop`.
    pub time: Option<f64>,
}

/// Options for [`super::ModelAnimationCollection::add`].
#[derive(Clone, Default)]
pub struct ModelAnimationOptions {
    pub name: Option<String>,
    pub index: Option<usize>,
    pub start_time: Option<JulianDate>,
    pub delay: f64,
    pub stop_time: Option<JulianDate>,
    pub remove_on_stop: bool,
    pub multiplier: Option<f64>,
    pub reverse: bool,
    pub loop_mode: Option<ModelAnimationLoop>,
    pub animation_time: Option<AnimationTimeCallback>,
}

/// An active animation derived from a glTF asset.
///
/// Create animations with
/// [`ModelAnimationCollection::add`](super::ModelAnimationCollection::add).
pub struct ModelAnimation {
    id: usize,
    animation_index: usize,
    name: Option<String>,
    runtime_channels: Vec<ModelAnimationChannel>,
    local_start_time: f64,
    local_stop_time: f64,
    start_time: Option<JulianDate>,
    delay: f64,
    stop_time: Option<JulianDate>,
    remove_on_stop: bool,
    multiplier: f64,
    reverse: bool,
    loop_mode: ModelAnimationLoop,
    animation_time: Option<AnimationTimeCallback>,
    computed_start_time: Option<JulianDate>,
    duration: Option<f64>,
    state: ModelAnimationState,
    previous_animation_delta: Option<f64>,
    update_event_time: f64,

    pub start: Event<ModelAnimationEvent>,
    pub update: Event<ModelAnimationEvent>,
    pub stop: Event<ModelAnimationEvent>,
}

impl ModelAnimation {
    /// Builds the runtime animation from its glTF animation and the scene
    /// graph's runtime nodes. Channels whose target node is undefined are
    /// skipped, as in Cesium.
    pub fn new<G: Gl, D>(
        animation_index: usize,
        animation: &mc::Animation,
        runtime_nodes: &[Option<ModelRuntimeNode<G, D>>],
        options: &ModelAnimationOptions,
    ) -> Result<Self, RuntimeError> {
        if let Some(multiplier) = options.multiplier {
            if multiplier <= 0.0 {
                return Err(RuntimeError::new("options.multiplier must be greater than zero."));
            }
        }
        let multiplier = options.multiplier.unwrap_or(1.0);

        let mut runtime_channels = Vec::with_capacity(animation.channels.len());
        let mut local_start_time = f64::MAX;
        let mut local_stop_time = f64::MIN;
        for channel in &animation.channels {
            let Some(target) = &channel.target else {
                continue;
            };
            let Some(node_index) = target.node else {
                continue;
            };
            let Some(sampler_index) = channel.sampler else {
                continue;
            };
            let Some(sampler) = animation.samplers.get(sampler_index) else {
                continue;
            };
            let Some(runtime_node) = runtime_nodes.get(node_index).and_then(Option::as_ref) else {
                continue;
            };
            let runtime_channel =
                ModelAnimationChannel::new(channel, sampler, node_index, runtime_node.morph_weights().len())?;
            if let (Some(first), Some(last)) = (sampler.input.first(), sampler.input.last()) {
                local_start_time = local_start_time.min(*first);
                local_stop_time = local_stop_time.max(*last);
            }
            runtime_channels.push(runtime_channel);
        }
        // Cesium leaves NaN when there are no usable channels; keep the same
        // sentinel so the collection's duration math matches.
        if runtime_channels.is_empty() {
            local_start_time = f64::MAX;
            local_stop_time = -f64::MAX;
        }

        let runtime_animation = Self {
            id: 0,
            animation_index,
            name: animation.name.clone(),
            runtime_channels,
            local_start_time,
            local_stop_time,
            start_time: options.start_time,
            delay: options.delay,
            stop_time: options.stop_time,
            remove_on_stop: options.remove_on_stop,
            multiplier,
            reverse: options.reverse,
            loop_mode: options.loop_mode.unwrap_or(ModelAnimationLoop::None),
            animation_time: options.animation_time.clone(),
            computed_start_time: None,
            duration: None,
            state: ModelAnimationState::Stopped,
            previous_animation_delta: None,
            update_event_time: 0.0,
            start: Event::new(),
            update: Event::new(),
            stop: Event::new(),
        };
        Ok(runtime_animation)
    }

    /// The index of the glTF animation in the model components.
    pub fn animation_index(&self) -> usize {
        self.animation_index
    }

    /// The collection-unique id of this animation instance. Several active
    /// animations can refer to the same glTF animation.
    pub fn id(&self) -> usize {
        self.id
    }

    pub(crate) fn set_id(&mut self, id: usize) {
        self.id = id;
    }

    /// The name that identifies this animation, if it exists.
    pub fn name(&self) -> Option<&str> {
        self.name.as_deref()
    }

    pub fn name_owned(&self) -> Option<String> {
        self.name.clone()
    }

    /// The runtime channels for this animation.
    pub fn runtime_channels(&self) -> &[ModelAnimationChannel] {
        &self.runtime_channels
    }

    /// The minimum keyframe time across the animation.
    pub fn local_start_time(&self) -> f64 {
        self.local_start_time
    }

    /// The maximum keyframe time across the animation.
    pub fn local_stop_time(&self) -> f64 {
        self.local_stop_time
    }

    pub fn start_time(&self) -> Option<&JulianDate> {
        self.start_time.as_ref()
    }

    pub fn delay(&self) -> f64 {
        self.delay
    }

    pub fn stop_time(&self) -> Option<&JulianDate> {
        self.stop_time.as_ref()
    }

    pub fn multiplier(&self) -> f64 {
        self.multiplier
    }

    pub fn reverse(&self) -> bool {
        self.reverse
    }

    pub fn loop_mode(&self) -> ModelAnimationLoop {
        self.loop_mode
    }

    pub fn animation_time(&self) -> Option<&AnimationTimeCallback> {
        self.animation_time.as_ref()
    }

    /// When true, the collection removes the animation after it stops.
    pub fn remove_on_stop(&self) -> bool {
        self.remove_on_stop
    }

    pub fn state(&self) -> ModelAnimationState {
        self.state
    }

    pub(crate) fn set_state(&mut self, state: ModelAnimationState) {
        self.state = state;
    }

    pub(crate) fn computed_start_time(&self) -> Option<JulianDate> {
        self.computed_start_time
    }

    pub(crate) fn set_computed_start_time(&mut self, start_time: JulianDate) {
        self.computed_start_time = Some(start_time);
    }

    pub(crate) fn duration(&self) -> Option<f64> {
        self.duration
    }

    pub(crate) fn set_duration(&mut self, duration: f64) {
        self.duration = Some(duration);
    }

    pub(crate) fn previous_animation_delta(&self) -> Option<f64> {
        self.previous_animation_delta
    }

    pub(crate) fn set_previous_animation_delta(&mut self, delta: f64) {
        self.previous_animation_delta = Some(delta);
    }

    pub(crate) fn set_update_event_time(&mut self, time: f64) {
        self.update_event_time = time;
    }

    pub fn update_event_time(&self) -> f64 {
        self.update_event_time
    }

    pub fn event_args(&self) -> ModelAnimationEvent {
        ModelAnimationEvent { animation: self.id, name: self.name.clone(), time: None }
    }

    /// Evaluates all animation channels at the given local animation time.
    pub fn animate<G: Gl, D>(
        &self,
        time: f64,
        runtime_nodes: &mut [Option<ModelRuntimeNode<G, D>>],
        clamp_animations: bool,
    ) -> Result<(), RuntimeError> {
        for channel in &self.runtime_channels {
            channel.animate(time, runtime_nodes, clamp_animations)?;
        }
        Ok(())
    }
}

impl std::fmt::Debug for ModelAnimation {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ModelAnimation")
            .field("animation_index", &self.animation_index)
            .field("name", &self.name)
            .field("state", &self.state)
            .field("channels", &self.runtime_channels.len())
            .finish_non_exhaustive()
    }
}
