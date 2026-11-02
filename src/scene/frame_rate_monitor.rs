//! CesiumJS `FrameRateMonitor`: monitors average frames per second over a
//! sliding window and triggers events when the frame rate drops below or recovers
//! to acceptable performance thresholds.
//!
//! Cesium subscribes to `scene.preUpdate` and `document` visibility changes and
//! reads `getTimestamp()`. Here the host calls [`FrameRateMonitor::update`] with
//! that timestamp in milliseconds each pre-update, and
//! [`FrameRateMonitor::visibility_changed`] on visibility changes. Events are
//! queued and popped instead of raised; each carries the frames per second.
//! Periods are in seconds, as in Cesium.

use crate::core::time_constants::SECONDS_PER_MILLISECOND;

/// Configuration options for [`FrameRateMonitor`].
#[derive(Clone, Debug, PartialEq)]
pub struct FrameRateMonitorOptions {
    /// Sliding window duration over which to compute average FPS, in seconds.
    pub sampling_window: f64,
    /// Delay before measuring performance at startup and after resume, in seconds.
    pub quiet_period: f64,
    /// Warmup period duration during which lower frame rates are acceptable, in seconds.
    pub warmup_period: f64,
    /// Minimum frames-per-second required during warmup.
    pub minimum_frame_rate_during_warmup: f64,
    /// Minimum frames-per-second required after warmup.
    pub minimum_frame_rate_after_warmup: f64,
}

impl Default for FrameRateMonitorOptions {
    fn default() -> Self {
        Self {
            sampling_window: 5.0,
            quiet_period: 2.0,
            warmup_period: 5.0,
            minimum_frame_rate_during_warmup: 4.0,
            minimum_frame_rate_after_warmup: 8.0,
        }
    }
}

/// Tracks frame timestamps across a sliding window and raises performance alerts.
pub struct FrameRateMonitor {
    pub sampling_window: f64,
    pub quiet_period: f64,
    pub warmup_period: f64,
    pub minimum_frame_rate_during_warmup: f64,
    pub minimum_frame_rate_after_warmup: f64,

    pause_count: usize,
    needs_quiet_period: bool,
    quiet_period_end_time: f64,
    warmup_period_end_time: f64,
    frame_times: Vec<f64>,
    last_frames_per_second: Option<f64>,
    frame_rate_is_low: bool,
    low_frame_rate_events: Vec<f64>,
    nominal_frame_rate_events: Vec<f64>,
}

impl Default for FrameRateMonitor {
    fn default() -> Self {
        Self::new(FrameRateMonitorOptions::default())
    }
}

impl FrameRateMonitor {
    /// Creates a new `FrameRateMonitor` with the given configuration options.
    pub fn new(options: FrameRateMonitorOptions) -> Self {
        Self {
            sampling_window: options.sampling_window,
            quiet_period: options.quiet_period,
            warmup_period: options.warmup_period,
            minimum_frame_rate_during_warmup: options.minimum_frame_rate_during_warmup,
            minimum_frame_rate_after_warmup: options.minimum_frame_rate_after_warmup,
            pause_count: 0,
            needs_quiet_period: true,
            quiet_period_end_time: 0.0,
            warmup_period_end_time: 0.0,
            frame_times: Vec::new(),
            last_frames_per_second: None,
            frame_rate_is_low: false,
            low_frame_rate_events: Vec::new(),
            nominal_frame_rate_events: Vec::new(),
        }
    }

    /// Pauses frame rate monitoring.
    pub fn pause(&mut self) {
        self.pause_count += 1;
        if self.pause_count == 1 {
            self.frame_times.clear();
            self.last_frames_per_second = None;
        }
    }

    /// Resumes frame rate monitoring. Unbalanced calls clamp at zero and,
    /// as in Cesium, still restart the quiet period.
    pub fn unpause(&mut self) {
        self.pause_count = self.pause_count.saturating_sub(1);
        if self.pause_count == 0 {
            self.needs_quiet_period = true;
        }
    }

    /// Cesium's `visibilitychange` handler: pause while the document is
    /// hidden, unpause when it is shown.
    pub fn visibility_changed(&mut self, hidden: bool) {
        if hidden {
            self.pause();
        } else {
            self.unpause();
        }
    }

    /// Returns whether monitoring is currently paused.
    pub fn is_paused(&self) -> bool {
        self.pause_count > 0
    }

    /// Returns the most recently computed frames per second.
    pub fn last_frames_per_second(&self) -> Option<f64> {
        self.last_frames_per_second
    }

    /// Returns whether the monitor currently considers the frame rate low.
    pub fn is_frame_rate_low(&self) -> bool {
        self.frame_rate_is_low
    }

    /// Cesium's pre-update step at `getTimestamp()` milliseconds.
    pub fn update(&mut self, timestamp_ms: f64) {
        if self.pause_count > 0 {
            return;
        }

        if self.needs_quiet_period {
            self.needs_quiet_period = false;
            self.frame_times.clear();
            self.quiet_period_end_time = timestamp_ms + self.quiet_period / SECONDS_PER_MILLISECOND;
            self.warmup_period_end_time =
                self.quiet_period_end_time + (self.warmup_period + self.sampling_window) / SECONDS_PER_MILLISECOND;
        } else if timestamp_ms >= self.quiet_period_end_time {
            self.frame_times.push(timestamp_ms);

            let beginning_of_window = timestamp_ms - self.sampling_window / SECONDS_PER_MILLISECOND;

            if self.frame_times.len() >= 2 && self.frame_times[0] <= beginning_of_window {
                let mut first = 0;
                while self.frame_times.len() - first >= 2 && self.frame_times[first + 1] < beginning_of_window {
                    first += 1;
                }
                self.frame_times.drain(..first);

                let average_time_between_frames =
                    (timestamp_ms - self.frame_times[0]) / (self.frame_times.len() - 1) as f64;
                let fps = 1000.0 / average_time_between_frames;
                self.last_frames_per_second = Some(fps);

                let maximum_frame_time = 1000.0
                    / if timestamp_ms > self.warmup_period_end_time {
                        self.minimum_frame_rate_after_warmup
                    } else {
                        self.minimum_frame_rate_during_warmup
                    };

                if average_time_between_frames > maximum_frame_time {
                    if !self.frame_rate_is_low {
                        self.frame_rate_is_low = true;
                        self.needs_quiet_period = true;
                        self.low_frame_rate_events.push(fps);
                    }
                } else if self.frame_rate_is_low {
                    self.frame_rate_is_low = false;
                    self.needs_quiet_period = true;
                    self.nominal_frame_rate_events.push(fps);
                }
            }
        }
    }

    /// Pops the next low frame rate event (average FPS), if any was raised.
    pub fn pop_low_frame_rate_event(&mut self) -> Option<f64> {
        if self.low_frame_rate_events.is_empty() {
            None
        } else {
            Some(self.low_frame_rate_events.remove(0))
        }
    }

    /// Pops the next nominal frame rate event (average FPS), if any was raised.
    pub fn pop_nominal_frame_rate_event(&mut self) -> Option<f64> {
        if self.nominal_frame_rate_events.is_empty() {
            None
        } else {
            Some(self.nominal_frame_rate_events.remove(0))
        }
    }
}
