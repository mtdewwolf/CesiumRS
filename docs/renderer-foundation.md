# Renderer resources and shaders

`src/renderer` ports CesiumJS Renderer (`@cesium/engine` 23.0.0 / CesiumJS 1.139)
with an executable WebGL2 browser adapter and a native `Gl` host interface.
Resource management, geometry packing, shader construction and command execution
run in Rust. The raw C-ABI viewer has not yet migrated to this API, and a native
GPU driver adapter is not included.

The continuation closes eight of the original nine partial modules:
`VertexArray`, `ShaderSource`, `ShaderProgram`, `Texture`, `RenderState`,
`PassState`, `DrawCommand` and `ClearCommand`. Context-level legacy and Scene
integration work remains explicitly partial, as do the new uniform-state paths
that still need complete Scene frame updates.

## API and ownership

| Rust API | Supported behavior |
| --- | --- |
| `Buffer`, `BufferData`, `BufferKind`, `BufferUsage` | Vertex, all index widths and pixel-pack storage; writes, GPU copies and byte readback |
| `VertexArray`, `VertexAttribute`, `GeometryUploadOptions` | Buffered/constant/normalized/instanced layouts; geometry conversion, double-to-float conversion, interleaving with alignment/padding and index promotion; constant and layout updates |
| `Framebuffer`, `FramebufferOptions`, `Attachment` | Color textures/renderbuffers, depth/stencil attachments, status, draw/read binding, explicit attachment ownership and blitting |
| `Renderbuffer`, `RenderbufferOptions`, `MultisampleFramebuffer` | Storage formats/default dimensions, multisample allocation and paired render/resolve targets |
| `ShaderSource`, `PickColorQualifier`, `demodernize_shader` | GLSL source assembly, builtin dependency graph/cycle detection, automatic declarations, picking, cache keys, varying helpers and WebGL1 source rewriting |
| `ShaderBuilder`, `ShaderFunction`, `ShaderStruct`, `ShaderDestination` | Attribute location accounting, stage-specific defines/uniforms/varyings, structs/functions, source generation and independent clones |
| `ShaderProgram`, `ShaderVariable`, `UniformValue` | Compile/link/validate diagnostics, attribute/uniform reflection, sampler unit assignment, cached uniform locations/values, vectors/matrices/arrays and low-precision uniform renaming |
| `ShaderCache`, `ShaderProgramOptions`, `TextureCache` | Reuse, derived/replaced shaders, released-resource collection and explicit destruction |
| `Texture`, `TextureOptions`, `TextureRegion`, `FramebufferCopy` | Typed, compressed, DOM/image and framebuffer sources; supplied mip chains, subimages, samplers, mip generation, IDs/dimensions and memory accounting |
| `RenderState`, `RenderStateCache`, `CachedRenderState`, `PassState` | Immutable state interning, acquire/release, cached sparse transitions, full application and viewport/blending/scissor/pass framebuffer precedence |
| `DrawCommand`, `DrawMetadata`, `UniformMap`, `ClearCommand`, `Pass` | Indexed/non-indexed/instanced submission, per-draw callbacks, texture binding, Scene metadata/flags, dirty tracking, shallow clones/derived maps and masked/scissored clears |
| `Context`, `ContextLimits`, `ContextCapabilities`, `ReadState`, `PixelReadback` | Target selection, typed and pixel-pack readback, hardware limits/extensions and end-frame cleanup |
| `UniformState`, `AutomaticUniform`, `automatic_uniforms` | Model/view/projection and normal matrices, camera high/low encoding, viewport/frustum/pass uniforms; descriptors for all 90 uniforms and explicit Scene-host value overrides |

GPU objects use context-local shared ownership (`Rc`). Last-owner drop deletes
resources. Explicit `destroy()` invalidates aliases and reports host errors;
failed explicit deletions can be retried. Drop cannot return errors. A VAO keeps
its buffers alive. A framebuffer defaults to destroying its attachments, matching
Cesium; disable that with `destroy_attachments: false` or
`set_destroy_attachments(false)` when another owner manages them.

Rust cache leases are `Rc` handles. Dropping a shader or texture lease releases
it; `destroy_released_shader_programs` / `destroy_released_textures` collect entries
with no external owners. A cache hit before collection reuses the same object.
Explicit cache destruction invalidates retained handles and derived programs.
Caches are explicitly owned rather than process-global JavaScript objects.
`ShaderBuilder::build_shader_program` accepts a shader cache;
`compile_shader_program` performs uncached compilation.

Render states are immutable once interned. `from_cache` acquires a state;
`remove_from_cache` releases it, and `clear_cache` removes cached identities while
existing `Rc` values remain valid. Command submission caches transitions and
reapplies changed groups. Clear commands always reapply scissor state for ANGLE
compatibility. After external fixed-function GL changes, call
`Context::invalidate_render_state()` before submitting another command.

