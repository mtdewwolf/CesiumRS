# Pinned Renderer shader data

Generated from Apache-2.0 CesiumJS 1.139 (`@cesium/engine` 23.0.0):

- `glsl-builtins.json`: 230 GLSL builtin bodies and automatic-uniform declarations.
- `automatic-uniforms.json`: datatype and array-size descriptors for 90 uniforms.

Run `node tools/cesium/generate-renderer-assets.mjs` from the repository root
with `tools/cesium` dependencies installed. Rust resolves dependencies and
applies uniforms; these files contain data, not a JavaScript renderer runtime.
See the repository's LICENSE and NOTICE for Cesium attribution.
