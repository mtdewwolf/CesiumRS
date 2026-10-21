# Renderer implementation and validation

All 46 CesiumJS Renderer modules (`@cesium/engine` 23.0.0 / CesiumJS 1.139)
have Rust implementations and reference validation. Resource management,
geometry packing, shader construction, frame uniforms and command execution run
in Rust. The browser adapter executes WebGL1/WebGL2 calls, extension entrypoints
and software VAOs when OES vertex arrays are absent. Native applications implement
`Gl`; a concrete native GPU driver is outside this port. The raw C-ABI viewer
still awaits migration to the Renderer API.

The final 13 modules complete Context, UniformState and AutomaticUniforms and
add CubeMap, CubeMapFace, loadCubeMap, Texture3D, TextureAtlas, VertexArrayFacade,
SharedContext, createUniform, createUniformArray and freezeRenderState.

## API and ownership

| Rust API | Supported behavior |
| --- | --- |
| `Buffer`, `BufferData`, `BufferKind`, `BufferUsage` | Vertex, all index widths and pixel-pack storage; writes, GPU copies and byte readback |
| `VertexArray`, `VertexAttribute`, `GeometryUploadOptions` | Buffered/constant/normalized/instanced layouts; geometry conversion, double-to-float conversion, interleaving with alignment/padding and index promotion; constant and layout updates |
| `Framebuffer`, `FramebufferOptions`, `Attachment` | Color textures/renderbuffers, depth/stencil attachments, status, draw/read binding, explicit attachment ownership and blitting |
| `Renderbuffer`, `RenderbufferOptions`, `MultisampleFramebuffer` | Storage formats/default dimensions, multisample allocation and paired render/resolve targets |
| `FramebufferManager`, `FramebufferManagerOptions` | Owned or borrowed attachments, dirty checks, reuse, resizing, multiple color targets, depth/stencil resources, multisample resolve, clear routing and reusable destruction |
| `ComputeCommand`, `ComputeEngine`, `ViewportQuad` | Fullscreen fragment-shader compute, custom geometry/programs, callback uniforms/textures, pre/post/cancel callbacks, persistence and output texture ownership |
| `Sync` | GPU fence allocation/status/destruction and asynchronous polling with host-supplied scheduling, retry budget and timeout errors |
| `ShaderSource`, `PickColorQualifier`, `demodernize_shader` | GLSL source assembly, builtin dependency graph/cycle detection, automatic declarations, picking, cache keys, varying helpers and WebGL1 source rewriting |
| `ShaderBuilder`, `ShaderFunction`, `ShaderStruct`, `ShaderDestination` | Attribute location accounting, stage-specific defines/uniforms/varyings, structs/functions, source generation and independent clones |
| `ShaderProgram`, `ShaderVariable`, `UniformValue` | Compile/link/validate diagnostics, attribute/uniform reflection, sampler unit assignment, cached uniform locations/values, vectors/matrices/arrays and low-precision uniform renaming |
| `ShaderCache`, `ShaderProgramOptions`, `TextureCache` | Reuse, derived/replaced shaders, released-resource collection and explicit destruction |
| `Texture`, `TextureOptions`, `TextureRegion`, `FramebufferCopy` | Typed, compressed, DOM/image and framebuffer sources; supplied mip chains, subimages, samplers, mip generation, IDs/dimensions and memory accounting |
| `RenderState`, `RenderStateCache`, `CachedRenderState`, `PassState` | Immutable state interning, acquire/release, cached sparse transitions, full application and viewport/blending/scissor/pass framebuffer precedence |
| `DrawCommand`, `DrawMetadata`, `UniformMap`, `ClearCommand`, `Pass` | Indexed/non-indexed/instanced submission, per-draw callbacks, texture binding, Scene metadata/flags, dirty tracking, shallow clones/derived maps and masked/scissored clears |
| `Context`, `ContextLimits`, `ContextCapabilities`, `ReadState`, `PixelReadback`, `PickId`, `ViewportCommand` | WebGL1/WebGL2 capabilities/dispatch, targets/readback, default resources/caches, viewport commands, picking, debug validation and deterministic cleanup |
| `CubeMap`, `CubeMapFace`, `load_cube_map`, `load_cube_map_resources` | Six typed/DOM/blank faces, uploads/subimages, framebuffer/mip copies, samplers, supplied/generated mips and concurrent Resource/image decoding |
| `Texture3D`, `TextureVolume`, `SampledTexture`, `TextureBinding` | Typed/blank volumes, subvolume copies and mip chains; common 2D/cube/volume draw and compute sampler bindings |
| `TextureAtlas`, `AtlasImageRequest` | Async images/Resources/providers, deduplication, completion-ordered packing, subregions, resizing, GPU preservation and cancellation |
| `VertexArrayFacade`, `FacadeAttribute`, `FacadeWriter` | Usage-grouped aligned buffers, typed writers, resizing, commit/subcommit, instancing/precreated buffers and indexed chunks |
| `SharedContext`, `SceneContext`, `SharedSurfaceHost` | Independent scene surfaces and dimensions, shared resources, growth/cropped presentation and optional teardown after the last scene |
| `UniformBinding`, `create_uniform`, `create_uniform_array`, `FrozenRenderState` | Typed cached uniform factories and sampler leases; structural deep render-state immutability |
| `UniformState`, `UniformFrame`, `UniformCamera`, `UniformFrustum`, `FrameTextures`, `AutomaticUniform` | All 90 descriptors/providers: geometry, camera/ellipsoid/ENU, 2D/Columbus View, astronomy/lighting, fog/atmosphere, frame values and automatic textures |

