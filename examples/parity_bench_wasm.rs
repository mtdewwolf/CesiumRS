//! CesiumRS side of the CesiumJS parity benchmark, as a raw wasm module for
//! the browser. `tools/cesium/bench/wasm-host.mjs` drives it:
//!
//! 1. `bench_inputs()` returns the JSON list of input files the workloads read;
//! 2. the host fetches them and writes `{ settings, filter, files }` as JSON
//!    into a buffer from `bench_alloc(len)`;
//! 3. `bench_run(ptr, len)` takes that buffer, runs the workloads from
//!    `parity_bench/workloads.rs` and returns the result JSON.
//!
//! Every returned string stays valid until the next call; `bench_output_len()`
//! gives its length. The clock is the host's `performance.now()`, imported as
//! `env.bench_now`. No wasm-bindgen: build with
//! `cargo build --release --target wasm32-unknown-unknown --example parity_bench_wasm`.

#[path = "../tests/common/mod.rs"]
mod common;
#[path = "parity_bench/harness.rs"]
mod harness;
#[path = "parity_bench/workloads.rs"]
mod workloads;

use harness::{Clock, Settings};
use serde_json::{json, Map, Value};
use std::cell::RefCell;

#[cfg(target_arch = "wasm32")]
mod host {
    extern "C" {
        pub fn bench_now() -> f64;
    }
}

struct HostClock;

impl Clock for HostClock {
    #[cfg(target_arch = "wasm32")]
    fn now_ms(&self) -> f64 {
        // SAFETY: the host supplies `env.bench_now`, a plain () -> f64 function.
        unsafe { host::bench_now() }
    }

    // Native builds of this example (clippy --all-targets) use a monotonic clock.
    #[cfg(not(target_arch = "wasm32"))]
    fn now_ms(&self) -> f64 {
        use std::sync::OnceLock;
        use std::time::Instant;
        static START: OnceLock<Instant> = OnceLock::new();
        START.get_or_init(Instant::now).elapsed().as_secs_f64() * 1e3
    }
}

struct Files(Map<String, Value>);

impl workloads::Source for Files {
    fn text(&self, path: &str) -> Result<String, String> {
        self.0.get(path).and_then(Value::as_str).map(str::to_string).ok_or_else(|| format!("{path}: not supplied"))
    }
}

thread_local! {
    static OUTPUT: RefCell<Vec<u8>> = const { RefCell::new(Vec::new()) };
}

fn publish(value: &Value) -> *const u8 {
    OUTPUT.with(|out| {
        let mut out = out.borrow_mut();
        *out = serde_json::to_vec(value).unwrap_or_default();
        out.as_ptr()
    })
}

/// Length of the string returned by the last `bench_inputs` or `bench_run`.
#[no_mangle]
pub extern "C" fn bench_output_len() -> usize {
    OUTPUT.with(|out| out.borrow().len())
}

/// A buffer of `len` bytes for the request; `bench_run` takes ownership of it.
#[no_mangle]
pub extern "C" fn bench_alloc(len: usize) -> *mut u8 {
    let mut buffer = vec![0u8; len];
    let ptr = buffer.as_mut_ptr();
    std::mem::forget(buffer);
    ptr
}

/// JSON array of the repository-relative files the workloads read.
#[no_mangle]
pub extern "C" fn bench_inputs() -> *const u8 {
    let mut inputs: Vec<&str> = workloads::all().iter().map(|w| w.input).collect();
    inputs.sort_unstable();
    inputs.dedup();
    publish(&json!(inputs))
}

/// Runs the workloads described by the request in `bench_alloc`'s buffer.
///
/// # Safety
/// `ptr` and `len` must come from one `bench_alloc(len)` call, and the buffer
/// must not be used after this call.
#[no_mangle]
pub unsafe extern "C" fn bench_run(ptr: *mut u8, len: usize) -> *const u8 {
    // SAFETY: the caller passes back a buffer from bench_alloc(len).
    let request = unsafe { Vec::from_raw_parts(ptr, len, len) };
    let request: Value = match serde_json::from_slice(&request) {
        Ok(v) => v,
        Err(e) => return publish(&json!({ "error": format!("request JSON: {e}") })),
    };
    let s = &request["settings"];
    let settings = Settings {
        samples: s["samples"].as_u64().unwrap_or(Settings::FULL.samples as u64) as usize,
        sample_ms: s["sampleMs"].as_f64().unwrap_or(Settings::FULL.sample_ms),
        warmup_ms: s["warmupMs"].as_f64().unwrap_or(Settings::FULL.warmup_ms),
    };
    let filter = request["filter"].as_str().unwrap_or("").to_string();
    let source = Files(request["files"].as_object().cloned().unwrap_or_default());

    let mut results = Vec::new();
    for w in workloads::all().into_iter().filter(|w| w.id.contains(filter.as_str())) {
        let mut entry = json!({ "id": w.id, "group": w.group, "title": w.title, "input": w.input });
        let measured = match (w.prepare)(&source) {
            Ok(mut prepared) => harness::measure(&mut prepared, settings, &HostClock),
            Err(e) => json!({ "status": "error", "error": e }),
        };
        if let (Value::Object(e), Value::Object(m)) = (&mut entry, measured) {
            e.extend(m);
        }
        results.push(entry);
    }
    publish(&json!({ "settings": settings.to_json(), "results": results }))
}
