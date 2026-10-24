# Changelog

## Unreleased

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
