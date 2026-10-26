# Scene

`src/scene` ports the CesiumJS `Scene` package (`@cesium/engine` 23.0.0). It
has one Rust module per Cesium module, as `core` does. `Scene/Model/...`
modules live under `scene::model`.

## Scene and tileset runtime

`Scene<B: SceneBackend>` now owns the frame loop: render requests, time and
camera changes, frame numbering, pre/post update and render events, caught
render errors, primitive passes, flight destination preloading, deferred
callbacks, fog and atmosphere uniforms, and command submission. Call
`initialize_frame(monotonic_seconds)` before `render(julian_time)` to advance
camera flights. Pre/post passes also run when request-render mode skips a draw.
`SceneRenderRequest` can cross threads and supplies a `Waker` for asynchronous
content. The host still drives calls to the frame loop.

`RendererSceneBackend` uses the existing Rust `Context`, uniform state, clears
and executable `DrawCommand`. `SceneCommand::draw` owns the shader and vertex
array while queued. Submission handles pass ordering, culling, occlusion,
back-to-front translucency, depth partitions, closest-frustum execution and
overlays. Other backends can implement the same typed boundary.

`Cesium3DTileset::from_url` loads JSON and relative external metadata schemas
through `ResourceClient`; `from_json` accepts an already loaded document.
Loading validates asset versions and required extensions, builds the explicit
tile tree, inherits transforms/refinement/error, resolves content URLs and
version queries, and sets metadata, credits, bounds and clipping origin.
`load_tileset(resource, json, parent)` preserves each nested document's URL.
Sphere, box, region and S2 bounds and implicit-root subtree descriptors reuse
the existing Scene ports.

Tile content is supplied by `TilesetContentLoader`, whose future returns an
owned `TilesetContent`. Content provides processing, metrics, command updates,
CPU ray picks and destruction. Futures are polled during pre-passes with the
Scene render-request waker. Processing readiness updates statistics, load
events and the cache; post-passes cancel obsolete requests, defer progress
events and evict unused content under the memory budget. `trim_loaded_tiles`
protects tiles touched in the current frame. Destroying a tileset cancels its
requests and suppresses queued events. A tileset implements `ScenePrimitive`
and can be added directly to `scene.primitives`.

`FrameState` now carries the complete upstream data surface. The required
camera, projection, time and culling volume live in an initialized `UniformFrame`;
other optional services/resources use owned `FrameResource` handles. Shadow,
environment, metadata-picking, panorama and pass-state fields retain the
upstream defaults. `Fog` and `Atmosphere` are complete.

Traversal now implements the base, skip-LOD and most-detailed algorithms,
including empty/external ancestors, descendant fallback, mixed-content selection
depths, priority holders, progressive/foveated priorities, movement deferral,
request deduplication and child-union optimization. Visibility and distance use
sphere, oriented-box, region and S2 volumes. Bounds are rebuilt when transforms
or exaggeration change, rather than on every frame.

`from_url` installs `TilesetResourceLoader` by default. It expands external
JSON and implicit subtrees, fetching external subtree buffers and applying
availability, URI templates, metadata bounds/errors and child-subtree headers.
`from_ion_asset_id` uses the existing Ion resource and credentials/credits.
Multiple contents load concurrently and preserve successful contents when a
sibling fails. Statistics account for each pending request and shared GPU
texture identity. Expired content remains available during replacement;
accepted refreshes update accounting, clear the expiry date after download,
and destroy structural descendants when needed. Collision height callbacks
support cancellation, including callbacks already queued for after-render.

Renderable formats require a `RenderableTileFactory` or a custom
`TilesetContentLoader`. Missing decoders report an error; structural content
and raw bytes never stand in for rendered models. A content implementation
owns its format-specific processing, rendering, CPU geometry, aggregate
metrics and GPU texture identities.

`SceneView` fits command bounds to depth partitions, builds pass bins,
tracks shadow ranges/debug overlap masks and dispatches movement start/end
with Cesium's camera tolerance and delay. Call `initialize_frame` with an
absolute monotonic timestamp to advance flights and movement-event timing.
`SceneCommand::from_draw` retains geometry, program, textures, uniforms,
instancing and draw ranges. Build the program with
`ShaderProgram::from_sources` (or `ShaderCache`) so Scene can derive log-depth,
HDR, depth-only and pick shaders from original fragments. Metadata-pick shader
configuration uses `PickMetadataShader`. Compiled variants are reused across
submissions. `framebuffer` and `viewport` route clears and draws to a target;
`draw_passes` exposes the same path for caller-supplied render/pick/depth flags.
On `Scene<RendererSceneBackend<G>>`, `pick` returns the closest visible object
ID within a canvas rectangle. `pick_objects` collects visible IDs in center-out
order. Keep the renderer `PickId` alive and supply its color as the command's
pick uniform. Picked objects retain their original `Rc<dyn Any>` identity.
`begin_pick_async` issues the PBO transfer and fence immediately and returns an
owned `PickReadback`; `.wait(schedule)` uses a host future for each GPU poll.
`pick_async` combines these steps. WebGL1 captures picked objects synchronously.
Picking preserves the caller's framebuffer, viewport, scissor and blending
settings, including primitive/shader failures. It does not advance frame numbers
or fire render events.

View now owns the Scene/id, offscreen pick, depth, globe-depth/translucency,
edge, OIT and translucent-classification GPU resources and destroys them with
its Scene. Depth snapshots are captured per frustum; position reconstruction
supports log depth and scene modes. OIT supports weighted MRT rendering and
the two-pass fallback. The fallback supplies an output declaration missing
from the pinned upstream WebGL2 shader. Globe-depth copy/update honors scissor,
MSAA and classification stencil masks.

Picking now exposes depth positions, metadata and voxel reads, drill picks,
ray intersections, height sampling and clamping. Register `ScenePickObject`
identities and attach their show handles to command visibility for drill
picking. Most-detailed requests preload participating tilesets and resolve
after a subsequent post-render event; the host must keep driving frames.
Cancellation and Scene destruction release queued requests. Metadata decoding
has 500 pinned-source cases, including normalization, transforms and 64-bit
integers. Variable-length metadata arrays remain unsupported by the shader
path. The new depth-position, metadata-render, ray and most-detailed APIs still
need independent end-to-end GPU comparisons; native tests cover failure
restoration, drill visibility and request completion/cancellation.

`SceneTransforms` implements world-to-canvas/buffer transforms, eye offsets,
2D map wrapping, Columbus View/morph coordinates and reconstruction from current
frustum depth, including log depth. `cartesian_to_canvas_coordinates` exposes
the Scene wrapper. Canvas output has a top-left origin; depth reconstruction
takes bottom-left GL coordinates.

Scene and Cesium3DTileset remain **partial**. Scene still needs the actual globe/terrain integration, complete classification
validation, shadows, post-processing, full picking validation, 2D viewport
wrapping/WebVR and controller/morph/terrain integration. The new framebuffer
and classification plumbing does not supply the unported globe primitives.
Tileset rendering still needs styling, polygon clipping, imagery, environment/lighting
and debug rendering. Plane clipping and the mixed-LOD stencil/backface pass
now execute through the native renderer.
`SceneComponent` lifecycle hooks do not implement those effects. Existing
runtime options do not establish support for missing rendering features.

`tools/cesium/fixtures/scene-runtime.mjs` supplies **124 scenarios** (54 Scene,
five fog sequences with 480 updates, six Atmosphere, 59 tileset).
`scene-completion.mjs` adds **377 scenarios**: two FrameState, 48 View fitting,
180 traversal (including clipped trees), 18 implicit expansion, 47 derived shader,
nine camera movement, 24 dynamic-SSE, 21 clipping plane/collection, 24 stencil
state and four mixed-LOD command sequences. `tests/scene_runtime.rs` also checks cancellation,
expiration, shared textures, partial content failure, resource bases, CPU
height queries and renderer/resource ownership. The reference frame harness
uses explicit host operations and does not establish full browser Scene parity.