GPU objects use context-local shared ownership (`Rc`). Last-owner drop deletes
resources. Explicit `destroy()` invalidates aliases and reports host errors;
failed explicit deletions can be retried. Drop cannot return errors. A VAO keeps
its buffers alive. A framebuffer defaults to destroying its attachments, matching
Cesium; disable that with `destroy_attachments: false` or
`set_destroy_attachments(false)` when another owner manages them.

Framebuffer managers destroy only attachments they create. External attachments
remain valid when a manager is resized or destroyed. `destroy_framebuffer`
releases target objects while retaining attachments; `destroy` also releases
owned attachments and permits a later `update`, as in Cesium. Updates build a
candidate before releasing the current target, so failed construction preserves
the previous valid framebuffer and caller-owned resources.
Attachment changes remain dirty until a successful update, even if a later setter
receives an unchanged handle. Ordinary depth attachments require one sample;
multisampled depth uses the paired depth-stencil path. Requesting ordinary depth
with multiple samples returns an error before allocating or replacing resources,
preserving the existing depth-capable target.

Compute pre-execution callbacks can update resources or replace/remove themselves
before validation. Post-execution runs after submission and transient cleanup.
Nonpersistent commands destroy their program and caller-supplied vertex array;
output textures remain alive. Use `persists: true` to reuse those resources. The
engine owns its default quad and shader cache explicitly and releases them on
destruction, replacing Cesium's implicit Context cache ownership. Cancellation is
invoked by the host/Scene through `ComputeCommand::cancel`.

Sync polling awaits the supplied scheduler before every status query, including
the first. A retry budget of N permits N+1 polls; the default budget is 10.
Scheduler, host and timeout errors propagate to the caller. Browser hosts can
schedule on animation frames or timers; native hosts supply their own future.

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

Shader uniform maps are evaluated for every submission. `Context::update_frame`
accepts a typed `UniformFrame` and `FrameTextures`: camera/frustum snapshots,
Core map projection/time, lighting, atmosphere and image-based lighting inputs.
Renderer computes 2D/Columbus View transforms, eye/ellipsoid/ENU values,
sun/moon directions, fog/atmosphere and the remaining automatic values. The
TEME fallback is used without loaded Earth orientation; `use_earth_orientation`
selects Core's loaded ICRF transform, and a supplied celestial rotation supports
other central bodies. `frame_state`, camera-only and frustum-only updates are
available independently.

Automatic samplers hold context-owned texture leases without resource/cache
cycles. The environment defaults to the white cube; optional depth/edge/BRDF/
specular inputs must be supplied when a shader uses them. Explicit named numeric
overrides remain available. Draws allocate automatic texture units after their
explicit samplers, avoiding collisions. Model matrices, viewport and pass values
continue to come from the current command.

