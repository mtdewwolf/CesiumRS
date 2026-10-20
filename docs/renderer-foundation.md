# Renderer foundation

The `renderer` module ports the first executable slice of CesiumJS Renderer
(`@cesium/engine` 23.0.0 / CesiumJS 1.139). Rendering decisions and resource
management run in Rust. `renderer::browser::BrowserGl`, enabled by the `browser`
feature on wasm32, forwards GL entrypoints to a supplied WebGL2 context. The
`Gl` trait is also available to native hosts; a native GPU adapter is not yet
included. The raw C-ABI viewer has not yet migrated to this API.

## Public API

| Rust API | Implemented behavior |
| --- | --- |
| `Context`, `Gl`, `GlValue` | Context identity, default-framebuffer draw/clear/readback and a fallible WebGL2 host boundary |
| `Buffer`, `BufferData`, `BufferKind`, `BufferUsage` | Vertex, all three index widths and pixel-pack buffers; allocation, byte writes, nonoverlapping GPU copies and byte readback |
| `VertexArray`, `VertexAttribute`, `AttributeSource` | Buffered/interleaved/normalized layouts, constants, optional indices, disabled attributes and instance divisors |
| `ShaderFunction`, `ShaderStruct`, `ShaderDestination` | GLSL helpers including empty structs, indentation and destination set operations |
| `ShaderSource`, `ShaderCapabilities` | Source/define/version/extension/precision assembly for WebGL2, fragment output declaration and main replacement |
| `ShaderProgram`, `UniformValue` | Compilation/linking with driver logs, pre-link attribute bindings, cached uniform locations, float/int vectors and square matrices |
| `Texture`, `TextureOptions`, `TextureRegion`, `Sampler` | Uncompressed 2D RGB/RGBA/RED/RG typed uploads; byte/float/half/packed pixels, subimages, mipmaps and blank depth textures |
| `RenderState`, `PassState` | Complete fixed-function state application; viewport fallback and pass blending/scissor overrides |
| `DrawCommand`, `ClearCommand`, `Pass` | Indexed/non-indexed and instanced submission, explicit uniforms/textures, masked and scissored clears |

GPU objects use shared context-local ownership (`Rc`). A VAO retains its buffers;
last-owner drop deletes objects. `destroy()` is idempotent, invalidates every
alias and reports host errors. An explicit deletion that fails can be retried;
errors during `Drop` cannot be returned. This replaces Cesium's
`vertexArrayDestroyable` flag. Contexts and their resources must stay on their
owning thread, including in native hosts.

Buffers expose byte slices and readbacks rather than JS typed-array element
views. Texture slices contain tightly packed little-endian elements. Unsupported
texture format/type combinations return errors before allocation. Double vertex
attributes must first be split into float high/low components using Core.
Bounds, alignment, cross-context use, destroyed resources and overlapping buffer
copies return `RuntimeError` instead of issuing invalid GL calls.

GL limits and errors can be inspected through `Context::host()`. Allocation and
shader compiler/linker failures become `Result` errors; ordinary GL errors retain
WebGL's `getError` semantics. The host boundary copies input arrays and returns
owned byte arrays for `getBufferSubData` and `readPixels`. Raw host access is for
integration; draw commands reapply their full render state. Drawing targets the
default framebuffer. `read_pixels` returns RGBA8 rows in GL's bottom-up order.

A Rust browser caller can begin with:

```rust,ignore
use cesium_rs::renderer::{browser::BrowserGl, *};

let context = Context::new_webgl2(BrowserGl::new(webgl2_context)?);
let buffer = Buffer::new(
    &context,
    BufferKind::Vertex,
    BufferUsage::StaticDraw,
    BufferData::Bytes(&position_bytes),
)?;
let vertices = VertexArray::new(
    &context,
    vec![VertexAttribute::buffer(0, buffer, 3)],
    None,
)?;
let program = ShaderProgram::new(
    &context,
    complete_glsl_es_300_vertex_source,
    complete_glsl_es_300_fragment_source,
    &[("position", 0)],
)?;
ClearCommand::all().execute(&context, &PassState::default())?;
DrawCommand::new(&vertices, &program).execute(&context, &PassState::default())?;
```