`scripts/check-scene.sh` compares **568 actual GPU results** with
CesiumJS. The original 24 WebGL2 cases cover log depth, HDR, render/pick/depth
passes, framebuffer targets and retained uniforms/textures. Another 48 phase
comparisons cover float and byte plane textures, mutation/partial upload,
removal/resizing, edge colors, union/intersection modes and WebGL1/WebGL2 log
depth. Another 24 cases exercise actual mixed-LOD draws: masked clears,
ancestor backfaces, selection-depth stencil tests, depth overlap and preservation
of classification bits. The reference uses real Cesium ModelDrawCommand
variants. A further 176 comparisons check color picking, object identity,
rectangle order, transparent fragments, high-DPI coordinates, edge padding,
sync/async readback and WebGL1 fallback. This does not establish visual parity
for unported globe/effects or the newly added picking APIs. Another 64
comparisons cover the View depth/framebuffer resources with WebGL1/WebGL2,
HDR, MSAA, scissor and stencil. Sixty-four compare actual OIT draws across MRT,
two-pass fallback, log depth, HDR, reversed submission order, one/four samples
and opaque occlusion. Sixteen further comparisons probe the packed depth
available to classification after globe/tile draws with HDR, log depth and MSAA.
Scene resolves opaque depth before OIT and updates packed tile depth before
classification. Depth-only picks skip classification passes. These probes cover
the depth handoff; full classification-volume rendering still needs validation.

`scene-transforms.mjs` and `scene-picking.mjs` add **452 reference cases**:
259 transforms, 81 pick culling/rectangle inputs and 112 framebuffer spiral
sequences. Native resource tests check fence timeout/cancellation cleanup,
WebGL1 object snapshots, framebuffer destruction and pass-state restoration
after a failed primitive update.

`ClippingPlaneCollection` owns its GPU texture, handles callbacks/events and
plane indices, supports transformed volume tests, and tracks a single owner.
Attach an `Rc<RefCell<ClippingPlaneCollection>>` with
`tileset.set_clipping_planes(...)`; replacement and destruction release the old
collection. Scene schedules owner texture updates during pre-passes, including
hidden tilesets and skipped draws. Intersecting draw commands inherit the plane
collection and render its clipping/edge stage; fully clipped bounds are culled.
Standalone users can call `collection.update(&context)` and retrieve its typed
texture. A new empty collection reports zero dimensions without allocating one.

Mixed skip-LOD trees enqueue a clear restricted to the skip-LOD stencil bits,
then unresolved ancestor backfaces, then the selected front draws. Selection
depth controls the stencil reference. Backfaces disable color writes and shadows,
cull front faces, and supply polygon offset to linear and logarithmic depth.
Opaque 3D Tiles commands also mark the classification bit. Custom Scene backends
that support these features implement clipping uploads and masked stencil clears.

```bash
cargo test --test scene_completion --test scene_runtime
bash scripts/check-scene.sh
```

## Model resource loaders

The first model-runtime dependencies now implement the shared `ResourceLoader`
lifecycle: `BufferLoader`, `MetadataSchemaLoader`, `GltfJsonLoader` and
`GltfBufferViewLoader`. Calling `load()` repeatedly shares one request and
failure result. Awaiting returns true when data is available, or false if the
loader was destroyed while pending. `unload()` releases data; `destroy()` also
cancels pending work. ResourceClient's host scheduler processes cancelled
transport requests on its next update. Dropping the final owner also cancels
pending work. Embedded buffer/schema loads retain the upstream UNLOADED state
because their data already exists before loading.

`GltfJsonLoader` accepts a parsed pipeline document, a byte view, or a resource
URL. It parses GLB/JSON, decodes data URI buffers, upgrades legacy glTF, fills
defaults, checks the pinned required-extension list and removes pipeline extras.
The result owns cleaned JSON plus embedded and legacy-upgraded buffer sources;
views consume rewritten bytes without refetching old data. Recognizing required
extensions here does not implement their model rendering stages.

`GltfBufferViewLoader` produces shared byte views or decompresses
`EXT_meshopt_compression`: vertex codecs v0/v1, triangle and sequence indices,
and octahedral, quaternion and exponential filters. Its quaternion filter fixes
the signed-component handling missing in meshopt-rs 0.1.2. Hosts can supply a
shared `BufferLoader` lease; views release their own leases after loading.

Ninety-three pinned-source cases cover these five modules. Native tests cover
shared/failing loads, cancellation, retained byte views and feeding JSON/GLB
loads into buffer views. The next batch adds 184 pinned-source cases for cache identities/statistics,
cache ownership traces, glTF texture utilities, Draco decoding and vertex/index
loader lifecycles. Twenty-four additional WebGL1/2 comparisons exercise real
uploads, byte readbacks, indexed triangle pixels, CPU retention and last-lease
GPU destruction across all three index datatypes and synchronous/deferred uploads.

`ResourceCache` is owned by the application or model pipeline; clones share a
pool. Keep a `CacheLease<T>` while using a resource. Cloning a lease increments
its reference count; dropping the last lease destroys the loader and releases
its children. `loader()` returns an observation handle without extending the
cache lease. CPU keys share data across contexts; GPU keys include context IDs.
JSON loaders register embedded/upgraded buffers, and buffer views release their
child leases when their bytes are available. Legacy upgrades publish rewritten
external animation bytes back to the cache. Clearing a cache destroys dependent
resources before their sources. Cache keys preserve pinned-source quirks.

`GltfDracoLoader` shares a capacity-limited `TaskProcessor`. Await `load()` for
the compressed view, then call `process(&frame)` until it returns `Ok(true)`.
Decoded data includes explicit index datatype information at the native wire
boundary. Quantization metadata preserves shader dequantization and octahedral
normal encoding. The portable worker decodes when polled; applications can use
`set_draco_decoder` with a host worker for background decoding. Dropping the last
lease cancels pending tasks and returns decoder capacity.

`GltfVertexBufferLoader` and `GltfIndexBufferLoader` can retain CPU data, upload
GPU buffers, or do both. Provide a renderer context for GPU loads. For deferred
uploads, store `Rc<BufferUploadBudget>` in `FrameState.job_scheduler` and reset
the quota each frame; a missing quota permits immediate upload. An exhausted
quota leaves the loader pending. `process` propagates decode/allocation errors.
This budget is a host adapter, not a port of the complete JobScheduler.

`GltfImageLoader` shares embedded buffer views and external/data-URI image
requests. Embedded PNG, JPEG and WebP are identified by byte signatures, with
`flipY: false` and color conversion disabled at the host boundary. KTX2 uses the
existing Rust/Basis codec and preserves explicit mip chains. Image decoding
uses `ImageHost`; applications provide their decoder, and browsers use
`BrowserImageHost`. Keep the image host and supported KTX2 formats consistent
within each cache. Destroying the final lease aborts pending work; the request
scheduler cleans up cancelled transport jobs on its next update.

`GltfTextureLoader` selects the supported image, shares its loader, then uploads
a texture during `process`. Implement `ModelImageHost<G>` for a custom host;
its default uploads RGBA pixels, while the browser implementation uploads the
original DOM image/bitmap. Raster textures retain glTF's orientation and resize
to powers of two when the sampler requires it, including on WebGL2 as upstream
does. Compressed textures without supplied mips downgrade mipmap filters;
supplied KTX2 mip levels keep their upstream orientation and accounting rules.
Place `Rc<ResourceUploadBudget>` in `FrameState.job_scheduler` for independent
`buffers` and `textures` quotas, or retain the existing buffer-only budget.
Reset the quotas each frame. A failed allocation preserves the loaded image for
retry; image-load failures release dependencies and retain their error chain.

