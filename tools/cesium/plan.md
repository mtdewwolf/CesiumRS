Batches toward **100% Core**, in dependency order. Each module is ported with CesiumJS fixtures before it is marked ported. The 75% milestone is an intermediate checkpoint, not the completion target.

Along the way, `core::js_math` made the port's `sin`, `cos`, `log` and the other `Math` functions round exactly as V8's do, after a one-bit difference in `cos` moved a rectangle's bounding sphere.

1. ✅ **Small utilities:** containers, URI and query helpers, typed-array strings, `PixelFormat`, `PixelDatatype`, and Core's enums.
2. ✅ **Polygon and instance foundations:**
   - ✅ earcut 3.2.4 and `PolygonPipeline`
   - ✅ `GeometryInstance` and its Color, Show, Offset and DistanceDisplayCondition attributes
   - ✅ `GeometryPipeline`, including antimeridian splitting of triangles, lines and polylines
3. ✅ **Surface geometries:**
   - ✅ `EllipseGeometry`, `CircleGeometry`
   - ✅ `RectangleGeometry`, `RectangleGeometryLibrary`, `RectangleOutlineGeometry`
   - ✅ `PolygonGeometryLibrary`, `PolygonGeometry`, `PolygonOutlineGeometry`, and the coplanar polygon geometries
4. ✅ **Line and volume geometries:**
   - ✅ `PolylinePipeline`, `PolylineGeometry`, `SimplePolylineGeometry`
   - ✅ `WallGeometryLibrary`, `WallGeometry`, `WallOutlineGeometry`
   - ✅ `PolylineVolumeGeometryLibrary`, `PolylineVolumeGeometry`, `PolylineVolumeOutlineGeometry`
   - ✅ `CorridorGeometryLibrary`, `CorridorOutlineGeometry`
   - ✅ filled `CorridorGeometry`, including extrusion, surface frames, bounds and shadow volumes
5. ✅ **Terrain helpers:** `TerrainEncoding` and `HeightmapTessellator`, passing the 75% Core checkpoint
6. ✅ **Terrain data and picking:**
   - ✅ `TerrainProvider` contract, cached grid/edge/skirt indices and geometric errors; `TileAvailability` quadtree ranges and coverage queries
   - ✅ `TerrainPicker`, `TerrainMesh`, and native incremental picker refinement
   - ✅ `TerrainData`, `HeightmapTerrainData`, LERC decoding and native heightmap mesh processing
   - ✅ `QuantizedMeshTerrainData` and native quantized mesh creation/upsampling
   - ✅ 3D Tiles and Google Earth Enterprise terrain data/processors
7. ✅ **Remaining spatial algorithms:**
   - ✅ `S2Cell`, Hilbert levels above 32 through Cesium's finite numeric bounds, and `ApproximateTerrainHeights` with the pinned lookup table
   - ✅ `RectangleCollisionChecker`, including multi-level R-tree insertion/removal reference cases
   - ✅ `GroundPolylineGeometry`
8. ✅ **Asset and texture processing:** compressed texture buffers, portable KTX2/Basis decoding/loading, texture packing, vector/GEE decoding and concrete browser image/canvas adapters. Reference fixtures and real Chromium checks pass.
9. ✅ **Resource and execution infrastructure:** requests/errors, scheduling/statistics, resources and typed HTTP operations/retries, trusted servers, workers, proxies and sanitized credits. Reference scheduler fixtures, native HTTP/worker integration and Chromium lifecycle checks pass.
10. ✅ **Terrain providers and service clients:**
   - ✅ `EllipsoidTerrainProvider`, `CustomHeightmapTerrainProvider`
   - ✅ network heightmap/quantized and 3D Tiles terrain providers, Google Earth Enterprise metadata and terrain, world-terrain factories. Ion resources/geocoding and platform clients are now validated. Bing, Google, Pelias, OpenCage and coordinate geocoders pass deterministic parsing, request construction, attribution and error fixtures.
11. ✅ **Contracts and host behavior:** interpolation/packing interfaces, geometry factories, video bindings, terrain sampling, screen-space gestures and canvas text/pin rendering validated.

Core, Renderer, Shaders and Workers have reached 100% of applicable modules; Scene remains in progress; DataSources property foundations are now active. Progress requires implemented behavior and reference validation; placeholder APIs do not count. Existing n/a entries describe JavaScript-specific idioms and must retain explicit Rust replacement notes. The viewer already runs on Core — it was migrated ahead of 100% Core so the two implementations could not drift apart. After Core, proceed to Renderer and Scene.

### Validated partial-module batch

