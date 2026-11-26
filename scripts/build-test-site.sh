#!/usr/bin/env bash
# Build the CesiumRS live feature testbed WASM bundle into test-site/pkg/.
# Pass --dev for fast incremental compilation, or omit for size-optimized release.
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"

if ! command -v wasm-bindgen >/dev/null; then
  echo 'Install the CLI matching Cargo.lock: cargo install wasm-bindgen-cli --version 0.2.129 --locked' >&2
  exit 1
fi

profile_flag="--release"
profile_dir="release"
if [[ "${1:-}" == "--dev" ]]; then
  profile_flag=""
  profile_dir="debug"
fi

echo "Building live_testbed ($profile_dir) for wasm32-unknown-unknown..."
cargo build $profile_flag --target wasm32-unknown-unknown --features browser --example live_testbed

wasm="target/wasm32-unknown-unknown/${profile_dir}/examples/live_testbed.wasm"
out="test-site/pkg"
mkdir -p "$out"

echo "Running wasm-bindgen -> $out ..."
wasm-bindgen --target web --out-dir "$out" --out-name cesium_rs_testbed "$wasm"

echo "Built $out ($(stat -c%s "$out/cesium_rs_testbed_bg.wasm") bytes wasm)"
echo "Run: node scripts/serve-test-site.mjs"
