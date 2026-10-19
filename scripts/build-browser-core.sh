#!/usr/bin/env bash
# Build optional wasm-bindgen browser adapters and the accompanying JS modules.
# The raw C-ABI viewer is built separately by build-web.sh.
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"
if ! command -v wasm-bindgen >/dev/null; then
  echo 'Install the CLI matching Cargo.lock: cargo install wasm-bindgen-cli --version 0.2.129 --locked' >&2
  exit 1
fi
cargo build --release --target wasm32-unknown-unknown --features browser
out="${1:-target/browser-core}"
wasm-bindgen --target web --out-dir "$out" --out-name cesium_rs target/wasm32-unknown-unknown/release/cesium_rs.wasm

cp LICENSE "$out/LICENSE"
cp NOTICE "$out/NOTICE"
cp web/host/vendor/DOMPurify-LICENSE "$out/DOMPurify-LICENSE"
