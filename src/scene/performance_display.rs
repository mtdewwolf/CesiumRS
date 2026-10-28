//! CPU sampling logic of CesiumJS `PerformanceDisplay`. The host supplies monotonic
//! milliseconds and presents the text; no DOM elements are created by this module.
#[derive(Debug)]
pub struct PerformanceDisplay {
    last_fps: f64,
    last_ms: f64,
    fps_frames: u32,
    ms_frames: u32,
    pub fps_text: String,
    pub ms_text: String,
    throttled: bool,
}
impl PerformanceDisplay {
    pub fn new(timestamp_ms: f64) -> Self {
        Self {
            last_fps: timestamp_ms,
            last_ms: timestamp_ms,
            fps_frames: 0,
            ms_frames: 0,
            fps_text: String::new(),
            ms_text: String::new(),
            throttled: false,
        }
    }
    pub fn throttled(&self) -> bool {
        self.throttled
    }
    pub fn set_throttled(&mut self, value: bool) {
        self.throttled = value;
    }
    pub fn throttled_text(&self) -> &str {
        if self.throttled {
            "(throttled)"
        } else {
            ""
        }
    }
    pub fn update(&mut self, timestamp_ms: f64, rendered: bool) {
        self.fps_frames += 1;
        let elapsed = timestamp_ms - self.last_fps;
        if elapsed > 1000.0 {
            self.fps_text = if rendered {
                format!("{} FPS", (self.fps_frames as f64 * 1000.0 / elapsed) as i32)
            } else {
                "N/A FPS".into()
            };
            self.last_fps = timestamp_ms;
            self.fps_frames = 0;
        }
        self.ms_frames += 1;
        let elapsed = timestamp_ms - self.last_ms;
        if elapsed > 200.0 {
            self.ms_text =
                if rendered { format!("{:.2} MS", elapsed / self.ms_frames as f64) } else { "N/A MS".into() };
            self.last_ms = timestamp_ms;
            self.ms_frames = 0;
        }
    }
}