`PassState<G>` and `ClearCommand<G>` now carry typed framebuffer references.
A command framebuffer overrides the pass framebuffer. An absent framebuffer
selects the default target. A command viewport overrides the pass viewport, then
falls back to drawing-buffer dimensions. Pass scissor/blending override command
state. Typed readback returns bytes with output pixel format/datatype metadata;
PBO readback returns a `Buffer` that supports `get_buffer_data`. Both return four
components per pixel: RGBA bytes for normalized/packed colors, RGBA float32 for
float/half-float colors, or RGBA_INTEGER uint32 for unsigned integer colors.
Missing channels use WebGL's default components. RG and integer color textures
are accepted as framebuffer attachments.

Shader uniform maps are evaluated for every submission. Static uniforms and
texture bindings remain available. Automatic matrices use the current command
model matrix and context uniform state. Scene hosts must provide lighting,
atmosphere and other missing values through `UniformState::set`; a missing active
automatic value is an error. Full Scene frame-state updates are still pending.

Buffers use byte slices, avoiding JS typed-array offset ambiguity. Base texture
pixels are tightly packed little-endian elements; supplied mip rows retain the
base-level unpack alignment. DOM sources are opaque host image,
canvas, video or ImageBitmap objects; callers supply intrinsic dimensions in
`TextureOptions`. GL allocation/compile/link exceptions become `RuntimeError`;
ordinary GL validation retains `getError` semantics. Cross-context use, invalid
ranges/layouts, destroyed resources and overlapping buffer copies are rejected.
Resources stay on their context's owning thread.

## Validation by batch

The original foundation's 187 portable cases and 173 GPU assertions continue to
pass. The extended suite has **358 portable Renderer reference cases** and
**307 Chromium GPU assertions**, plus native ownership/cache/failure tests.

1. **Targets, textures and state:** GPU comparison covers command/pass target
   precedence, renderbuffer/depth attachment metadata/status, framebuffer copies,
   pixel-pack reads, multisample resolves, masked/scissored clears and 38 GL state
   parameters. Texture draws compare every pixel for byte/float/half/packed data,
   DOM uploads/subimages with flip/premultiply/color-space options, compressed
   DXT mip chains and explicitly sampled supplied mip levels. Native tests cover
   attachment ownership, cross-context rejection, state interning/release and
   sparse transition reuse/invalidation.
2. **Shaders and geometry:** exact fixtures compare builtin dependency ordering,
   picking wrappers, cache keys, varying detection, WebGL1 rewriting, stage-specific
   builder output/clones, all 90 automatic declarations, precision mismatch
   rewriting and transform-derived uniform values. GPU comparison checks geometry
   bytes, alignment/padding, separate/interleaved layouts, normalization, automatic
   model/view/projection uploads, shader reflection/cache reuse and
   float/int/bool/vector/matrix uniform arrays. Context limits are compared against all
   23 Cesium `ContextLimits` properties.
3. **Lifetime and commands:** native regressions cover buffer/VAO sharing,
   destruction/retry, injected setup failures, derived shader replacement and
   release, texture cache leases, per-draw callbacks, command dirty tracking and
   clones. All 234 Rust tests and 63 existing Core Chromium checks pass.

Preserved reference details include constant attributes being context state,
index offsets measured in indices and converted to bytes, count clamping against
the full allocation, packed-pixel `PixelFormat.flipY` padding, supplied mip levels
being uploaded without row reversal and retaining base-level unpack alignment,
nearest filtering for depth textures and
unsupported float-linear filtering, and `floor(base * 4 / 3)` mip memory accounting.
The inverse-normal uniform uses the transpose of model-view, matching Cesium.

Review regressions cover RGB/RED/RG/RGBA, integer, float, half-float and packed
texture readback through typed arrays and PBOs, including allocation and output
metadata. These cases use Cesium texture allocation and explicit WebGL reads:
pinned `Context.readPixels` underallocates non-RGBA outputs, and its framebuffer
debug check omits RG/integer formats. Rust corrects those inconsistencies.
Compressed dimensions are rejected before allocation when zero, above hardware
limits or outside signed GL sizes. Anisotropy metadata and applied GL values are
compared at 1, 1.5 and 2; Rust preserves pinned Cesium's `texParameteri` conversion
of fractional values.

```bash
cargo test --all-features
scripts/check-renderer.sh        # wasm32, wasm-bindgen-cli 0.2.129, Chromium
bash scripts/check-browser-core.sh
node tools/cesium/generate-renderer-assets.mjs
cd tools/cesium
npm run fixtures -- RendererAdvanced
npm run tracker
```

## Remaining Renderer work

`Context` remains partial: WebGL1/extension fallback execution, picking IDs,
default texture/viewport-quad helpers, native GPU adapter and the remaining
Context/Scene lifecycle integration are not implemented. WebGL1 **shader source
rewriting** is implemented and validated independently of backend execution.

`UniformState` and `AutomaticUniforms` remain partial for complete Scene frame
updates: 2D/Columbus View camera conversion, eye/ellipsoid/ENU calculations,
astronomical lighting, fog/atmosphere and environment texture population. Current
3D transform calculations and explicit host-provided values are executable and
reference-validated; they do not claim full Scene integration.

Further modules still include cube/3D textures, atlases, framebuffer managers,
vertex-array facades, compute commands/engine, shared contexts and synchronization.
The tracker lists each outstanding module without treating it as completed.
