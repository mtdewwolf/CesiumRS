# Core foundations validation

The 35 formerly partial Core modules and `Workers/transcodeKTX2` were validated
against `@cesium/engine` 23.0.0 (CesiumJS 1.139). The batch adds **617 reference
cases** across 24 fixture files. The full Rust suite passes **197 tests**, and
Chromium passes **23 runtime checks**.

Reference fixtures cover resource URL/query inheritance, proxies, trusted
servers, credits, tile information, texture packing, multi-level R-tree
insertion/removal, vector and GEE packet decoding, geocoder requests/responses,
request scheduling, packing/factory contracts, video ticks and terrain sampling.
Terrain cases include throttling, dynamically discovered availability, grouped
points and rejection with concurrent successful tiles/levels. Existing quaternion
and polynomial fixtures now execute through the new interpolation traits.

The 96 KTX2 cases compare exact bytes from seven pinned real textures plus
synthetic alpha slices, cubemaps/mipmaps and byte/half/float formats. They exercise ETC1,
ETC2, S3TC, PVRTC, ASTC and BC7 target selection, unsupported target errors,
ETC1S/UASTC decoding and Zstd input. Truncated files are rejected separately.
Texture provenance is recorded in `tests/data/ktx2/README.md`.

Native integration checks use actual localhost HTTP and worker threads. Controlled
pending futures verify concurrency limits, cancellation, dropped tasks, processor
destruction, shared initialization and wake delivery to multiple waiters. Resource
checks cover retry slot release, credentials, HTTP errors/no-content, XML, charset
handling and JSONP parsing. Image contracts check backing-buffer views and options.

Chromium loads the shipped wasm-bindgen package and compares image/canvas and
sanitized credit results with CesiumJS. It exercises HTTP, image loading, JSONP
cleanup, cancellation and module workers. A separate WASM example checks Rust
worker initialization/capacity/errors/destruction and video clock/seek events,
element replacement and subscription cleanup. Large ETC1S ASTC/BC7 output is
compared byte-for-byte in WASM as well as on native Rust.

Validation fixed:

- GEE packets under eight bytes must repeatedly XOR the first key byte.
- ETC1S output must preserve the reference flip bit; PVRTC buffers need their
  minimum allocation footprint, including zero padding for small mipmaps.
- Cesium's Basis build disables newer BC7 chroma filtering and ASTC higher opaque
  quality modes. The pinned Rust codec now uses matching flags and a legacy ASTC
  CEM12 compatibility path.
- Terrain rejection must preserve results from other ready tile and level jobs;
  all ready jobs are polled before returning the first error.
- Browser operations register Promise rejection handlers immediately and install
  image cleanup guards before returning futures. Dropping or destroying an
  unpolled task/HTTP/image future no longer causes unhandled browser rejections.

Run from the repository root:

```bash
. "$HOME/.cargo/env"
cargo test --all-features
(cd tools/cesium && npm ci && npm run fixtures)
bash scripts/check-browser-core.sh
```

The browser check uses `/usr/bin/chromium` by default; set `CHROMIUM_PATH` to
another installed Chromium executable. `wasm-bindgen-cli` must match Cargo.lock
(currently 0.2.129). The separate validation example does not add test exports to
the shipped library.

Host adaptations remain explicit: HTTP/media/canvas behavior belongs to the host,
missing sampled heights are NaN, and Rust response/task futures have one consumer.
Terrain borrows positions for the lifetime of its future; returning an error drops
still-pending Rust jobs, while preserving already-ready results. Runtime checks
use local deterministic responses; they do not call paid geocoding services.

Core remains at **87.2%** by applicable source lines, now validated as well as
implemented. Twenty Core modules remain todo; this batch does not claim 100% Core.
