#!/usr/bin/env bash
set -euo pipefail
cd "$(dirname "$0")/.."
cargo build --release --target wasm32-unknown-unknown --features browser --example renderer_completion_validation
wasm-bindgen --target web --out-dir target/renderer-completion --out-name completion target/wasm32-unknown-unknown/release/examples/renderer_completion_validation.wasm
node tools/cesium/renderer-completion-check.mjs
