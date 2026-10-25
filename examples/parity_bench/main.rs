//! CesiumRS side of the CesiumJS parity benchmark (native).
//!
//! Times the workloads in `workloads.rs` on the same inputs that
//! `tools/cesium/bench/node.mjs` feeds CesiumJS, and writes one JSON result
//! file that `tools/cesium/bench/compare.mjs` reads. Run everything with
//! `scripts/bench.sh`, or this side alone with:
//!
//! ```text
//! node tools/cesium/bench/inputs.mjs          # generated inputs, once
//! cargo run --profile bench --example parity_bench -- --out target/bench-results/rust-native.json
//! ```
//!
//! Options: `--quick` (fewer, shorter samples), `--filter <substring>`,
//! `--setup <name>`, `--out <path>`. Use the bench profile: the release
//! profile is size-tuned for the wasm viewer and understates native speed.

#[path = "../../tests/common/mod.rs"]
mod common;
mod harness;
mod workloads;

use harness::{Clock, Settings};
use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::time::Instant;

struct NativeClock(Instant);

impl Clock for NativeClock {
    fn now_ms(&self) -> f64 {
        self.0.elapsed().as_secs_f64() * 1e3
    }
}

struct Disk(PathBuf);

impl workloads::Source for Disk {
    fn text(&self, path: &str) -> Result<String, String> {
        std::fs::read_to_string(self.0.join(path)).map_err(|e| format!("{path}: {e}"))
    }
}

fn main() {
    let mut settings = Settings::FULL;
    let mut filter = String::new();
    let mut setup = "cesiumrs-native".to_string();
    let mut out: Option<PathBuf> = None;
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--quick" => settings = Settings::QUICK,
            "--filter" => filter = args.next().expect("--filter needs a value"),
            "--setup" => setup = args.next().expect("--setup needs a value"),
            "--out" => out = Some(PathBuf::from(args.next().expect("--out needs a value"))),
            other => panic!("unknown argument {other}"),
        }
    }
    if cfg!(debug_assertions) {
        eprintln!("warning: debug build; use `cargo run --profile bench --example parity_bench`");
    }

    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let source = Disk(root.to_path_buf());
    let clock = NativeClock(Instant::now());
    let mut results = Vec::new();
    for w in workloads::all().into_iter().filter(|w| w.id.contains(filter.as_str())) {
        let mut entry = json!({ "id": w.id, "group": w.group, "title": w.title, "input": w.input });
        match (w.prepare)(&source) {
            Ok(mut prepared) => {
                let measured = harness::measure(&mut prepared, settings, &clock);
                eprintln!(
                    "{:<34} {:>12.4} ms/pass  {:>4} cases  {}",
                    w.id,
                    measured["medianMs"].as_f64().unwrap_or(f64::NAN),
                    prepared.cases,
                    measured["status"].as_str().unwrap_or("")
                );
                merge(&mut entry, measured);
            }
            Err(e) => {
                eprintln!("{:<34} error: {e}", w.id);
                merge(&mut entry, json!({ "status": "error", "error": e }));
            }
        }
        results.push(entry);
    }

    let report = json!({
        "schema": 1,
        "setup": setup,
        "implementation": "cesiumrs",
        "runtime": format!("native, {}", std::env::var("BENCH_RUSTC").unwrap_or_else(|_| "rustc (version not recorded)".into())),
        "host": { "os": std::env::consts::OS, "arch": std::env::consts::ARCH,
                  "cpus": std::thread::available_parallelism().map_or(0, |n| n.get()) },
        "debugBuild": cfg!(debug_assertions),
        "settings": settings.to_json(),
        "unixMs": std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_millis() as u64),
        "results": results,
    });
    let text = serde_json::to_string_pretty(&report).expect("report JSON");
    match out {
        Some(path) => {
            if let Some(dir) = path.parent() {
                std::fs::create_dir_all(dir).expect("create output directory");
            }
            std::fs::write(&path, text).expect("write report");
            eprintln!("wrote {}", path.display());
        }
        None => println!("{text}"),
    }
}

fn merge(entry: &mut Value, extra: Value) {
    if let (Value::Object(e), Value::Object(x)) = (entry, extra) {
        e.extend(x);
    }
}