All 35 formerly partial Core implementations and the native KTX2 worker calculation now pass validation. The batch adds 617 pinned CesiumJS cases, native lifecycle/HTTP checks and 23 Chromium checks, including Rust video/worker bindings and packaged WASM codec output. Validation found and fixed short GEE packet decoding, Basis codec configuration/output differences and lost ready terrain results on tile/level errors. See [validation details](docs/core-validation.md).

The final 20 Core modules are implemented and validated. This batch adds 199 pinned reference cases, native terrain/input integration and exact Chromium text/pin and gesture comparisons. Core has 270 validated applicable modules and no remaining partial or todo entries. The 18 n/a entries retain explicit Rust replacement notes. Renderer is now complete; Workers completion is the next package milestone.


### Renderer completion

All **46 Renderer modules** are implemented and reference-validated. The final
13 close Context, UniformState and AutomaticUniforms and add cube/volume textures,
face loading, atlases, vertex-array facades, shared contexts, uniform factories
and immutable render-state helpers. The browser adapter executes WebGL1 and
WebGL2, including extension dispatch and a software VAO fallback.

Validation covers **1,549 portable Renderer cases**, **1,720 Chromium assertions**
across the original and completion suites, and native lifecycle, async ordering,
cache, cancellation and failure regressions. All 90 automatic uniforms have value
providers; 83 numeric values are compared across 14 Scene frame scenarios and
all seven sampler inputs are exercised during draws. See
[Renderer API and validation](docs/renderer-foundation.md).

After Workers, port Scene using the typed frame snapshots, shared resource
ownership and executable Renderer commands. The raw viewer's migration to this
API and a concrete native GPU driver remain separate integration work; native
applications can implement the existing `Gl` host contract.

### Scene

Scene has **210 of 482 modules** fully ported, including Camera and
CameraFlightPath. The validated batches cover the foundations, all 24
GltfPipeline utilities, 3D Metadata schemas/tables/entities, tile bounding
volumes and implicit tiling.

Large remaining modules are prioritized by upstream size, with prerequisites
implemented first. Camera (3,990 lines) and CameraFlightPath (577) now cover
navigation, fitting, picking, change events, renderer snapshots, timed flights,
callbacks, preload culling and correction tweens. Their 1,161 direct CesiumJS
scenarios pass; hosts supply the clock and consume the controller input gate.
Flight state is owned per camera instead of using shared JavaScript scratch.

Scene orchestration (5,293 upstream lines) and Cesium3DTileset (3,933) now
have partial Rust runtimes: frame decisions and events, renderer submission,
JSON/schema/tree loading, explicit-tree traversal, content futures, cancellation,
progress and cache eviction. Fog and Atmosphere are fully ported. The new batch
has 124 CesiumJS scenarios and Rust integration checks; these do not establish
full rendering or traversal parity. FrameState has since been validated; Scene, View, Picking and Cesium3DTileset remain partial.

The 3D Tiles binary content parsers (Cesium3DTileFeatureTable, B3dmParser,
I3dmParser, PntsParser), the NGA_gpm_local data classes with GltfGpmLoader,
and the rest of 3D Metadata (property textures, StructuralMetadata and its
three parsers) are validated, with preprocess3DTileContent,
TileReplacementQueue and Cesium3DContentGroup; the B3dm/I3dm/Pnts loaders that build
model components from them come with GltfLoader. Continue model prerequisites: SPZ resources, then the main
GltfLoader/Model pipeline. ResourceCache and vertex loading remain partial;
Draco, ModelComponents, image/texture and GPU index loading are validated. ScreenSpaceCameraController,
Scene/View picking integrations and built-in renderable tile content remain
gaps; the runtime currently requires a caller-supplied content loader. See [Scene](docs/scene.md) for APIs, validation and remaining work.


### Workers completion

All **52 applicable Workers modules** are validated, matching the Workers row in the package table; `transferTypedArrayTest` is n/a.
Direct CesiumJS worker fixtures cover typed/packed inputs and batch/result wire
formats; geometry, terrain, vector-tile, Gaussian splat, I3S, Draco, combineGeometry, and
createTaskProcessorWorker workers are in that validated set. Ground polylines use the bundled height table. Host-registered
Rust factories replace dynamic module imports. No Scene module is marked complete
for the result-packing helpers.

`combineGeometry` is ported. It unpacks `createGeometry` results onto packed
instances, then runs the existing geometry pipeline: world transforms, optional
antimeridian splits, `batchId`, vertex-cache reorder, combine, 2D projection,
high/low encoding, compression, and unsigned-short index fitting. Bounding
spheres are packed as `Float32Array`s. Direct worker cases call the real
adapter. Shared geometry helpers are unchanged.

