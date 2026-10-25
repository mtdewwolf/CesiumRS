#!/usr/bin/env bash
# CesiumJS vs CesiumRS parity benchmark: runs every available setup on the
# same inputs, then writes target/bench-results/report.md.
#
#   scripts/bench.sh [--quick] [--filter <workload id substring>]
#
# Setups: CesiumRS native (bench profile) and CesiumJS in Node always; CesiumRS
# wasm (release and bench profiles) when the wasm32-unknown-unknown target is
# installed; the browser setups when Chromium is found ($CHROMIUM_PATH, else
# /usr/bin/chromium). See docs/benchmarks.md.
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"
args=("$@")

[ -d tools/cesium/node_modules ] || (cd tools/cesium && npm ci)
node tools/cesium/bench/inputs.mjs

# Stale files from an earlier run would be compared with this one.
mkdir -p target/bench-results target/bench-wasm
rm -f target/bench-results/*.json target/bench-wasm/*.wasm

BENCH_RUSTC="$(rustc --version)" cargo run --locked --quiet --profile bench --example parity_bench -- \
  --out target/bench-results/cesiumrs-native.json ${args[@]+"${args[@]}"}
node tools/cesium/bench/node.mjs --out target/bench-results/cesiumjs-node.json ${args[@]+"${args[@]}"}

if rustup target list --installed 2>/dev/null | grep -qx wasm32-unknown-unknown; then
  # The bench profile shares target/.../release with the release profile, so copy each build out.
  cargo build --locked --release --target wasm32-unknown-unknown --example parity_bench_wasm
  cp target/wasm32-unknown-unknown/release/examples/parity_bench_wasm.wasm target/bench-wasm/cesiumrs-wasm-shipped.wasm
  cargo build --locked --profile bench --target wasm32-unknown-unknown --example parity_bench_wasm
  cp target/wasm32-unknown-unknown/release/examples/parity_bench_wasm.wasm target/bench-wasm/cesiumrs-wasm-fast.wasm
else
  echo "wasm32-unknown-unknown is not installed; skipping the CesiumRS wasm setups" >&2
  echo "  (rustup target add wasm32-unknown-unknown)" >&2
fi

chromium="${CHROMIUM_PATH:-/usr/bin/chromium}"
if [ -x "$chromium" ]; then
  CHROMIUM_PATH="$chromium" node tools/cesium/bench/browser.mjs ${args[@]+"${args[@]}"}
else
  echo "Chromium not found at $chromium; skipping the browser setups (set CHROMIUM_PATH)" >&2
fi

node tools/cesium/bench/compare.mjs