`examples/renderer_validation.rs` contains a complete textured drawing path with
explicit uniforms, indices, instancing and pixel readback.

## Reference validation by batch

1. **Buffers and vertex arrays:** independent Rust and CesiumJS WebGL2 contexts
   compare uploaded/modified/copied bytes, slice readback, vertex/index counts,
   all index widths, pixel-pack buffers and nonoverlapping same-buffer copies.
   Native host-injection tests cover shared ownership, explicit destruction,
   cross-context rejection, invalid ranges/layouts and failure cleanup.
2. **Shaders, samplers and render-state data:** 187 checked-in cases generated
   by the pinned CesiumJS engine replay constants/validation, GLSL functions,
   structs, destination flags, combined sources and complete state defaults.
   Browser comparisons also exercise shader compile/link rejection and logs,
   inactive uniforms, program destruction and 38 GL state parameters.
3. **Textures and command execution:** Chromium compares every RGBA8 pixel of
   46 draw scenarios (including packed, float and half-float textures) and six
   masked/scissored clears. It checks GL errors on both contexts and texture
   memory accounting, including mipmaps. State transitions include nondefault
   values, resets to defaults and pass overrides. The browser suite has 173
   assertions, with no skipped cases.

Validation preserves these Cesium behaviors:

- The first non-instanced buffered attribute determines the vertex count.
- Constant attribute values are context state and must be restored on each bind.
- Draw offsets count vertices/indices; indexed submission multiplies the offset
  by bytes per index. Explicit counts clamp to the **full** allocation count,
  even with a nonzero offset. WebGL reports an invalid final range.
- Typed texture uploads flip rows in Rust and disable GL's unpack flip flag.
  Cesium's packed-pixel `PixelFormat.flipY` counts unpacked components; its larger,
  zero-filled temporary buffer and resulting sampled pixels are preserved.
- Float textures fall back to nearest filtering without float-linear support;
  WebGL2 depth textures use nearest filtering. Half-float linear filtering is
  core in WebGL2. Mipmap memory accounting uses Cesium's `floor(base * 4 / 3)`.
- Render-state viewport takes precedence over pass viewport; pass blending and
  scissor settings override command state.

Run from the repository root (install `tools/cesium` dependencies first):

```bash
cargo test --all-features
cargo test --test renderer_foundations --test renderer_resources
scripts/check-renderer.sh   # wasm32 target, wasm-bindgen-cli 0.2.129, Chromium
```

Portable fixtures regenerate with:

```bash
cd tools/cesium
npm ci
npm run fixtures -- BufferUsage TextureWrap TextureMinificationFilter \
  TextureMagnificationFilter MipmapHint Pass Sampler ShaderFunction \
  ShaderStruct ShaderDestination ShaderSource RenderState
npm run tracker
```

## Remaining work

The tracker keeps the large modules **partial**. This foundation does not count
those entire modules as validated. Remaining Renderer work includes:

- WebGL1/extension fallback paths and a concrete native GPU host.
- `VertexArray.fromGeometry`, attribute updates and vertex-array facades.
- Builtin GLSL dependency injection, pick shaders, shader builders/caches,
  reflection, uniform arrays/unsigned vectors and automatic scene uniforms.
- Compressed/DOM/framebuffer texture sources, supplied mip chains, integer color
  textures, cube maps, 3D textures and texture atlases/caches.
- Framebuffer/renderbuffer abstractions, pass/command framebuffer selection,
  context limits/capability APIs and render-state interning/differential caching.
- Scene-facing draw metadata, dirty/derived-command tracking and scheduling,
  compute commands, uniform state and Scene integration.
