//! CesiumJS `FrameRateMonitor`: monitors average frames per second over a
//! sliding window and triggers events when the frame rate drops below or recovers
//! to acceptable performance thresholds.

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

    /// Resumes frame rate monitoring.
    pub fn unpause(&mut self) {
        if self.pause_count > 0 {
            self.pause_count -= 1;
            if self.pause_count == 0 {
                self.needs_quiet_period = true;
            }
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

    /// Records a frame at the specified monotonic timestamp (in seconds).
    pub fn update(&mut self, timestamp: f64) {
        if self.pause_count > 0 {
            return;
        }

        if self.needs_quiet_period {
            self.needs_quiet_period = false;
            self.frame_times.clear();
            self.quiet_period_end_time = timestamp + self.quiet_period;
            self.warmup_period_end_time =
                self.quiet_period_end_time + self.warmup_period + self.sampling_window;
        } else if timestamp >= self.quiet_period_end_time {
            self.frame_times.push(timestamp);

            let beginning_of_window = timestamp - self.sampling_window;

            if self.frame_times.len() >= 2 && self.frame_times[0] <= beginning_of_window {
                while self.frame_times.len() >= 2 && self.frame_times[1] < beginning_of_window {
                    self.frame_times.remove(0);
                }

                let count = self.frame_times.len();
                let average_delta = (timestamp - self.frame_times[0]) / ((count - 1) as f64);
                let fps = if average_delta > 0.0 {
                    1.0 / average_delta
                } else {
                    0.0
                };
                self.last_frames_per_second = Some(fps);

                let required_fps = if timestamp > self.warmup_period_end_time {
                    self.minimum_frame_rate_after_warmup
                } else {
                    self.minimum_frame_rate_during_warmup
                };

                let max_frame_delta = 1.0 / required_fps;

                if average_delta > max_frame_delta {
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