The image/texture batch adds 119 pinned cases and 128 real WebGL comparisons,
including PNG/JPEG/WebP decoding, URI and embedded images, KTX2/Basis uploads,
NPOT resizing, supplied/generated mips, sampled pixels, memory counts and
last-lease destruction. The pinned RGB mip path retains the base image's unpack
alignment: the upstream Green4x4Mipmap RGB asset therefore produces
`INVALID_OPERATION` for its tightly packed 2x2 level. The comparisons preserve
and check this source behavior; RGBA and compressed mip chains upload normally.

`GltfSpzLoader` loads `KHR_gaussian_splatting_compression_spz_2` data. Like
upstream, it always reads buffer view 0 and requests that view when loading.
`scene::spz::load_spz` ports the pinned `@spz-loader/core` 0.3.0 decoder that
upstream calls: SPZ versions 1-3, 24-bit fixed-point or half-float positions,
first-three and smallest-three quaternions, and the wrapper's exponentiated
scales, sigmoid alphas and scaled colors. Results match the WebAssembly bit for
bit. That includes musl's table-driven `logf`, which the `libm` crate's FreeBSD
`logf` does not match. Invalid data returns the C++ decoder's reason
(`deserializePackedGaussians: read error`, unsupported version or SH degree, too
many points). The binding itself aborts with "divide by zero" after printing that
reason. Gzip failures and empty clouds also fail.

Call `process(&frame)` after loading: the first call decodes and returns false.
Upstream's decode promise settles before the next frame, so the result applies at
the first call with a later `frame_number`. That call returns true, or
`Failed to load SPZ` with the reason. Calls in the starting frame return false,
as upstream's pending promise does.

`VertexBufferSource::Spz` reads one attribute of the shared cloud: `POSITION`,
scales and rotations, `COLOR_0` as RGBA8 with alpha, or a spherical-harmonic
coefficient (`KHR_gaussian_splatting:` or `_` prefix,
`SH_DEGREE_<l>_COEF_<n>`). The SPZ loader is leased when loading, as upstream
does. Upstream also swallows SPZ load failures: the attribute's `load()` then
resolves to false and the attribute stays LOADING. Decode failures fail the
attribute that first observes them, and attributes processed afterwards stay
pending. Coefficients the cloud lacks read as NaN, as JavaScript's undefined
array reads do. An unrecognized semantic has no typed array, so it cannot
upload a GPU buffer.

The SPZ batch adds 154 pinned cases against the real WebAssembly decoder and
loaders. They cover every byte value through each transform, every decoder
error, all three versions and SH degrees, and attribute lifecycles with deferred
uploads, failures and the cache entries shared through one SPZ loader and buffer
view. ResourceCache and GltfVertexBufferLoader are now fully validated.

`GltfLoader` now builds typed model components from the shared JSON, geometry,
image/texture and metadata loaders. Await `load()`, then call `process(frame)`
until it returns true. Resource futures are polled each frame. Upload budgets
spread GPU work over frames; `incrementally_load_textures` allows geometry to
become ready while textures stream. Texture load errors are reported without
discarding ready geometry. `components()` borrows the resulting graph.

The loader covers accessor deinterleaving and quantization, Draco/SPZ sources,
primitive restart groups, outlines, morph targets, GPU instancing, node
transforms, skins and animation channels, PBR material factors/textures, modern
and legacy feature IDs, structural metadata, GPM and edge visibility. The
`NGA_gpm_local` root extension is available as typed `Components::gpm_local`.
Undefined accessor offsets and material alpha cutoffs retain `None`.
`release_gltf_json` drops the JSON cache lease after parsing; transient document
references are released after geometry processing.

Three prerequisite ports (`get_mesh_primitives`, `PrimitiveLoadPlan`, and
`PrimitiveOutlineGenerator`) pass 139 direct cases. The main loader adds 48
reference scenarios plus cancellation, cache cleanup, upload quota, generated
buffer ownership and incremental texture regressions. Native tests check outline
lookup texture caching. Geometry/texture uploads use the existing renderer host
contract; these tests do not establish complete Model rendering parity.

The Model pipeline starts with `model::model_utility` (371 direct cases), model,
node and primitive render resources, and the alpha, model color, splitter,
skinning, morph-target, geometry, dequantization, lighting,
vertical-exaggeration, edge-detection, atmosphere, silhouette and classification
stages (87 direct cases). Shader sources, attribute layouts, render states and
numeric uniforms are checked against CesiumJS. `ModelUniformState` and shared
joint/weight arrays supply live uniforms; `renderer_uniform_map()` adapts the
stage providers to `DrawCommand`. Child resources clone shader, alpha and
render-state options while retaining shared GPU resources and uniform providers.
The geometry stage covers 2D projection, instancing and quantized attributes;
the remaining stages add their defines, shader lines and live uniforms.

The next model batch adds `B3dmLoader`, `I3dmLoader`, `PntsLoader` and
`GeoJsonLoader` under `scene::model`. Each implements `ResourceLoader` and
exposes model components. B3DM/I3DM wrap the shared glTF loader, preserve RTC
transforms, and convert batch tables to structural metadata. I3DM builds
world-space instances with quantized positions, orientation, scale and batch
IDs. PNTS builds point attributes, colors, oct normals and metadata; its Draco
path uses the existing decoder worker. GeoJSON converts supported points, lines
and polygon rings to local ENU buffers. A new 22-case pinned suite compares
component attributes, transforms, metadata counts and buffer bytes. Independent
compressed PNTS validation remains pending. Native regressions cover delayed
textures, texture failures, cleanup and destruction. Keep calling `process()`
after geometry is ready while `textures_loaded()` is false.

Material, feature IDs/selection, batch textures, CPU styling, metadata picking,
clipping planes/polygons and tileset stages have 28 further pinned shader and
uniform cases. Picking, wireframe, outline and statistics have native resource
and lifetime tests. Texture providers live in `texture_uniform_map` on model,
node and primitive resources; evaluate them when binding draw textures. Material
and feature texture readers are snapshots from the stage invocation; rebuild
those stage resources when incremental loading attaches new textures.

Instancing and non-instanced 2D projection stages are implemented with persistent
runtime state and native smoke tests. Independent CesiumJS comparison of their
projection/matrix cases remains pending, so the tracker marks them implemented
rather than validated.

`ModelFeature` and `ModelFeatureTable` now manage stable feature handles,
property/class queries, semantic inheritance and callback-based style evaluation.
A feature retains a weak reference to its table and a table retains a weak model
owner, avoiding pick-registry ownership cycles. `BatchTexture` lazily allocates
show/alpha and quantized color arrays, uploads nearest-sampled feature textures
at frame boundaries, and owns per-feature pick IDs. Failed uploads can be retried.
Table `batch_pipeline_source()` and `picking_pipeline_source()` adapters follow
new textures automatically; `batch_statistics()` supplies live memory accounting.
Tile-content callers can use `update_with_pick_objects()` to supply their own
feature objects. The style callbacks accept typed feature handles; expression
parsing and complete tile-content integration remain separate work.

