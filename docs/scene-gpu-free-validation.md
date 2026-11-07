# Validating rendering modules without a GPU

This plan covers the partial Scene modules whose remaining gaps are GPU
resources, draw and pick commands, or rendering:

- **Collections (group 4):** PointPrimitive, PointPrimitiveCollection,
  Polyline, PolylineCollection, Billboard, BillboardCollection,
  BillboardTexture, Label, LabelCollection, CumulusCloud, CloudCollection,
  ParticleSystem.
- **Single renderables (group 5):** DepthPlane, DebugCameraPrimitive,
  DebugModelMatrixPrimitive, EllipsoidPrimitive, EquirectangularPanorama,
  Moon, SkyBox, ViewportQuad.
- **Primitive core (group 6):** Primitive, PrimitiveCollection.

None of these needs a hardware GPU to be validated. The repository already
has the three tools required.

## The three tools

| Tool | Where it exists | What it proves | Speed |
| --- | --- | --- | --- |
| **Captured CPU state from CesiumJS** | `tools/cesium/generate-scene-*.mjs` | Exact bytes and values Cesium computes before upload: vertex buffers, index buffers, shader text, uniform values, command fields | Milliseconds, runs in `cargo test` |
| **Recording WebGL host** | the `Host` in `tests/scene_nz_contracts.rs` and `tests/scene_debug_inspector.rs` | The Rust module issues the right GL calls: buffer sizes, attribute pointers, draw counts, program reuse, destruction | Milliseconds, runs in `cargo test` |
| **SwiftShader WebGL2 in headless Chromium** | `scripts/check-scene.sh`, `tools/cesium/scene-runtime-check.mjs`, `examples/renderer_validation.rs` | Real rendering: Cesium on one context and the Rust wasm build on another, in the same page, compared pixel for pixel | Seconds, needs Chromium and `wasm-bindgen` |

SwiftShader is a CPU implementation of Vulkan that Chromium's ANGLE uses for
WebGL2. It is deterministic, so the same shader on the same inputs gives
identical pixels in both runs. It supports float render targets
(`EXT_color_buffer_float`), which picking, OIT and HDR need.

It works without any GPU. In a cloud container with no graphics hardware,
`scripts/check-scene.sh` passes all **568 Scene pixel comparisons**
(derived shaders, clipping, mixed-LOD depth and stencil, classification,
synchronous and asynchronous picking):

```
cargo install wasm-bindgen-cli --version 0.2.129 --locked   # matches Cargo.lock
rustup target add wasm32-unknown-unknown
CHROMIUM_PATH=/path/to/chrome bash scripts/check-scene.sh
```

Getting there needed two fixes, which shows why the harness should run in CI:

- `examples/renderer_validation.rs` had not compiled since PR #82. The
  material `scene::ViewportQuad` added there shadowed the renderer's
  full-screen `renderer::ViewportQuad` through the functions'
  `use cesium_rs::scene::*` imports.
- Asynchronous picking timed out because the Rust `Sync` fence was never
  flushed. WebGL 2 only guarantees that a fence signals after a flush.
  `renderer::Sync::new` now flushes after `fenceSync`.

## Layer 1: capture Cesium's CPU state (most of the work)

Most of what a collection "renders" is decided on the CPU and uploaded once.
If the bytes match, the GPU can only differ through shaders, which are
validated separately as text.

**Vertex attributes.** Every collection packs vertices through
`VertexArrayFacade`, which keeps a CPU `ArrayBuffer` per buffer
(`_allBuffers[i].arrayBuffer`) before `copyFromArrayView` uploads it.
`Renderer/VertexArrayFacade` is already ported and validated. A generator
can:

1. Build a collection in Cesium with scripted contents, run `update` once
   on a real context (SwiftShader page) or a stub context (Node), and dump
   `_vaf._allBuffers[*].arrayBuffer` as base64, with the attribute layout.
2. Make the same collection in Rust, run its writer, and compare the bytes
   exactly.

This replaces the current "attribute expectations computed in the generator"
fixtures for BillboardCollection and CloudCollection, which test the port
against a re-derivation rather than against Cesium.

**Dirty updates.** Script mutations between updates (show, position,
color, removal, reordering) and capture which byte ranges change. Cesium
writes only dirty properties through `writeXxx` functions and rewrites
everything when `_createVertexArray` is set. Capturing per-update bytes
checks both the values and the partial rewrite logic.

**Shaders.** Capture `ShaderProgram.replaceCache` options: vertex and
fragment `ShaderSource` defines and sources, and attribute locations. Compare
as text, as `tests/scene_shadow_volume_appearance.rs` and
`tests/scene_debug_inspector.rs` do.

**Commands.** Serialize each `DrawCommand` Cesium pushes to
`frameState.commandList`: pass, primitive type, offset and count, instance
count, bounding volume, `debugOverlappingFrustums`, render-state fields, and
every uniform-map value evaluated. Compare field by field.

**Pick IDs.** `context.createPickId` hands out sequential colors, and the
collections write them into vertex buffers. Start both sides from a fresh
context so pick colors match byte for byte.

## Layer 2: recording WebGL in `cargo test`

For behavior that is GL calls rather than data, use a recording host in
ordinary Rust tests:

- buffer creation, resize (`createVertexArray` when capacity grows) and
  deletion on `destroy`;
- one draw per command, with the expected count and instancing;
- program reuse across updates (`replaceCache` must not recreate unchanged
  programs);
- the ordering of opaque and translucent command lists.

These tests are cheap and run in CI, which the browser harness does not
yet.

## Layer 3: pixel parity in SwiftShader

Use the existing harness pattern: a wasm export takes a WebGL2 context and a
JSON config, draws into an offscreen framebuffer and returns `readPixels`
bytes, while the page runs the same config through Cesium's real classes on
a second context. Use small framebuffers (64x64) and fixed cameras so
comparisons are exact.

This layer catches what bytes and text cannot: GLSL that compiles but
computes differently, render-state mistakes, depth and blending order, and
derived commands (picking, OIT, shadows).

## What each group needs

### Group 4: collections

| Module(s) | Missing pieces | Validation route |
| --- | --- | --- |
| PointPrimitive, PointPrimitiveCollection | Done: validated by `tools/cesium/generate-scene-point-primitive-collection.mjs`, which records Cesium's facade bytes, every `bufferData`/`bufferSubData`, commands and defines on SwiftShader, replayed in `tests/scene_point_primitive_collection.rs` | Layer 3 pixels for a handful of points remain optional |
| Billboard, BillboardCollection, BillboardTexture | Done except terrain clamping and the vector-tile batch table: `tools/cesium/generate-scene-billboard-collection.mjs` records bytes, uploads, commands, atlas and image states for instanced and batched contexts with ImageData images; replayed in `tests/scene_billboard_collection.rs` | Clamping needs `Scene.updateHeight` |
| Label, LabelCollection | Glyph rasterization and layout, SDF | Run both sides in the same Chromium page and have Rust call the page's canvas through a host glyph callback. Both sides then rasterize with the same fonts, which removes the platform font problem. Compare glyph bitmaps, then billboard bytes |
| Polyline, PolylineCollection | Done: `tools/cesium/generate-scene-polyline-collection.mjs` records uploads, pointers, commands and polyline state in every mode | Layer 3 pixels for width and miter joins remain optional |
| CumulusCloud, CloudCollection | Noise texture (a compute pass), writers, commands | Layer 3 for the noise texture via `readPixels`; layer 1 for the per-cloud bytes |
| ParticleSystem | Done: `tools/cesium/generate-scene-particle-system.mjs` compares pooled billboards and buffers each frame | None |

Prerequisite: `createBillboardPointCallback` (71 lines, todo) for point
images in billboards.

### Group 5: single renderables

| Module | Notes |
| --- | --- |
| EllipsoidPrimitive, Moon | Own box geometry, shader and command. Layer 1 for geometry and shader text, layer 3 for the ray-cast ellipsoid. Moon follows EllipsoidPrimitive. |
| DepthPlane | Own vertex array and command; layer 1 and a depth readback in layer 3. |
| DebugCameraPrimitive, DebugModelMatrixPrimitive | Built from `Primitive` with Frustum and Polyline geometry. They wait for group 6. |
| EquirectangularPanorama, SkyBox | Texture and cube-map uploads (CubeMap is ported); use ImageData sources and compare pixels. |
| ViewportQuad | Already executes against the recording host; add layer 3 pixels and Scene submission. |

### Group 6: Primitive and PrimitiveCollection

Done for the synchronous path: `tools/cesium/generate-scene-primitive.mjs`
records Cesium's state, uploads, attribute pointers, exact shader text, render
states, commands, bounding volumes and batch-table values on SwiftShader across
every scene mode, picking, depth-fail and instance-attribute writes, replayed
in `tests/scene_primitive.rs`. It builds on the validated `BatchTable`.

Remaining:

1. Asynchronous creation: drive the state machine with the native worker
   adapter and compare readiness across frames (today it is emulated over
   updates).
2. `rtcCenter` rendering and texture-valued material uniforms.
3. Layer 3 pixels for a translucent and a depth-fail primitive (optional).

PrimitiveCollection's remaining work (identity membership and events) is
CPU-only and can be validated with ordinary pinned traces.

## Suggested order

1. **BatchTable** (done), then **Primitive** (group 6, synchronous path done). This unblocks
   DebugCameraPrimitive, DebugModelMatrixPrimitive and much of the rest of
   Scene.
2. **PointPrimitiveCollection** (done), the simplest VertexArrayFacade
   collection. Its capture generator is the template for the others: wrap
   `gl.bufferData`/`gl.bufferSubData`, hash the facade's CPU buffers and
   record each command after every update.
3. **BillboardCollection** (done but clamping), then **LabelCollection** (shared-page glyphs),
   **PolylineCollection**, **CloudCollection** and **ParticleSystem**, all
   reusing that generator.
4. **EllipsoidPrimitive** and **Moon**, **DepthPlane**, the panoramas and
   **SkyBox**.

Running the SwiftShader harness in CI would protect all of this. CI
currently runs only `cargo test`, clippy and the wasm builds. A job that
installs Chromium through Playwright and runs `scripts/check-scene.sh`
would make layer 3 a regression gate rather than a local check.