`createTaskProcessorWorker` is ported. It adapts a calculation to the
TaskProcessor message contract: task id, parameters, `CESIUM_BASE_URL`,
`Error` versus non-Error failures, clearing the transfer list when
`canTransferArrayBuffer` is false, and the `postMessage` / `onmessageerror`
failure strings produced by `formatError`. Direct worker cases call the real
adapter. `TaskProcessor` itself is unchanged and still moves owned buffers.
Object keys in those failure strings follow `JSON.stringify`: array indexes
first, in ascending order, then every other key in insertion order.

`transferTypedArrayTest` is n/a. It is a Web Worker `onmessage` handler that
posts the received typed array back by transferring `array.buffer`, or posts an
empty object if that transfer throws. It performs no calculation. Rust workers
return owned buffers, and `TaskProcessor` moves those values instead of
transferring `ArrayBuffer`s.

`decodeDraco` is ported. Meshes and point clouds, including non-float and
64-bit attributes, the geometry-type errors, and the WebAssembly initialization
message (`true`; there is no module to compile) match the CesiumJS worker on
direct cases. `dequantizeInShader`, which Model's Draco loader turns on by
default, keeps the quantized integers and their quantization or octahedral
parameters. Upstream `draco-core` 2.3.2 always inverse-transforms, so it is
vendored in `third_party/draco-core` with an equivalent of C++ Draco's
`SetSkipAttributeTransform`. Mesh decode, face indices, and the mapped-index
attribute walk live in `workers::draco`, which `decodeI3S` also uses. See
[Workers details](docs/workers.md).

Shaders is complete: all **307 modules** are ported. Each `Source/Shaders`
subdirectory is one Rust module that registers its shaders in `src/shaders`.
Shader text is the exact default export of the `@cesium/engine` 23.0.0 module:
140 Builtin modules and the `CzmBuiltins.js` table, 14 Appearances,
19 Materials, 55 root `Shaders/*.js`, 36 Model, 26 PostProcessStages and
16 Voxels modules. Each is compared byte for byte with its CesiumJS default
export, and the fixtures discover every upstream module, so an unregistered one
fails its replay. Two root exports differ from their upstream `.glsl` file
because Cesium's build drops unescaped backslashes; the port keeps the export.
The renderer still assembles programs from the pinned builtin table. See
[Shaders](docs/shaders.md).


### DataSources property foundations

Seven of 108 DataSources modules are validated: Property, ConstantProperty,
CallbackProperty, PropertyArray, SampledProperty, TimeIntervalCollectionProperty
and CompositeProperty. The batch includes 206 pinned CesiumJS scenarios,
14 native ownership/callback integration tests and real browser clock checks.
Owned values, Rc property references, local definition events and observable Core
intervals provide the prerequisites for Entity and document loading.

Position/velocity/orientation properties and the entity foundation are now
validated below. Next implement composite collections, graphics definitions,
data-source clocks/collections and the loaders. CzmlDataSource and KmlDataSource are the largest loaders; their property
and entity dependencies come first. GeoJSON and GPX follow the same foundation.
No entity, visualizer or document-loader module is counted as complete by this
batch. See [DataSources API and validation](docs/data-sources.md).


### DataSources positions and motion

DataSources has 17 of 108 modules validated. Ten position/motion modules add
fixed/inertial conversion, constant/callback/sampled positions, interval and
composite positions, position arrays, surface scaling, velocity and orientation.
PositionContext reuses Core transforms with TEME fallback, loaded IAU/EOP data
or a custom central-body transform. The batch adds 412 pinned scenarios, eight
native lifecycle checks and real browser evaluation with omitted times.

PropertyBag, Entity/EntityCollection and references are now validated below.
See [DataSources](docs/data-sources.md).


### DataSources entity foundation

DataSources has 23 of 108 modules validated. Six modules add property/raw accessor
slots, heterogeneous PropertyBag values, Entity hierarchy/availability/transforms,
batched EntityCollection membership and lazy ReferenceProperty paths. The batch
adds 126 pinned scenarios, 14 native lifecycle checks and browser ownership/clock
coverage, bringing DataSources to 744 reference cases.

Entity graphics slots accept typed EntityComponent implementations; concrete
Graphics classes and automatic raw graphics-object conversion remain unported.
Scoped registries replace JavaScript prototype registration. Weak references and
RAII subscriptions prevent ownership cycles and stale removed-child callbacks.

Next implement CompositeEntityCollection, concrete graphics definitions,
DataSourceClock/data-source collections and loaders. See [DataSources](docs/data-sources.md).