`MetadataPipelineStage` emits metadata, class and statistics structs and
initialization for property attributes, property textures and GPU-packed tables.
It handles property/texture transforms, shared samplers, shader-stage restrictions
and WebGL1 fallback. Only property attribute/texture IDs referenced by the
primitive are processed. Shared samplers retain distinct per-reader UV transforms.
`MetadataUsage` records vertex/fragment property use from
custom-shader and point-cloud callers so unused table reads can be skipped while
preserving packed row indices. Twenty-three pinned shader/uniform cases cover
the compatible source behavior. Dedicated regressions intentionally correct
CesiumJS 23's invalid WebGL1 unsigned declarations, shared-transform collisions,
and integer casts that truncate fractional statistics. Integer value transforms
use typed arithmetic because the shared GLSL transform helper only has floating
point overloads. Native tests preserve unsigned uniforms beyond float32's exact
integer range. Four generated WebGL1/WebGL2 scalar/vector shaders were compiled
and rendered with fractional mean, standard deviation and variance; all produced
RGBA `[128, 128, 128, 255]` for the expected 1.5/3.0 values. Six further reference
traces cover feature visibility/color/style/property sequences, and native tests
cover texture uploads/retries, pick identity, inheritance and destruction.

Custom shader/IBL/point-cloud styling stages and model draw-command construction remain separate work. The built-in renderable
tile factory also remains pending; the default tileset loader requires a factory
for renderable tile content.

## Model runtime and scene graph

`scene::model` provides the glTF runtime transform hierarchy, skinning, articulation, animation, and primitive pipeline stage orchestration:

- `ModelSceneGraph`: builds the runtime scene graph from parsed `ModelComponents`. Manages root node hierarchies, axis correction (glTF Y-up to Cesium Z-up conversions), model matrix updates, 2D/Columbus View map projection (`MappedPositions`), skin matrices, and dirty propagation.
- `ModelRuntimeNode` & `ModelNode`: non-recursive hierarchy nodes maintaining local TRS transforms, ancestor matrices (`transform_to_root`), computed transforms, and joint matrix caching. `ModelNode` provides the public handle with user-specified matrix overrides, visibility toggles, and transform resets.
- `ModelSkin`: manages joint node hierarchies and inverse bind matrices, computing joint matrices (`jointWorldTransform * inverseBindMatrix`) for skeletal mesh skinning.
- `ModelMatrixUpdateStage`: traverses dirty node subtrees non-recursively, updates `transform_to_root` matrices down the tree, and computes draw command model matrices and backface/frontface cull winding reversals under negative determinant transformations.
- `ModelArticulation` & `ModelArticulationStage`: ports `AGI_articulations` glTF extension support with all 10 stage types (`xTranslate`, `yTranslate`, `zTranslate`, `xRotate`, `yRotate`, `zRotate`, `xScale`, `yScale`, `zScale`, `uniformScale`), clamping to stage limits, and matrix composition.
- `ModelAnimation`, `ModelAnimationChannel`, & `ModelAnimationCollection`: keyframe animation player with spline evaluation (STEP, LINEAR, and CUBICSPLINE hermite splines for translation, rotation, scale, and morph weights), playback speed/direction, start/stop/update events, and looping modes (`CLAMP`, `REPEAT`, `MIRRORED_REPEAT`) driven by `JulianDate`.
- `ModelRuntimePrimitive`: selects and configures pipeline stages across 2D/3D modes, morph targets, skins, vertical exaggeration, styles, wireframe, and feature IDs. Adapts runtime draw commands to the host renderer.
- `MappedPositions`: projects cartographic bounding rectangles across ellipsoids for 2D map views.

Tests: `tests/scene_model_runtime.rs`, 24 pinned reference cases generated from CesiumJS 23.0.0 (`tools/cesium/fixtures/scene-model-runtime.mjs`), plus 4 native lifecycle and ownership test suites.

```bash
cargo test --test scene_model_runtime
TZ=UTC node tools/cesium/fixtures/generate.mjs scene-model-runtime
```

`model_components` supplies the 27 glTF data classes, animated-property enum
and material defaults. Shared node/skin/sampler references are indices; buffers
and texture readers use the renderer's typed resources. Structural metadata and
primitive imagery references use optional `FrameResource` handles pending their
runtime modules. A further 293 pinned cases cover these defaults and constants,
texture/cache-key edge cases, and decoder initialization and failure timelines.
The additional suites live under `tests/fixtures/scene/foundations` where their
module names overlap the cache/GPU suites.

`DracoLoader` initializes a host-supplied decoder worker lazily, leaving the
first decode pending until initialization settles. Falsy and rejected module
initializations remain errors on subsequent requests. `GltfDracoLoader::from_host`
uses this initialized worker with a buffer-view factory; the ResourceCache
constructor keeps the portable worker and explicit cache leases. Both paths
share one frame-driven loader and release pending tasks on unload/destruction.

## 3D Tiles content parsers

`Cesium3DTileFeatureTable`, `B3dmParser`, `I3dmParser` and `PntsParser` read
the binary 3D Tiles 1.0 tile formats. Each parser takes the tile buffer as an
`Arc<Vec<u8>>` and a byte offset into it (non-zero inside composite tiles) and
returns its sections as `BufferSource`s: the feature table binary and an
aligned glb are views into the tile buffer, the batch table binary is copied
so the tile buffer can be released, and a glb that is not 4-byte aligned is
copied, as upstream does.

Byte handling follows JavaScript's typed-array rules, because those decide
which tiles load and which fail:
- reading a header word past the end is the DataView `RangeError`;
- a section past the end, or a negative glb length from a short `byteLength`,
  is the Uint8Array `RangeError` — except for an unaligned glb, whose copy
  uses `subarray` and so clamps to an empty glb;
- a feature table property view must start at a multiple of its element size
  relative to the whole tile buffer.

`B3dmParser` recognizes both legacy headers by a batch table length of at
least 0x22000000, the first byte of the JSON or glb a legacy header puts
there. Upstream's `deprecationWarning` calls are returned as
`TileFormatWarning`s.

`Cesium3DTileFeatureTable` caches one typed array per semantic, so a later
request with another component type returns the first array; a binary global
is a one-element typed array (`GlobalProperty::Binary`), and arithmetic reads
it with `ToNumber`, as `PntsParser` does for a binary `POINTS_LENGTH`. JSON
properties go through `createTypedArray`: a JSON number makes that many zeros.

`PntsParser` parses only what `3DTILES_draco_point_compression` does not
provide, keeps upstream's `hasConstantColor` (always false here, since it
reads a property the parse never sets), drops an empty batch table and
removes batch table properties that point into a missing binary body unless
Draco decodes them; the removed names replace upstream's one-time warning.

Tests: `tests/scene_tile_formats.rs`, 59 pinned tiles and call sequences from
`tools/cesium/fixtures/scene-tile-formats.mjs`, built byte by byte and covering
every error path. The B3dm/I3dm/Pnts loaders that turn these into model
component builders remain to be ported.

```bash
cargo test --test scene_tile_formats
TZ=UTC node tools/cesium/fixtures/generate.mjs Cesium3DTileFeatureTable B3dmParser I3dmParser PntsParser
```

## NGA_gpm_local extension

`scene::model::extensions::gpm` holds the `NGA_gpm_local` (generic point-cloud
model) classes: `AnchorPointDirect`, `AnchorPointIndirect`, `CorrelationGroup`,
`Spdcf`, `GltfGpmLocal`, `PpeSource`, `PpeMetadata`, `PpeTexture` and
`MeshPrimitiveGpmLocal`, plus `GltfGpmLoader`, which reads the extension JSON.

`GltfGpmLocal` has two constructors, `direct` and `indirect`. Upstream checks
in debug builds that each storage type has only its own fields; the
constructors make other combinations impossible. The `Spdcf` range checks are
debug-build `DeveloperError`s and are not ported. `PpeSource::SigR` keeps
upstream's value, `"VARZ"`.

`GltfGpmLoader` reads JSON as JavaScript does: `Cartesian3.fromArray` leaves a
missing element undefined (NaN), while the covariance `Matrix3` constructor
turns a missing or null element into 0; a missing array is not iterable and
reading a property of null throws a `TypeError`.

