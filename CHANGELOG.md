# Changelog

## Unreleased

- Port `Particle`, the `ParticleEmitter` trait and the box, circle, cone and sphere emitters (seeded emission matches CesiumJS bit for bit), plus `PointCloudShading`, `ImageryLayerFeatureInfo`, `KeyframeNode`, `ImageryConfiguration` and `DiscardEmptyTileImagePolicy`. 65 pinned CesiumJS cases.

- Fix reentrant DataSources callbacks: preserve renewed subscriptions and evaluate PropertyArray children from one definition. Add regressions for renewal/cancellation, stale handles, duplicate registration counts and array replacement; document the native lifecycle contracts.

- Start DataSources with seven property modules: typed constant/callback/array properties, sampled interpolation and derivatives, raw interval values and composites. Validate 206 pinned CesiumJS scenarios, native callback/ownership lifecycle and browser default clocks. Entity and document loaders remain unported.

- Port the rest of 3D Metadata and four 3D Tiles helpers. The metadata modules are `PropertyTextureProperty`, `PropertyTexture`, `StructuralMetadata`, `getMetadataProperty` and the three parsers: `parseStructuralMetadata` (including the RGBA8 property table texture), `parseFeatureMetadataLegacy` and `parseBatchTable`. The helpers are `preprocess3DTileContent`, `TileReplacementQueue` and `Cesium3DContentGroup`. `PropertyTable` now carries its texture and accepts legacy string ids (`MetadataId`). 94 pinned CesiumJS cases.

- Port the `NGA_gpm_local` extension: `GltfGpmLoader` and nine data classes (anchor points, correlation groups, `Spdcf`, `GltfGpmLocal`, per-point error textures and metadata), with 35 pinned CesiumJS cases.

- Port the 3D Tiles binary content parsers: `Cesium3DTileFeatureTable`, `B3dmParser` (current and both legacy headers), `I3dmParser` and `PntsParser` (quantized positions, all color and normal encodings, binary globals, Draco point compression). 59 pinned CesiumJS tiles cover JavaScript's typed-array errors, unaligned glb copies and every parser error.

- Port glTF image and texture loaders with 119 pinned cases and 128 additional WebGL1/2 comparisons. Add cached image decoding, KTX2/Basis mip chains, deferred texture uploads, NPOT resizing, cancellation and GPU resource accounting. Extend upload budgets to separate geometry and texture work; ResourceCache and vertex loading remain partial pending SPZ.

- Integrate ModelComponents and initialized Draco workers with the shared model cache and GPU loaders. Preserve both parity suites (293 additional cases), unify cache key/texture helpers, and retain lease cancellation and renderer ownership across the overlapping Scene batches.

- Add shared model resource caching, glTF texture utilities and frame-driven Draco/vertex/index loading, with 184 new CesiumJS cases and 24 WebGL comparisons. Cache and vertex loading remain partial pending image/texture/SPZ paths; the main GltfLoader/Model pipeline remains unported. Correct JavaScript number stringification at the `1e-7` boundary for schema cache identities.

- Fix MSAA opaque occlusion in OIT and update packed 3D Tiles depth before classification; expand Scene GPU comparisons to 416. Start the model pipeline with five validated resource/JSON/buffer-view loaders, including glTF upgrades, shared requests, cancellation and meshopt filters.

- Add View-owned depth/framebuffer resources, weighted MRT/two-pass OIT, translucent classification plumbing, metadata decoding, depth/drill/ray/height picking and queued most-detailed preload requests. Add pinned-source fixtures and GPU comparisons; Scene, View, Picking and Cesium3DTileset remain partial pending the documented integrations and validation.

- Complete Scene Camera and CameraFlightPath: 3D/2D/Columbus navigation, view fitting, picking, change events, timed flights, callbacks, preload culling and position correction. Validate 1,161 CesiumJS scenarios, renderer snapshots, concurrent cameras and callback resource cleanup.
- Port all 307 `Source/Shaders` modules: 140 builtins and the `CzmBuiltins.js` table, 14 appearances, 19 materials, 55 root shaders, 36 Model, 26 PostProcessStages and 16 Voxels shaders. Each is compared byte for byte with its `@cesium/engine` 23.0.0 default export.
- Reach **100% validated Core coverage**: all 270 applicable Core modules are ported; 18 JavaScript-specific entries retain documented Rust replacements.
- Complete the final 20 modules: Ion/platform clients, ArcGIS/VR/Cesium/GEE/implicit 3D Tiles terrain and metadata, world terrain/bathymetry factories, ground polylines, screen-space gestures and canvas text/pins.
- Add native GEE protobuf/packet processing, meshopt v0/v1 terrain decoding, terrain cache/availability integration and concrete browser canvas/input adapters.
- Validate 25,832 pinned CesiumJS cases, 217 Rust tests and 63 Chromium checks, including full terrain meshes/upsampling, geometry attributes, exact canvas pixels and gesture lifecycle behavior.
- Complete and validate all 35 formerly partial Core modules and portable KTX2/Basis processing; fix decoder/configuration differences, asynchronous error propagation and dropped browser/native task cleanup.
- Keep README, PORTING.md, progress charts and host/validation documentation synchronized. The optional wasm-bindgen package and raw C-ABI viewer both build.

## v0.1.0 — 2026-10-18

First tagged release. A Rust port of CesiumJS 1.139 (`@cesium/engine` 23.0.0), checked module by module against fixtures generated from the real CesiumJS: 22,997 cases across 93 fixture files, all passing.

### Ported

- **19.8% of CesiumJS overall; Core at 77.3%** (209 modules ported, 1 partial — HilbertOrder above level 32). See `PORTING.md` for the module-by-module list.
- Core coverage includes the math types, ellipsoid and projections, geodesics and rhumb lines, splines, geometry types and the geometry pipeline, tiling schemes, terrain encoding/tessellation/providers/picking, time (`JulianDate`, leap seconds, clocks), and Earth orientation / Simon1994 ephemeris.
- Bit-exact V8 math in `core::js_math`, so results match CesiumJS even where algorithms branch on rounding.

### Viewer

- The Lakebed/WebGL viewer in `web/` runs entirely on `src/core` — camera, picking, geodesics, tile grids, and sun direction are thin layers over the ported modules; the first-generation duplicates were removed.
- Apache-2.0 `LICENSE` and `NOTICE` added.

### Notes

- Not published to crates.io (`publish = false`) while the API is still moving; use the git tag.
- The release profile is size-tuned for the wasm viewer; use `cargo run --profile bench --example benchmark` for native performance numbers.
