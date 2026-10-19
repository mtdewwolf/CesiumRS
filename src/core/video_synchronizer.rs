//! CesiumJS video synchronization through an explicit media adapter.
//!
//! The host calls `on_tick` after clock ticks and `on_seeked` for media seeked
//! events. Dropping/replacing a media adapter removes host subscriptions.

use super::{iso8601, Clock, JulianDate, RuntimeError};

pub trait VideoElement {
    fn ready_state(&self) -> u8;
    fn paused(&self) -> bool;
    fn current_time(&self) -> f64;
    fn duration(&self) -> f64;
    fn looping(&self) -> bool;
    fn play(&mut self);
    fn pause(&mut self);
    fn set_playback_rate(&mut self, rate: f64) -> Result<(), RuntimeError>;
    fn set_current_time(&mut self, time: f64);
}

pub struct VideoSynchronizer<V> {
    pub epoch: JulianDate,
    pub tolerance: f64,
    element: Option<V>,
    last_playback_rate: Option<f64>,
    seeking: bool,
    first_tick_after_seek: bool,
    destroyed: bool,
}

impl<V: VideoElement> VideoSynchronizer<V> {
    pub fn new(element: Option<V>, epoch: Option<JulianDate>, tolerance: Option<f64>) -> Self {
        Self {
            epoch: epoch.unwrap_or_else(iso8601::minimum_value),
            tolerance: tolerance.unwrap_or(1.0),
            element,
            last_playback_rate: None,
            seeking: false,
            first_tick_after_seek: false,
            destroyed: false,
        }
    }
    pub fn element(&self) -> Option<&V> {
        self.element.as_ref()
    }
    pub fn element_mut(&mut self) -> Option<&mut V> {
        self.element.as_mut()
    }
    pub fn set_element(&mut self, element: Option<V>) {
        self.element = element;
        self.seeking = false;
        self.first_tick_after_seek = false;
    }
    pub fn on_seeked(&mut self) {
        self.seeking = false;
        self.first_tick_after_seek = true;
    }
    pub fn is_destroyed(&self) -> bool {
        self.destroyed
    }
    pub fn destroy(&mut self) {
        self.element = None;
        self.destroyed = true;
    }

    pub fn on_tick(&mut self, clock: &Clock) {
        if self.destroyed {
            return;
        }
        let Some(element) = &mut self.element else {
            return;
        };
        if element.ready_state() < 2 {
            return;
        }
        let should_animate = clock.should_animate();
        if should_animate == element.paused() {
            if should_animate {
                element.play();
            } else {
                element.pause();
            }
        }
        if self.seeking || self.first_tick_after_seek {
            self.first_tick_after_seek = false;
            return;
        }
        let rate = clock.multiplier();
        if self.last_playback_rate != Some(rate) {
            if element.set_playback_rate(rate).is_err() {
                let _ = element.set_playback_rate(0.0);
            }
            self.last_playback_rate = Some(rate);
        }
        let mut video_time = JulianDate::seconds_difference(&clock.current_time(), &self.epoch);
        let duration = element.duration();
        let desired_time = if element.looping() {
            video_time %= duration;
            // Preserve Cesium's negative-time wrapping, even above duration.
            if video_time < 0.0 {
                video_time = duration - video_time;
            }
            video_time
        } else if video_time > duration {
            duration
        } else if video_time < 0.0 {
            0.0
        } else {
            video_time
        };
        let tolerance = if should_animate { self.tolerance } else { 0.001 };
        if (desired_time - element.current_time()).abs() > tolerance {
            self.seeking = true;
            element.set_current_time(desired_time);
        }
    }
}