Tests: `tests/scene_gpm.rs`, 35 pinned cases from
`tools/cesium/fixtures/scene-gpm.mjs`.

`GltfMeshPrimitiveGpmLoader` loads a mesh primitive's per-point error (PPE)
textures and exposes them two ways: as `MeshPrimitiveGpmLocal`, and as
`StructuralMetadata` with one property texture per PPE texture. Each texture
gets a class with a single normalized UINT8 property, named after the
texture's source. Its offset and scale come from the texture, with the scale
multiplied by 255. CesiumJS caches these schemas for the whole page, keyed by
the classes' JSON, and numbers new ones `PPE_TEXTURE_SCHEMA_n` by the cache's
size. The port keeps the cache per thread. Textures that share an index share
one lease. `PpeTexture` has no name, so the property textures are unnamed, as
upstream's `ppeTexture.name` is undefined. It is covered in
`tests/scene_metadata_loaders.rs`, below.

## Structural metadata

`StructuralMetadata` is a model's 3D Metadata: its schema with the property
tables, property textures and property attributes that use it. Three parsers
build it:

- `parse_structural_metadata` reads `EXT_structural_metadata`;
- `parse_feature_metadata_legacy` reads the deprecated `EXT_feature_metadata`,
  storing its dictionaries in UTF-16 key order with the keys as ids
  (`MetadataId::Key`) and rewriting feature textures as property textures;
- `parse_batch_table` transcodes a 3D Tiles 1.0 batch table into a generated
  `_batchTable` class: JSON arrays become a `JsonMetadataTable`, the
  hierarchy a `BatchTableHierarchy`, and binary properties either a binary
  `MetadataTable` or, for point clouds, a `PropertyAttribute` whose values
  are returned as custom vertex attributes.

`PropertyTexture` and `PropertyTextureProperty` read a texture info plus
`channels`. The model's textures are passed by glTF index (`GltfTextures`),
so `StructuralMetadata`, `PropertyTexture` and `PropertyTextureProperty` are
generic over the GL host. `get_metadata_property` finds the property texture
property a metadata pick reads.

`parse_structural_metadata` also packs each property table's GPU-compatible
properties into an RGBA8 texture: one row per class property, in class order,
one texel per feature, padded with zeros. Properties the table leaves out are
filled with their `noData` value, written as CesiumJS writes it. Component `k`
of array element `j` lands at `bytesPerElement * i + j + k`, without scaling
by the component size. An enum property has no component type, so filling
one fails; so does a scalar array, which is indexed as if it were wrapped
twice. A failed fill, or a table wider or taller than the context's maximum
texture size, leaves the table without a texture. The parsers return what
CesiumJS logs as `StructuralMetadataWarning`s; `id()` names the one-time
warnings CesiumJS logs once per process. Without a context no texture is
made. The table holds its texture as a `FrameResource` wrapping an
`Rc<Texture<G>>`.

`parse_batch_table` follows JavaScript for binary property offsets:
`binaryBody.byteOffset + property.byteOffset` concatenates when the offset is
a string, and a missing offset is NaN, which the typed array reads as 0, so
the first byte of the tile buffer rather than of the body. Misaligned and
out-of-range views are `RangeError`s. Attribute names are sanitized as in
`ModelUtility.sanitizeGlslIdentifier`; empty and repeated names get
`property_N` placeholders, and INT, UNSIGNED_INT and DOUBLE values are cast
to floats.

A texture reader's `scale` is a number. When a metadata property has a vector
`scale`, which shares the texture info's key, CesiumJS copies the array onto
the reader, and the port reads NaN.

Tests: `tests/scene_structural_metadata.rs`, 54 pinned cases from
`tools/cesium/fixtures/scene-structural-metadata.mjs`. Property table textures
are compared by the bytes uploaded: the fixture stubs `Texture.create`, and
the test host records `texImage2D`.

```bash
cargo test --test scene_structural_metadata
TZ=UTC node tools/cesium/fixtures/generate.mjs PropertyTextureProperty PropertyTexture StructuralMetadata getMetadataProperty parseStructuralMetadata parseFeatureMetadataLegacy parseBatchTable
```

`GltfStructuralMetadataLoader` is the glTF loader's path to this metadata.
Like CesiumJS, it is owned by the glTF loader rather than cached, and it
leases its buffer views, textures and schema from the shared `ResourceCache`.
Ids are gathered in JavaScript object key order, and leases are requested in
upstream's order: buffer views, then textures, then the schema. Upstream's
async ordering decides which failure is reported. A texture that cannot be
requested fails `load` synchronously. A bad buffer view or schema rejects the
load when awaited. `process` waits for every texture upload, then parses
`EXT_structural_metadata` (with property table textures) or the legacy
extension, and releases the buffer views. A loader destroyed while loading
resolves to false.

Tests: `tests/scene_metadata_loaders.rs`, 15 pinned lifecycles of both loaders
from `tools/cesium/fixtures/scene-metadata-loaders.mjs`. They run the real
CesiumJS loaders through its ResourceCache, with images, schema requests and
texture creation stubbed. The comparisons cover states, deferred uploads, the
parsed metadata, failures, cache entry counts and GPM schema ids.

```bash
cargo test --test scene_metadata_loaders
TZ=UTC node tools/cesium/fixtures/generate.mjs GltfStructuralMetadataLoader GltfMeshPrimitiveGpmLoader
```

## Tile content helpers

`preprocess_3d_tile_content` identifies a payload by its magic number, with
`glTF` renamed `glb`. JSON is identified by `root`, `asset`,
`tileAvailability`, `type` or `voxelTable`, checked in that order, so a
tileset, which also has an asset, is not taken for a glTF. A binary payload is
the input slice.

`TileReplacementQueue` is the globe's least-recently-rendered tile list.
CesiumJS links the tiles themselves; the port keeps the links in the queue,
keyed by tile id. `trim_tiles` takes a callback that frees an eligible tile
and reports whether it was eligible. `Cesium3DContentGroup` holds a content's
group metadata, an owned copy.

Tests: `tests/scene_tile_content.rs`, 40 pinned cases from
`tools/cesium/fixtures/scene-tile-content.mjs`. They include nine replacement
queue scripts, compared after each of 498 steps, front to back and back to
front.

## Particles, imagery helpers and keyframe nodes

`Particle` and the emitters are the parts of the particle system that are
independent of rendering. `ParticleEmitter` is a trait with four
implementations: `BoxEmitter`, `CircleEmitter`, `ConeEmitter` and
`SphereEmitter`. Each sets only the particle's position and velocity. Like
CesiumJS, they draw from the seeded Mersenne Twister behind
`core::math::random_between`, and trigonometry is V8-exact, so a seeded run
emits the same particles bit for bit. A particle's `image` is a
`FrameResource` the host owns, and `Particle::update` takes the optional
per-particle callback as a `ParticleUpdateFunction`.

`PointCloudShading` holds the attenuation and eye dome lighting options. Its
`is_supported` reads context capabilities, as
`PointCloudEyeDomeLighting.isSupported` does.

`ImageryLayerFeatureInfo` picks a feature name the way CesiumJS does: the
first truthy property called `name`, then `title`, then one whose key contains
"name", then "title". The comparisons ignore ASCII case only, like the
non-Unicode `/i` regular expressions. Its HTML description nests a table for
each object or array value, skips `null` values and converts others with
JavaScript's `String`.

`ImageryConfiguration` snapshots a layer's appearance through the
`ImageryLayerSettings` trait. `DiscardEmptyTileImagePolicy` discards the
shared empty image by identity. The host decodes `EMPTY_IMAGE_URI` once and
gives the shared image to each policy. `KeyframeNode` is generic over the
voxel spatial node and its content.

