# Core validation

All **270 applicable Core modules** are implemented and validated against
`@cesium/engine` 23.0.0 (CesiumJS 1.139). The 18 n/a entries describe JavaScript
idioms replaced by Rust ownership, traits or explicit host configuration.

The final 20 modules add **199 reference cases**: platform/Ion requests, terrain
metadata and binary tiles, GEE packets/meshes, meshopt v0/v1 glTF terrain, implicit
subtree metadata and ground polyline volumes. The complete fixture corpus has
**25,832 cases**. The full Rust suite passes **217 tests**; Chromium passes
**63 runtime checks**. These checks establish the tracked Core milestone, not
completion of Renderer, Scene or the rest of Cesium.

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

Core is **100% implemented and validated** by applicable source lines. Overall
Cesium coverage is **26.1%**; Renderer and Scene remain separate milestones.

Final-module checks additionally cover:

- Ion endpoint refresh failures, external assets and domain-scoped credentials;
  geocoder provider transitions, iTwin request headers and signed URL paths.
- ArcGIS/VR metadata and tile URLs, Cesium binary heightmaps/quantized meshes,
  normals/water extensions, attribution and world factory asset IDs/defaults.
- GEE dbRoot protobuf fields, quadtree packets, quadkeys, negative elevations,
  complete mesh/skirt/upsample buffers and parent/child terrain cache fan-out.
- 3D Tiles GLB and meshopt v0/v1 attributes/indices, filled-corner skirts,
  complete child clipping buffers, implicit subtrees, metadata semantics and
  cached availability queries.
- Ground polyline packing and complete geometry attributes for geodesic/rhumb
  paths, loops, duplicate points, antimeridian splits and both projections.
- Exact browser text metrics/pixels and pin pixels/cache identity; mouse,
  wheel, modifier priority, pointer/touch, pinch, long press and destruction.

Reference comparisons fixed operation-order differences at GEE mesh boundaries
and a signed-zero antimeridian split that changed the terrain-height lookup.
Native terrain tasks calculate synchronously behind futures. Canvas fonts,
image rasterization and DOM delivery use explicit host capabilities; network
checks use deterministic local/mock responses rather than live service tokens.
