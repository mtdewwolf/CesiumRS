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
6. ⬜ **Terrain data and picking:**
   - ✅ `TerrainProvider` contract, cached grid/edge/skirt indices and geometric errors; `TileAvailability` quadtree ranges and coverage queries
   - ✅ `TerrainPicker`, `TerrainMesh`, and native incremental picker refinement
   - ✅ `TerrainData`, `HeightmapTerrainData`, LERC decoding and native heightmap mesh processing
   - ✅ `QuantizedMeshTerrainData` and native quantized mesh creation/upsampling
   - ⬜ 3D Tiles and Google Earth Enterprise terrain data/processors (service-client batches below)
7. ⬜ **Remaining spatial algorithms:**
   - ✅ `S2Cell`, Hilbert levels above 32 through Cesium's finite numeric bounds, and `ApproximateTerrainHeights` with the pinned lookup table
   - 🟦 `RectangleCollisionChecker` implemented; validation deferred
   - ⬜ `GroundPolylineGeometry`
8. 🟦 **Asset and texture processing implemented:** compressed texture buffers, portable KTX2/Basis decoding/loading, texture packing, vector/GEE decoding and concrete browser image/canvas adapters. Reference validation remains.
9. 🟦 **Resource and execution infrastructure implemented:** requests/errors, scheduling/statistics, resources and typed HTTP operations/retries, trusted servers, workers, proxies and sanitized credits. Concrete native/browser hosts are supplied; reference validation remains.
10. ⬜ **Terrain providers and service clients:**
   - ✅ `EllipsoidTerrainProvider`, `CustomHeightmapTerrainProvider`
   - ⬜ network heightmap/quantized and 3D Tiles terrain providers, Google Earth Enterprise metadata and terrain, Ion resources/geocoding, platform clients and world-terrain factories. Bing, Google, Pelias, OpenCage and coordinate geocoders are implemented, awaiting validation. Validate parsing, request construction and response/error behavior using deterministic fixtures.
11. **Contracts and host behavior:** 🟦 interpolation/packing interfaces, geometry factories, video bindings and terrain sampling implemented; ⬜ screen-space events and text/pin rendering remain. Audit every remaining Core tracker row and validate implementations before claiming 100% Core.

Core remains the priority through 100% of applicable modules. Progress requires implemented behavior and reference validation; placeholder APIs do not count. Existing n/a entries describe JavaScript-specific idioms and must retain explicit Rust replacement notes. The viewer already runs on Core — it was migrated ahead of 100% Core so the two implementations could not drift apart. After Core, proceed to Renderer and Scene.

### Completed partial-module batch

All 35 Core partial implementations have been completed and are now tracked as **implemented, unchecked**, alongside the native KTX2 worker calculation. No tests or fixture generation were run at the user's request. Native and WebAssembly builds cover the adapters, and wasm-bindgen packages their actual JS modules; parity replay remains a separate milestone. Validated coverage is unchanged.

The remaining 20 Core modules are network terrain providers and metadata/3D Tiles processing, Ion/platform clients and world factories, ground polylines, screen-space input, pins and text rendering. These remain todo; browser adapter work is not used to mark unrelated Core modules complete.