Tests: `tests/scene_particles_imagery.rs`, 65 pinned cases from
`tools/cesium/fixtures/scene-particles-imagery.mjs`.

```bash
cargo test --test scene_particles_imagery
TZ=UTC node tools/cesium/fixtures/generate.mjs Particle ParticleEmitter BoxEmitter CircleEmitter ConeEmitter SphereEmitter PointCloudShading ImageryLayerFeatureInfo KeyframeNode ImageryConfiguration DiscardEmptyTileImagePolicy
```

## Camera

`scene::Camera` starts the large, frame-driven Scene modules. It is the
largest prerequisite of `Scene` itself (3,990 upstream lines). The port covers:

- cached local/world position and orientation, cartographic position,
  transform/inverse transform, view/inverse view, and heading/pitch/roll;
- 3D, 2D and Columbus View reference frames, including rotated local frames
  and Geographic/Web Mercator projections;
- move, look, twist, rotate and zoom controls, constrained-axis rotations,
  2D clamping and infinite map scrolling;
- `setView`, `lookAt`, `lookAtTransform`, rectangle fitting and
  `viewBoundingSphere`;
- perspective and orthographic pick rays, ellipsoid/map picking, projected
  sphere distances, pixel sizes, horizon-based view rectangles and frustum
  switching;
- camera change events, position deltas and cloning;
- `flyTo`, `flyHome`, `flyToBoundingSphere`, flight cancellation/completion,
  preload culling volumes and `createCorrectPositionTween`.

`CameraOptions` supplies the projection, canvas and drawing-buffer sizes,
pixel ratio, map mode, Scene ellipsoid and zoom limits. The Scene ellipsoid
and the projection's ellipsoid can differ. `CameraSurface` optionally supplies
globe/depth picks for orthographic-width calculations; the nearer hit wins.
`update_camera_changed(timestamp_ms)` takes an explicit monotonic time.
`move_start` and `move_end` events are exposed for the future Scene/controller
integration, as Cesium's Scene owns their dispatch.

The local pose fields are mutable. Derived-state getters take `&mut self`
because Cesium normalizes edited orientations on read. `set_transform`
preserves world pose while changing reference frames. `update(mode)` checks
the frustum and changes the coordinate interpretation; it does not perform
the Scene transitioner's pose conversion. `CameraView`, `CameraDestination`,
`CameraOrientation` and `CameraOffset` replace JavaScript option unions.
`uniform_camera()` produces the renderer's existing `UniformCamera` snapshot.

```rust
use cesium_rs::{core::Cartesian3, scene::{Camera, CameraOffset, CameraOptions}};
let mut camera = Camera::new(CameraOptions::new(800.0, 600.0));
camera.look_at(
    Cartesian3::new(6_378_137.0, 0.0, 0.0),
    CameraOffset::Cartesian(Cartesian3::new(0.0, -1000.0, 1000.0)),
).unwrap();
let frame_camera = camera.uniform_camera();
```

`tools/cesium/fixtures/scene-camera.mjs` and `scene-camera-flights.mjs` call
the real CesiumJS Camera, CameraFlightPath and TweenCollection with explicit
host inputs. `tests/scene_camera.rs` replays **1,161 scenarios** (793 navigation,
327 flight/lifecycle and 41 direct CameraFlightPath descriptors), including
seeded operation sequences, and checks the renderer snapshot.
Errors inside a scenario are recorded and compared, including Cesium's
debug error when an invalid horizon fallback is needed inside the ellipsoid.

`Camera` and `CameraFlightPath` are complete. Flight options include duration,
end transform, maximum altitude, forced longitude/route weighting, pitch
adjustment, built-in or custom easing, and completion/cancellation callbacks.
Heading and roll interpolate around the shorter angular interval; altitude
and pitch curves match Cesium in 3D, 2D and Columbus View. Orthographic flights
and sphere fits are covered too.

### Flight ownership and host timing

`fly_to(CameraFlightOptions)` queues a flight. Call
`update_flights(time_seconds)` once per frame with an absolute monotonic time.
The first update starts the tween; easing applies before path interpolation.
This scheduler implements the Camera use of `TweenCollection`; the general
Scene TweenCollection module remains separate work. `enable_inputs` exposes
the controller gate: it is disabled while creating a flight and restored
after the user callback runs. The host/controller must read that gate.

Callbacks receive `&mut Camera` and can start another flight. Natural
completion clears the active flight before the completion callback. Forced
`complete_flight()` follows Cesium's cancellation-then-completion order, so
both callbacks run. Replacement flights cancel their predecessors. Tests also
check that cancellation releases captured resources and that callbacks can
enqueue another flight on the same frame.

Flight starts, completion destinations and callbacks belong to each camera.
This deliberately replaces Cesium's shared `scratchStart` and `newOptions`
objects: concurrent cameras cannot overwrite one another's flight state.
Callbacks run without holding internal `RefCell` borrows. Camera uses Rust
ownership instead of the JavaScript object's shared scratch and closure state.

`preload_flight_culling_volume` exposes the destination culling planes.
`preload_flight_camera` optionally supplies an existing preload camera, matching
Cesium's reuse behavior; a temporary clone is otherwise used. 2D flights do
not generate preload planes. `fly_home` in morphing mode emits
`morph_completion_requested`, which the Scene transitioner handles.

`CameraFlightPath::create_tween` also exposes a descriptor directly. Its
`update(camera, time)` consumes the interpolated time value (in seconds),
and `complete`/`cancel` run the lifecycle wrappers. A zero-duration direct
descriptor applies its destination on completion; `fly_to` with an explicit
non-positive duration immediately calls `set_view`. A correction descriptor's
`update` instead consumes its eased 0-to-1 fraction and converts the corrected
world position back into the camera's current reference frame.

Two upstream behaviors are preserved: `complete_flight()` re-applies its
destination with `setView`'s default conversion, and the positional argument
dispatch in CameraFlightPath uses `flyOverLongitude` as Columbus View's pitch
adjustment threshold. The tests exercise those behaviors against CesiumJS.
The viewer in `src/camera.rs` remains a separate viewer helper.

```rust
use cesium_rs::{core::Cartesian3, scene::{Camera, CameraDestination, CameraFlightOptions, CameraOptions}};
let mut camera = Camera::new(CameraOptions::new(800.0, 600.0));
let mut flight = CameraFlightOptions::new(CameraDestination::Cartesian(
    Cartesian3::new(7_000_000.0, 1_000_000.0, 2_000_000.0),
));
flight.duration = Some(2.0);
camera.fly_to(flight).unwrap();
camera.update_flights(10.0).unwrap(); // starts the flight
camera.update_flights(11.0).unwrap(); // midpoint
camera.update_flights(12.0).unwrap(); // completion
```

```bash
cargo test --test scene_camera
TZ=UTC node tools/cesium/fixtures/generate.mjs scene/Camera
TZ=UTC node tools/cesium/fixtures/generate.mjs scene/CameraFlights scene/CameraFlightPath
```

## Foundations batch

The first batch covers **80 modules**. These are the enums and stateless
helpers that the rest of Scene imports, and none of them needs a frame, a
camera or GPU state.

- **Render-state constants:** `BlendEquation`, `BlendFunction`,
  `BlendingState`, `CullFace`, `DepthFunction`, `StencilFunction`,
  `StencilOperation` and `StencilConstants`. The GL-valued enums are `u32`.
  `BlendingState` and `StencilConstants.setCesium3DTileBit` return the
  renderer's `Blending` and `StencilTest`, so they plug straight into a
  `RenderState`.
