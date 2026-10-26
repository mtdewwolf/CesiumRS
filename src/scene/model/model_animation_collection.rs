//! CesiumJS `Scene/Model/ModelAnimationCollection`: the active animations of a
//! model.
//!
//! Cesium raises `start`, `update`, `stop` and `animationRemoved` at the end of
//! the frame through `frameState.afterRender`. The port queues those events and
//! the host raises them with [`ModelAnimationCollection::raise_pending_events`]
//! after rendering, so event ordering matches without borrowing the frame
//! state.

use super::{ModelAnimation, ModelAnimationEvent, ModelAnimationOptions, ModelRuntimeNode};
use crate::{
    core::{Event, JulianDate, RuntimeError},
    renderer::Gl,
    scene::{model_components as mc, ModelAnimationLoop, ModelAnimationState},
};

/// An event to raise after the current frame renders.
#[derive(Clone, Debug, PartialEq)]
enum PendingAnimationEvent {
    Start(ModelAnimationEvent),
    Update(ModelAnimationEvent),
    Stop(ModelAnimationEvent),
    Removed(ModelAnimationEvent),
}

/// A collection of active model animations.
///
/// Obtain one from a loaded model; do not construct it directly.
pub struct ModelAnimationCollection {
    /// Fired when an animation is added.
    pub animation_added: Event<ModelAnimationEvent>,
    /// Fired when an animation is removed.
    pub animation_removed: Event<ModelAnimationEvent>,
    /// When true, animations play even when the scene time is paused. Whether
    /// they advance still depends on their `animationTime` callbacks.
    pub animate_while_paused: bool,

    runtime_animations: Vec<ModelAnimation>,
    stopped_animations: Vec<ModelAnimation>,
    previous_time: Option<JulianDate>,
    pending_events: Vec<PendingAnimationEvent>,
    next_id: usize,
}

impl Default for ModelAnimationCollection {
    fn default() -> Self {
        Self::new()
    }
}

impl ModelAnimationCollection {
    pub fn new() -> Self {
        Self {
            animation_added: Event::new(),
            animation_removed: Event::new(),
            animate_while_paused: false,
            runtime_animations: Vec::new(),
            stopped_animations: Vec::new(),
            previous_time: None,
            pending_events: Vec::new(),
            next_id: 0,
        }
    }

    /// The number of animations in the collection.
    pub fn length(&self) -> usize {
        self.runtime_animations.len()
    }

    pub fn is_empty(&self) -> bool {
        self.runtime_animations.is_empty()
    }

    /// The animation at `index`, or `None` when the index is out of range.
    pub fn get(&self, index: usize) -> Option<&ModelAnimation> {
        self.runtime_animations.get(index)
    }

    pub fn get_mut(&mut self, index: usize) -> Option<&mut ModelAnimation> {
        self.runtime_animations.get_mut(index)
    }

    /// The animations in the collection, in add order.
    pub fn animations(&self) -> &[ModelAnimation] {
        &self.runtime_animations
    }

    /// Creates and adds an animation selected by `options.name` or
    /// `options.index`.
    ///
    /// The caller is responsible for the model being ready, as Cesium's
    /// `Model#ready` check does.
    pub fn add<G: Gl, D>(
        &mut self,
        animations: &[mc::Animation],
        runtime_nodes: &[Option<ModelRuntimeNode<G, D>>],
        options: &ModelAnimationOptions,
    ) -> Result<usize, RuntimeError> {
        if options.name.is_none() && options.index.is_none() {
            return Err(RuntimeError::new("Either options.name or options.index must be defined."));
        }
        if let Some(multiplier) = options.multiplier {
            if multiplier <= 0.0 {
                return Err(RuntimeError::new("options.multiplier must be greater than zero."));
            }
        }
        if let Some(index) = options.index {
            if index >= animations.len() {
                return Err(RuntimeError::new("options.index must be a valid animation index."));
            }
            return self.add_animation(index, &animations[index], runtime_nodes, options);
        }

        let name = options.name.as_deref().unwrap_or_default();
        let index = animations
            .iter()
            .position(|animation| animation.name.as_deref() == Some(name))
            .ok_or_else(|| RuntimeError::new("options.name must be a valid animation name."))?;
        self.add_animation(index, &animations[index], runtime_nodes, options)
    }

