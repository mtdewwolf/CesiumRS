# cesium-rs

A Rust port of the **CesiumJS 1.139 geospatial core** — the math Cesium uses to put a camera on a WGS84 globe. Not a binding to the JavaScript library, and not a line-by-line copy of the renderer, 3D Tiles, or terrain pipeline.

Ported, and covered by `cargo test`:

| CesiumJS | cesium-rs |
| --- | --- |
| `Ellipsoid.WGS84`, `scaleToGeodeticSurface`, cartographic ↔ Cartesian | `ellipsoid` |
| `Transforms.eastNorthUpToFixedFrame` | `camera::enu_axes` |
| `Camera` heading / pitch / range + perspective | `camera::look_at` |
| `IntersectionTests.rayEllipsoid` | `intersect::ray_ellipsoid` |
| `EllipsoidGeodesic` (Vincenty) | `geodesic` |
| `WebMercatorProjection` | `projection` |
| `GeographicTilingScheme`, `WebMercatorTilingScheme` | `projection` |
| Globe mesh, graticule, tile outlines, star directions | `mesh` |

Algorithms follow CesiumJS 1.139 ([Apache-2.0](https://github.com/CesiumGS/cesium)). This project is not affiliated with Cesium GS, Inc.

```bash
cargo test
cargo build --release --target wasm32-unknown-unknown
```

The `cdylib` exposes a C ABI (`camera_look_at`, `pick_ndc`, `build_globe`, …) for the in-browser globe.
