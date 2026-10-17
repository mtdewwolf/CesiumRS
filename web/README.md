# Viewer

A [Lakebed](https://lakebed.dev) capsule that renders a WGS84 globe with WebGL. Every camera matrix, pick ray, geodesic, tile outline, and mesh comes from this repo's Rust core, compiled to `cesium_rs.wasm`.

```txt
client/index.tsx   Preact UI: places, Vincenty geodesics, measuring, layers
client/globe.ts    WebGL renderer and orbit / tilt / zoom controls
client/engine.ts   wasm loader and typed wrappers for the C ABI
client/generated/  wasm and Earth texture as base64 (scripts/build-web.sh)
shared/places.ts   place list and formatters
server/index.ts    capsule definition (no data or endpoints)
```

A capsule can only ship its favicon as a static file, so the wasm and the 2048×1024 Natural Earth II texture travel inside the client bundle. The texture is equirectangular with the north edge first; it is decoded with `createImageBitmap` and uploaded with `UNPACK_FLIP_Y_WEBGL` off, matching `v = 0` at the north pole in `mesh::build_globe`.

```bash
scripts/build-web.sh          # from the repo root: cargo build + regenerate client/generated
cd web && npx lakebed dev     # http://localhost:3000
cd web && npx lakebed deploy
```

Controls: drag orbits, right-drag or Shift-drag tilts and turns, scroll zooms, click picks. On touch: drag, pinch, and two-finger twist or tilt.