    /// Adds one glTF animation by index.
    pub fn add_animation<G: Gl, D>(
        &mut self,
        animation_index: usize,
        animation: &mc::Animation,
        runtime_nodes: &[Option<ModelRuntimeNode<G, D>>],
        options: &ModelAnimationOptions,
    ) -> Result<usize, RuntimeError> {
        let mut runtime_animation = ModelAnimation::new(animation_index, animation, runtime_nodes, options)?;
        runtime_animation.set_id(self.next_id);
        self.next_id += 1;
        let id = runtime_animation.id();
        let args = runtime_animation.event_args();
        self.runtime_animations.push(runtime_animation);
        self.animation_added.raise_event(&args);
        Ok(id)
    }

    /// Adds every animation in the model, as `ModelAnimationCollection#addAll`.
    pub fn add_all<G: Gl, D>(
        &mut self,
        animations: &[mc::Animation],
        runtime_nodes: &[Option<ModelRuntimeNode<G, D>>],
        options: &ModelAnimationOptions,
    ) -> Result<Vec<usize>, RuntimeError> {
        if let Some(multiplier) = options.multiplier {
            if multiplier <= 0.0 {
                return Err(RuntimeError::new("options.multiplier must be greater than zero."));
            }
        }
        let mut added = Vec::with_capacity(animations.len());
        for (index, animation) in animations.iter().enumerate() {
            added.push(self.add_animation(index, animation, runtime_nodes, options)?);
        }
        Ok(added)
    }

    /// Removes an animation by its collection id.
    pub fn remove(&mut self, animation_id: usize) -> bool {
        let Some(index) = self.runtime_animations.iter().position(|a| a.id() == animation_id) else {
            return false;
        };
        let animation = self.runtime_animations.remove(index);
        self.animation_removed.raise_event(&animation.event_args());
        true
    }

    /// Removes all animations, raising `animationRemoved` for each.
    pub fn remove_all(&mut self) {
        let animations = std::mem::take(&mut self.runtime_animations);
        for animation in &animations {
            self.animation_removed.raise_event(&animation.event_args());
        }
    }

    /// Whether this collection contains the animation with the given id.
    pub fn contains(&self, animation_id: usize) -> bool {
        self.runtime_animations.iter().any(|animation| animation.id() == animation_id)
    }

    /// Events queued for the end of this frame, in the order they occurred.
    pub fn pending_event_count(&self) -> usize {
        self.pending_events.len()
    }

    /// Raises the events queued by [`Self::update`], as Cesium does from
    /// `frameState.afterRender`.
    pub fn raise_pending_events(&mut self) {
        let pending = std::mem::take(&mut self.pending_events);
        for event in pending {
            match event {
                PendingAnimationEvent::Start(args) => {
                    if let Some(animation) = self.find_mut(args.animation) {
                        animation.start.raise_event(&args);
                    }
                }
                PendingAnimationEvent::Update(args) => {
                    if let Some(animation) = self.find_mut(args.animation) {
                        animation.update.raise_event(&args);
                    }
                }
                PendingAnimationEvent::Stop(args) => {
                    let animation = self
                        .runtime_animations
                        .iter_mut()
                        .find(|a| a.id() == args.animation)
                        .or_else(|| self.stopped_animations.iter_mut().find(|a| a.id() == args.animation));
                    if let Some(animation) = animation {
                        animation.stop.raise_event(&args);
                    }
                }
                PendingAnimationEvent::Removed(args) => self.animation_removed.raise_event(&args),
            }
        }
        self.stopped_animations.clear();
    }

    fn find_mut(&mut self, animation_id: usize) -> Option<&mut ModelAnimation> {
        self.runtime_animations.iter_mut().find(|animation| animation.id() == animation_id)
    }

