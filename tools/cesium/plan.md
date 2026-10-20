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

Core has reached 100% of applicable modules; Renderer is the current milestone. Progress requires implemented behavior and reference validation; placeholder APIs do not count. Existing n/a entries describe JavaScript-specific idioms and must retain explicit Rust replacement notes. The viewer already runs on Core — it was migrated ahead of 100% Core so the two implementations could not drift apart. After Core, proceed to Renderer and Scene.

### Validated partial-module batch

All 35 formerly partial Core implementations and the native KTX2 worker calculation now pass validation. The batch adds 617 pinned CesiumJS cases, native lifecycle/HTTP checks and 23 Chromium checks, including Rust video/worker bindings and packaged WASM codec output. Validation found and fixed short GEE packet decoding, Basis codec configuration/output differences and lost ready terrain results on tile/level errors. See [validation details](docs/core-validation.md).

The final 20 Core modules are implemented and validated. This batch adds 199 pinned reference cases, native terrain/input integration and exact Chromium text/pin and gesture comparisons. Core has 270 validated applicable modules and no remaining partial or todo entries. The 18 n/a entries retain explicit Rust replacement notes. Renderer and Scene are the next package milestones.


### Renderer foundation

The first executable Renderer slice lives in `src/renderer`, with a concrete
WebGL2 browser adapter and a native host interface. Buffer/VAO, shader/state and
texture/draw batches pass pinned CesiumJS validation: 187 portable cases,
Chromium GPU byte/pixel/state comparisons and native ownership/failure tests.
See [Renderer API, validation and remaining work](docs/renderer-foundation.md).
Large modules stay partial where geometry conversion, automatic uniforms,
shader/cache helpers, texture sources, framebuffers or Scene integration remain.

Next batches, in dependency order:

1. Framebuffers/renderbuffers, capability/limit handling and texture source paths.
2. Shader builders, builtin dependencies, reflection and automatic uniform state.
3. Geometry-to-VAO conversion, caches, derived commands and compute execution.
4. Scene integration, followed by the remaining Renderer and Scene modules.
