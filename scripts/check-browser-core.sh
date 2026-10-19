#!/usr/bin/env bash
# Rebuild both the shipped browser API and the separate lifecycle test artifact.
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"
bash scripts/build-browser-core.sh
cargo build --release --target wasm32-unknown-unknown --features browser --example browser_validation
wasm-bindgen --target web --out-dir target/browser-validation --out-name validation target/wasm32-unknown-unknown/release/examples/browser_validation.wasm
node tools/cesium/browser-check.mjs