- **Scene and globe state:** `SceneMode`, `MapMode2D`, `HeightReference`
  (with `isHeightReferenceClamp` / `isHeightReferenceRelative`), `ShadowMode`,
  `SplitDirection`, `ClassificationType`, `TileSelectionResult`, `TileState`,
  `TerrainState`, `ImageryState`, `QuadtreeTileLoadState`, `JobType`,
  `CameraEventType`, `DynamicAtmosphereLightingType` and `Tonemapper`.
- **3D Tiles and metadata types:** `Cesium3DTileContentState`,
  `Cesium3DTileContentType`, `Cesium3DTileOptimizationHint`,
  `Cesium3DTilePass` (with its pass options), `Cesium3DTileRefine`,
  `Cesium3DTileColorBlendMode`, `ImplicitSubdivisionScheme`, `MetadataType`,
  `MetadataComponentType`, `MetadataSemantic`, `ExpressionNodeType` and
  `VoxelMetadataOrder`.
- **glTF and model types:** `AlphaMode`, `AttributeType`, `Axis`,
  `VertexAttributeSemantic`, `InstanceAttributeSemantic`, `ColorBlendMode`,
  `ModelAnimationLoop`, `ModelAnimationState`, `ResourceLoaderState`, and
  `Model/LightingModel`, `ModelType`, `CustomShaderMode`,
  `CustomShaderTranslucencyMode`, `StyleCommandsNeeded`, `UniformType`,
  `VaryingType`, `ModelAlphaOptions`, `ModelLightingOptions`, `ImageryFlags`,
  `CartesianRectangle` and `Extensions/Gpm/StorageType`.
- **Labels, billboards and imagery:** `LabelStyle`, `HorizontalOrigin`,
  `VerticalOrigin`, `BillboardLoadState`, `BlendOption`, `SDFSettings`,
  `ArcGisBaseMapType`, `BingMapsStyle`, `IonWorldImageryStyle`,
  `TileDiscardPolicy` and `NeverTileDiscardPolicy`.
- **Small helpers:** `hasExtension`, `getClipAndStyleCode`, `getBinaryAccessor`,
  `Splitter`, `SupportedImageFormats`, `Light`, `SunLight`, `DirectionalLight`,
  `ParticleBurst`, `QuadtreeOccluders`, `PostProcessStageSampleMode`,
  `CloudType` and `SensorVolumePortionToDisplay`.

### Rust shape

- **Enums:** Cesium's number enums become `#[repr]` Rust enums with
  `value()`, `from_value()`, `ALL` and the Cesium constant `name()`. Its string
  enums have `as_str()` and `from_str_value()` instead.
- **Invalid values:** functions that Cesium guards with a `DeveloperError`
  take the enum, so an invalid value can't reach them. Lookups that return
  `undefined` return `None`.
- **Abstract classes:** `Light` and `TileDiscardPolicy` are traits, validated
  through their implementations.
- **Getting a constructor back:** `AttributeType.getMathType` and
  `MetadataType.getMathType` return a constructor in Cesium. Here they return
  a `MathType` enum carrying the constructor's name.
- **`Axis.fromName`:** Cesium indexes the whole `Axis` object, so the function
  also returns the rotation matrices by name. Here it covers `X`, `Y` and `Z`,
  and the matrices are associated constants.
- **`Splitter.addUniforms`:** returns a renderer `UniformMap` that wraps the
  given map and appends `czm_splitDirection`, read from a closure at each draw.
- **64-bit metadata:** `MetadataComponentType` represents JavaScript `BigInt`
  as `MetadataNumber::BigInt(i128)`. `unnormalize` matches Cesium's mix of
  types. 64-bit results are BigInt, but a negative `UINT64` value clamps to the
  number `0`, because Cesium's lower bound for unsigned types is a plain number.
  A non-integral value is an error, the `RangeError` that `BigInt(NaN)` and
  `BigInt(Infinity)` throw. `set_data_view_value` follows the `DataView`
  setters: numbers wrap like typed-array stores, a BigInt passed to a
  non-64-bit setter is an error, and out-of-range writes fail.

## Validation

`tools/cesium/fixtures/scene-foundations.mjs` imports the real modules and
records 1,428 cases across 78 fixture files. `tests/scene_foundations.rs`
replays them exactly and compares 1,315. The other 113 are cases where
CesiumJS throws a `DeveloperError`. Those are debug-only preconditions and are
skipped, as everywhere else. Render-state presets are compared both as
Cesium's objects and as resolved by `RenderState.fromCache`.

```bash
cargo test --test scene_foundations
TZ=UTC node tools/cesium/fixtures/generate.mjs scene/SceneMode   # one module
```

## GltfPipeline

`scene::gltf_pipeline` ports all **24 `Scene/GltfPipeline` modules**: the glTF
JSON utilities Cesium's loaders run before building a model.

- **Glue for the loaders:** `parseGlb` reads binary glTF 1 and 2, and
  `updateVersion` upgrades glTF 0.8 and 1.0 to 2.0 and turns technique and
  `KHR_materials_common` materials into PBR. `addDefaults` fills in the 2.0
  defaults and `removeUnusedElements` drops unreferenced elements.
- **Helpers:** `ForEach`, the extension-list helpers, `removeExtension`,
  `addToArray`, `addBuffer`, `addPipelineExtras` and `removePipelineExtras`,
  `forEachTextureInMaterial`, the technique movers, and the accessor readers
  (`getComponentReader`, `readAccessorPacked`, `findAccessorMinMax`,
  `getAccessorByteStride`, `numberOfComponentsForType`,
  `updateAccessorComponentTypes`).

### Rust shape

- **The document:** a `Gltf` is the JSON as `serde_json::Value` plus a side
  table of binary sources. Cesium attaches each source to its buffer as
  `extras._pipeline.source`. In the port that property holds a marker naming
  a table entry, so the source moves with its buffer when indices shift.
- **Sources as views:** a `BufferSource` is a view into a shared buffer,
  because Cesium reads through `new DataView(source.buffer)` at
  `source.byteOffset`. GLB bodies are views into the container, and
  `updateVersion` rewrites glTF 0.8 rotations in place, copying the buffer on
  write.
- **JavaScript semantics:** a small helper layer reproduces what the code
  depends on:
  - `for...in` order, with array-index keys first;
  - `defined`, `??` and JavaScript truthiness;
  - `ToNumber` comparisons and `===`;
  - `DataView` `ToIndex` bounds and `new Array` length checks.

  The JavaScript `TypeError`s, `RangeError`s and `SyntaxError`s, and Cesium's
  `RuntimeError`s, become `PipelineError`s. Iteration over glTF 1.0 objects
  skips keys deleted mid-walk, as `hasOwnProperty` does.
- **`ForEach`:** its functions take closures that stop at the first `Some`,
  as Cesium stops at the first defined return. Passes that edit while
  iterating use id lists and mutable lookups.
- **Known deviations:**
  - **`undefined` assignments:** where Cesium assigns `undefined`, for
    example a glTF 1.0 id with no mapping, the port removes the property and
    writes `null` inside arrays. That is what `JSON.stringify` produces. Later
    steps that would read the `undefined` and throw skip it instead.
  - **sRGB rounding:** `updateVersion`'s sRGB conversion uses the platform
    `pow`, which can differ from V8's `Math.pow` in the last bit.

### Validation

`tools/cesium/fixtures/scene-gltf-pipeline.mjs` runs the real modules on
crafted glTF 0.8, 1.0 and 2.0 documents, GLB containers (including truncated,
mislabeled and invalid-JSON ones) and offset buffer views.
`tests/scene_gltf_pipeline.rs` replays the resulting 410 cases:

- 409 are compared, exactly except `updateVersion`, which allows a 1e-12
  relative difference for the `pow`.
- One debug-only precondition is skipped.

