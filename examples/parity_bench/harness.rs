//! Timing loop and result format shared by the native and wasm runners.
//!
//! `tools/cesium/bench/harness.mjs` implements the same algorithm for
//! CesiumJS, so the numbers from both sides are comparable:
//!
//! 1. time the first pass over the workload's inputs (cold);
//! 2. check the outputs against the CesiumJS fixture results;
//! 3. calibrate how many passes make one sample last `sample_ms`;
//! 4. warm up for `warmup_ms`;
//! 5. take `samples` samples and report the time per pass.

use serde_json::{json, Value};
use std::hint::black_box;

/// Milliseconds from an arbitrary origin.
pub trait Clock {
    fn now_ms(&self) -> f64;
}

#[derive(Clone, Copy, Debug)]
pub struct Settings {
    pub samples: usize,
    pub sample_ms: f64,
    pub warmup_ms: f64,
}

impl Settings {
    pub const FULL: Settings = Settings { samples: 15, sample_ms: 25.0, warmup_ms: 300.0 };
    /// `--quick`; the wasm runner takes its settings from the host instead.
    #[allow(dead_code)]
    pub const QUICK: Settings = Settings { samples: 5, sample_ms: 5.0, warmup_ms: 50.0 };

    pub fn to_json(self) -> Value {
        json!({ "samples": self.samples, "sampleMs": self.sample_ms, "warmupMs": self.warmup_ms })
    }
}

/// Result of checking a workload's outputs.
pub struct Verify {
    /// Cases compared with a CesiumJS fixture result. Zero for generated
    /// inputs, which are checked by checksum against the CesiumJS run.
    pub checked: usize,
    pub mismatches: Vec<String>,
}

/// A workload with its inputs decoded and ready to run.
pub struct Prepared {
    /// Inputs processed by one pass.
    pub cases: usize,
    /// One pass over every input. Returns a checksum of the outputs; the
    /// CesiumJS side computes the same checksum in the same order.
    pub run: Box<dyn FnMut() -> f64>,
    pub verify: Box<dyn FnMut() -> Verify>,
}

pub fn measure(prepared: &mut Prepared, settings: Settings, clock: &dyn Clock) -> Value {
    let run = &mut prepared.run;

    let start = clock.now_ms();
    let checksum = black_box(run());
    let first_ms = clock.now_ms() - start;

    let verify = (prepared.verify)();

    // Calibrate: double the passes per sample until one sample lasts long enough.
    let mut sink = 0.0;
    let mut passes: u64 = 1;
    loop {
        let start = clock.now_ms();
        for _ in 0..passes {
            sink += black_box(run());
        }
        let elapsed = clock.now_ms() - start;
        if elapsed >= settings.sample_ms || passes >= 1 << 24 {
            break;
        }
        passes *= 2;
    }

    let warm_start = clock.now_ms();
    while clock.now_ms() - warm_start < settings.warmup_ms {
        sink += black_box(run());
    }

    let mut samples = Vec::with_capacity(settings.samples);
    for _ in 0..settings.samples {
        let start = clock.now_ms();
        for _ in 0..passes {
            sink += black_box(run());
        }
        samples.push((clock.now_ms() - start) / passes as f64);
    }
    black_box(sink);

    let status = if !verify.mismatches.is_empty() {
        "mismatch"
    } else if verify.checked > 0 {
        "verified"
    } else {
        "checksum-only"
    };
    let mut sorted = samples.clone();
    sorted.sort_by(f64::total_cmp);
    json!({
        "status": status,
        "cases": prepared.cases,
        "checked": verify.checked,
        "mismatchCount": verify.mismatches.len(),
        "mismatches": verify.mismatches.iter().take(5).collect::<Vec<_>>(),
        "firstMs": first_ms,
        "passesPerSample": passes,
        "samplesMs": samples,
        "medianMs": percentile(&sorted, 0.5),
        "p95Ms": percentile(&sorted, 0.95),
        "minMs": sorted.first().copied().unwrap_or(f64::NAN),
        "checksum": checksum,
    })
}

/// Nearest-rank percentile of sorted samples (the same rule as harness.mjs).
fn percentile(sorted: &[f64], p: f64) -> f64 {
    if sorted.is_empty() {
        return f64::NAN;
    }
    let rank = (p * sorted.len() as f64).ceil() as usize;
    sorted[rank.clamp(1, sorted.len()) - 1]
}