    /// Updates the runtime animations, removing any that stopped.
    ///
    /// `clamp_animations` is `model.clampAnimations`. Returns whether any
    /// animation played during this update.
    pub fn update<G: Gl, D>(
        &mut self,
        scene_time: &JulianDate,
        clamp_animations: bool,
        runtime_nodes: &mut [Option<ModelRuntimeNode<G, D>>],
    ) -> Result<bool, RuntimeError> {
        if self.runtime_animations.is_empty() {
            self.previous_time = None;
            return Ok(false);
        }

        if !self.animate_while_paused && self.previous_time.as_ref().is_some_and(|t| *t == *scene_time) {
            return Ok(false);
        }
        self.previous_time = Some(*scene_time);

        let mut animation_occurred = false;
        let mut animations_to_remove = Vec::new();

        for i in 0..self.runtime_animations.len() {
            let runtime_animation = &mut self.runtime_animations[i];

            if runtime_animation.computed_start_time().is_none() {
                let start = runtime_animation.start_time().copied().unwrap_or(*scene_time);
                runtime_animation.set_computed_start_time(start.add_seconds(runtime_animation.delay()));
            }
            if runtime_animation.duration().is_none() {
                let duration = runtime_animation.local_stop_time() * (1.0 / runtime_animation.multiplier());
                runtime_animation.set_duration(duration);
            }

            let start_time = runtime_animation.computed_start_time().unwrap_or(*scene_time);
            let duration = runtime_animation.duration().unwrap_or(0.0);
            let stop_time = runtime_animation.stop_time().copied();

            let past_start_time = JulianDate::less_than_or_equals(&start_time, scene_time);
            let reached_stop_time = stop_time.is_some_and(|stop_time| JulianDate::greater_than(scene_time, &stop_time));

            // [0.0, 1.0] normalized local animation time.
            let mut delta = 0.0;
            if duration != 0.0 {
                let seconds = JulianDate::seconds_difference(
                    &if reached_stop_time { stop_time.unwrap_or(*scene_time) } else { *scene_time },
                    &start_time,
                );
                delta = match runtime_animation.animation_time() {
                    Some(animation_time) => animation_time(duration, seconds),
                    None => seconds / duration,
                };
            }

            let repeat = matches!(
                runtime_animation.loop_mode(),
                ModelAnimationLoop::Repeat | ModelAnimationLoop::MirroredRepeat
            );
            let play = (past_start_time || (repeat && runtime_animation.start_time().is_none()))
                && (delta <= 1.0 || repeat)
                && !reached_stop_time;

            if runtime_animation.previous_animation_delta() == Some(delta) {
                let animation_stopped = runtime_animation.state() == ModelAnimationState::Stopped;
                // No change to delta and no change to the state: skip.
                if play != animation_stopped {
                    continue;
                }
            }
            runtime_animation.set_previous_animation_delta(delta);

            if play || runtime_animation.state() == ModelAnimationState::Animating {
                // Transition from STOPPED to ANIMATING.
                if play && runtime_animation.state() == ModelAnimationState::Stopped {
                    runtime_animation.set_state(ModelAnimationState::Animating);
                    if runtime_animation.start.number_of_listeners() > 0 {
                        self.pending_events.push(PendingAnimationEvent::Start(runtime_animation.event_args()));
                    }
                }

                // Truncate to [0.0, 1.0] for repeating animations.
                match runtime_animation.loop_mode() {
                    ModelAnimationLoop::Repeat => delta -= delta.floor(),
                    ModelAnimationLoop::MirroredRepeat => {
                        let floor = delta.floor();
                        let fract = delta - floor;
                        // Odd repetitions mirror.
                        delta = if (floor % 2.0) == 1.0 { 1.0 - fract } else { fract };
                    }
                    ModelAnimationLoop::None => {}
                }

                if runtime_animation.reverse() {
                    delta = 1.0 - delta;
                }

                let local_animation_time = crate::core::math::clamp(
                    delta * duration * runtime_animation.multiplier(),
                    runtime_animation.local_start_time(),
                    runtime_animation.local_stop_time(),
                );

                runtime_animation.animate(local_animation_time, runtime_nodes, clamp_animations)?;

                if runtime_animation.update.number_of_listeners() > 0 {
                    runtime_animation.set_update_event_time(local_animation_time);
                    let mut args = runtime_animation.event_args();
                    args.time = Some(local_animation_time);
                    self.pending_events.push(PendingAnimationEvent::Update(args));
                }
                animation_occurred = true;

                if !play {
                    // Transition from ANIMATING to STOPPED.
                    runtime_animation.set_state(ModelAnimationState::Stopped);
                    if runtime_animation.stop.number_of_listeners() > 0 {
                        self.pending_events.push(PendingAnimationEvent::Stop(runtime_animation.event_args()));
                    }
                    if runtime_animation.remove_on_stop() {
                        animations_to_remove.push(runtime_animation.id());
                    }
                }
            }
        }

        for animation_id in animations_to_remove {
            if let Some(index) = self.runtime_animations.iter().position(|a| a.id() == animation_id) {
                let animation = self.runtime_animations.remove(index);
                self.pending_events.push(PendingAnimationEvent::Removed(animation.event_args()));
                self.stopped_animations.push(animation);
            }
        }

        Ok(animation_occurred)
    }
}

impl std::fmt::Debug for ModelAnimationCollection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ModelAnimationCollection")
            .field("length", &self.runtime_animations.len())
            .field("animate_while_paused", &self.animate_while_paused)
            .finish_non_exhaustive()
    }
}
