# Changelog

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