Context owns cached default textures/cube/quad, shaders and textures. Explicit
destruction invalidates retained GPU handles and clears picks/caches; failed
host deletions can be retried. Dropping the last public context releases its
caches without cycles. Shared scene contexts retain the same GPU identity but
use their own surface sizes. Call `SceneContext::begin_frame` before rendering
and `end_frame` to present. `BrowserSharedSurfaceHost` performs the actual 2D
canvas blit with the bottom-aligned crop used by Cesium.

Atlas image futures begin polling when registered and progress during `update`.
Completed images enter packing in completion order, then descending area, as in
Cesium. IDs share requests, subregions wait for parents, and destruction resolves
unfinished additions to -1. Decode, bounds and host-reported GPU errors propagate instead of
leaving callers waiting indefinitely. Facade writers become stale after resize;
managed bytes survive resizing and precreated buffers retain external ownership.

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
pass. The full suites have **1,549 portable Renderer reference cases** and
**1,672 Chromium assertions**, plus native ownership/cache/failure tests.

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
   clones. All 258 Rust tests and 63 existing Core Chromium checks pass.
4. **Managed targets, compute and fences:** GPU comparisons cover target reuse,
   resizing, multiple color attachments, depth/depth-stencil texture/renderbuffer
   choices, multisample resolve, clear-command preservation and destruction.
   Compute comparisons check exact pixels across output sizes, discard/clear
   behavior, input textures and per-execution uniforms, default/custom geometry
   and programs, callback order and persistence. Real fences complete through a
   scheduled asynchronous wait. The 29 new portable cases compare command/manager
   defaults, the viewport shader and exact fence polling/timeout counts. Native
   regressions inject construction, submission, scheduling and deletion failures,
   and check external ownership and callbacks that remove themselves.
   Review regressions also verify every attachment setter preserves pending dirty
   state, MRT pixels follow replacement attachments, and rejected ordinary-depth
   multisampling leaves the previous framebuffer usable. These fix two inherited
   Cesium manager bugs; the reference MRT setup explicitly preserves its dirty
   flag to obtain the intended replacement pixels.

5. **The final 13 modules:** the completion suite adds 1,162 portable cases
   covering all 83 numeric automatic uniforms across 14 frame configurations:
   3D/2D/Columbus View/morphing, four frusta, Geographic/WebMercator projections,
   Sun/directional lights, ellipsoids, times, depth-distance infinity and camera-
   or frustum-only updates. Its 1,311 Chromium assertions also compare cube face
   pixels, DOM/blank/subimage uploads, generated/supplied LODs, framebuffer/mip
   copies, packed volume data, atlas rectangles/repacking/preserved pixels,
   facade GPU bytes/normalization/subcommits/resize/chunk boundaries, every numeric
   uniform factory and array, all seven automatic samplers, Context defaults/
   picking/viewport helpers, WebGL1 software VAOs, shared canvas crops and
   Resource-backed cube loading/errors. Native regressions cover ownership,
   cleanup retry, async completion order/failures/cancellation, stale writers,
   precreated instanced buffers and immutable nested render-state snapshots.

The completion tests correct two additional pinned Texture3D allocation defects:
immutable typed volumes allocate all mip levels so generation can succeed, and
packed storage uses sized internal formats. Reference sampling uses Cesium's
mutable allocation path for generated mips and explicit valid GL allocation for
packed data. WebGL1 Context avoids the pinned constructor's unconditional
3D-limit query; the reference test verifies and clears that specific initial
INVALID_ENUM before enabling debug error checks. Volume `flipY` metadata is
preserved while typed voxel rows retain the reference ordering. Cube memory uses `floor(base * 4 / 3)` after mip loading/
generation; volume generation uses `floor(base * 8 / 7)`.

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
scripts/check-renderer-completion.sh
bash scripts/check-browser-core.sh
node tools/cesium/generate-renderer-assets.mjs
cd tools/cesium
npm run fixtures -- RendererAdvanced RendererCompletion
npm run tracker
```

## Next milestone

Renderer has no remaining partial or todo modules. Scene remains unported and
will supply the typed frame inputs and command scheduling. Migrating the existing
raw viewer and adding a concrete native GPU driver are separate integrations;
the Renderer host traits already support native applications and tests.
