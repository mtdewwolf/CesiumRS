#!/usr/bin/env bash
# Build the CesiumRS browser Scene WASM package (`target/browser-scene` by default),
# emitting an ESM package (package.json, cesium_rs.js, cesium_rs_bg.wasm, cesium_rs.d.ts)
# that Vite / gods-eye-view can import directly.
set -euo pipefail
root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$root"

if ! command -v wasm-bindgen >/dev/null; then
  echo 'Install the CLI matching Cargo.lock: cargo install wasm-bindgen-cli --version 0.2.129 --locked' >&2
  exit 1
fi

profile="${CESIUMRS_PROFILE:-release}"
if [ "$profile" = "debug" ]; then
  cargo build --target wasm32-unknown-unknown --features browser
  wasm="target/wasm32-unknown-unknown/debug/cesium_rs.wasm"
else
  cargo build --release --target wasm32-unknown-unknown --features browser
  wasm="target/wasm32-unknown-unknown/release/cesium_rs.wasm"
fi

out="${1:-target/browser-scene}"
mkdir -p "$out"
wasm-bindgen --target web --out-dir "$out" --out-name cesium_rs "$wasm"

cp LICENSE "$out/LICENSE"
cp NOTICE "$out/NOTICE"
cp web/host/vendor/DOMPurify-LICENSE "$out/DOMPurify-LICENSE"
cp assets/Meshoptimizer-LICENSE.md "$out/Meshoptimizer-LICENSE.md"

cat > "$out/package.json" <<'EOF'
{
  "name": "cesium-rs-browser",
  "version": "0.1.0",
  "private": true,
  "type": "module",
  "main": "./cesium_rs.js",
  "module": "./cesium_rs.js",
  "types": "./cesium_rs.d.ts",
  "exports": {
    ".": {
      "types": "./cesium_rs.d.ts",
      "import": "./cesium_rs.js"
    },
    "./cesium_rs_bg.wasm": "./cesium_rs_bg.wasm"
  },
  "sideEffects": false
}
EOF
