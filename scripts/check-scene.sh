#!/usr/bin/env bash
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"
cargo build --release --target wasm32-unknown-unknown --features browser --example renderer_validation
wasm-bindgen --target web --out-dir target/renderer-validation --out-name renderer target/wasm32-unknown-unknown/release/examples/renderer_validation.wasm
node tools/cesium/scene-runtime-check.mjs