```bash
cargo test --test scene_gltf_pipeline
TZ=UTC node tools/cesium/fixtures/generate.mjs gltf_pipeline/updateVersion
```

## 3D Metadata batch

The third batch is in progress. It has **24 modules** so far, in three parts.

- **Schema:** `MetadataEnumValue`, `MetadataEnum`, `MetadataClassProperty`,
  `MetadataClass`, `MetadataSchema`, `getMetadataClassProperty` and
  `MetadataEntity`. `MetadataEntity` is an abstract class in Cesium; here it
  is a trait, and its static helpers are module functions.
- **Tables:** `MetadataTableProperty`, `MetadataTable`, `JsonMetadataTable`,
  `BatchTableHierarchy`, `PropertyTable`, `PropertyAttributeProperty` and
  `PropertyAttribute`.
- **Entities:** `TilesetMetadata`, `TileMetadata`, `ContentMetadata`,
  `GroupMetadata`, `ImplicitSubtreeMetadata`, `ImplicitMetadataView`,
  `Cesium3DTilesetMetadata`, `findGroupMetadata`, `findTileMetadata` and
  `findContentMetadata`. `PickedMetadataInfo` holds a property texture
  property, so it waits for property textures.

### Rust shape

- **Values:** a `MetadataValue` holds what Cesium keeps in plain JavaScript
  values and in its Cartesian and Matrix classes. Where a value may be
  missing, `undefined` is `Option::None`; `Null` is JSON `null`. The model
  reproduces the JavaScript behaviour Cesium relies on:
  - `JsMap` keeps JavaScript key order, with array-index keys first;
  - `BigInt`s stay `BigInt`s, and mixing them with numbers is a `TypeError`;
  - `+` concatenates once an operand is an array or a Cartesian;
  - pack and unpack use Cesium's duck typing, so a missing component is
    NaN.

  Errors are `MetadataError`s, named after the JavaScript error each one
  stands for.
- **Binary tables:** a `MetadataTableProperty` reads its buffer views with
  JavaScript's typed-array rules:
  - offsets must be aligned and lengths must fit, or it is a `RangeError`;
  - 64-bit offsets are a `TypeError`;
  - 64-bit values are `BigInt`s.

  A string property unpacks on first read, as in Cesium, and then has no
  typed array; so does a variable-length array whose length a write changes.
  Inside an array value, an element Cesium reads as `undefined` is `Null`.
- **Mutation:** reads that unpack, and every write, take `&mut self`.
- **Entities:** the entity classes implement the `MetadataEntity` trait
  over the shared helpers. An `ImplicitMetadataView` is a row of its
  subtree's table; the views share that table behind an `Arc<Mutex<_>>`.
- **Hierarchy:** `BatchTableHierarchy` walks parents with an explicit stack.
  It always runs Cesium's debug validation (cycles, out-of-range parents),
  without recursion.
- **Known deviations:**
  - **Owned bytes:** each table property owns a copy of the bytes it reads.
    In Cesium a write goes into the glTF buffer that other views share.
  - **Owned properties:** an entity owns a copy of its JSON properties, so
    writes do not reach the tileset JSON.
  - **Missing properties:** a tile or content without `properties` reads as
    empty. In Cesium its helpers fail a debug check.
  - **No warning:** the finders' one-time missing-schema warning is not
    printed.
  - **Not ported:** the 64-bit fallbacks for platforms without
    `BigInt64Array`, and `PropertyTable`'s GPU texture.
  - **Non-numeric ids:** hierarchy ids that are not JSON numbers read as
    `undefined`.

### Validation

`tools/cesium/fixtures/scene-metadata.mjs`, `scene-metadata-tables.mjs` and
`scene-metadata-entities.mjs` generate the fixtures from the real modules.
Table and entity cases build one object and run a script of reads and
writes against it, so state carries from one step to the next.

- **Schema:** `tests/scene_metadata.rs` compares 11,804 cases exactly. It
  skips 514 debug-only preconditions.
- **Tables:** `tests/scene_metadata_tables.rs` compares 103 cases holding
  4,412 scripted operations. Each operation must match Cesium's value, or
  the name of the error Cesium throws. Eight cases that fail a debug-only
  check at construction are skipped.
- **Entities:** `tests/scene_metadata_entities.rs` compares 101 cases,
  1,275 of them scripted operations. It skips 13 debug-only preconditions.

```bash
cargo test --test scene_metadata --test scene_metadata_tables --test scene_metadata_entities
TZ=UTC node tools/cesium/fixtures/generate.mjs MetadataTableProperty
```

## Tiling batch

The fourth batch ports **11 modules**: the 3D Tiles bounding volumes and
implicit tiling.

- **Bounding volumes:** `TileBoundingVolume`, `TileBoundingSphere`,
  `TileOrientedBoundingBox`, `TileBoundingRegion` and `TileBoundingS2Cell`,
  with `BoundingVolumeSemantics` for volumes stored as `TILE_*` and
  `CONTENT_*` metadata.
- **Implicit tiling:** `ImplicitTileCoordinates`,
  `ImplicitAvailabilityBitstream`, `ImplicitTileset`, `ImplicitSubtree` and
  `ImplicitSubtreeCache`.

### Rust shape

- **Volumes:** `TileBoundingVolume` is a trait. A `TileBoundingVolumeFrame`
  carries the `FrameState` fields a volume reads: the scene mode, the camera's
  `positionWC` and `positionCartographic`, and the map projection.
  `createDebugVolume` returns the outline `GeometryInstance`s, with their
  geometry created. Cesium wraps the same instances in a `Primitive` drawn
  with a flat, opaque `PerInstanceColorAppearance`.
- **Coordinates:** `ImplicitTileCoordinates` keeps Cesium's 32-bit shift
  arithmetic, so coordinates past a level's range fail the same checks. Those
  checks are `MetadataError::Developer`.
- **Subtrees:** `ImplicitSubtree::from_subtree_json` takes a JSON subtree or a
  binary `.subtree` file. External buffers come from the caller's
  `load_buffer` callback, which Cesium gets from `ResourceCache`, and the
  subtree is built synchronously. Buffer views clamp like `subarray`. Metadata
  tables read from each view's offset in the whole buffer, as Cesium's typed
  arrays do. Jump buffers wrap at the width of the typed array Cesium would
  have chosen.
- **Cache:** `ImplicitSubtreeCache` is generic over a `CachedSubtree` trait,
  so it holds an `ImplicitSubtree`, an `Arc` of one, or a test stand-in.
- **Errors:** `MetadataError` gains a `Syntax` variant for `JSON.parse`
  failures in subtree files.

### Validation

`tools/cesium/fixtures/scene-tiling.mjs` runs the real modules and
`tests/scene_tiling.rs` replays 149 cases:

- each volume over random and edge-case inputs, with distances in all four
  scene modes, plane tests and debug geometry. S2 cases add camera points
  outside each face, edge and corner;
- coordinates in both schemes, including the range errors;
- binary, JSON, external-buffer, legacy, multiple-content and octree
  subtrees, with every tile's availability, indices and metadata view, and the
  `RangeError`, `SyntaxError`, `TypeError` and `RuntimeError` cases.

Results match to 1e-12, relative to each array's largest value.

```bash
cargo test --test scene_tiling
TZ=UTC node tools/cesium/fixtures/generate.mjs ImplicitSubtree
```

## Next

The next batches build on these:

- the glTF and 3D Tiles resource loaders on top of `GltfPipeline`;
- the rest of 3D Metadata: property textures, `StructuralMetadata`,
  `PickedMetadataInfo` and the parsers;
- the B3dm, I3dm and Pnts loaders on top of the content parsers;
- completion of Scene/FrameState and tileset traversal, followed by Primitive,
  globe, picking and effect integrations on the Renderer's typed snapshots.
