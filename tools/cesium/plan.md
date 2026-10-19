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
   - ⬜ `HeightmapTerrainData`, `QuantizedMeshTerrainData`, `TerrainData` and the terrain geometry processors
7. ⬜ **Remaining spatial algorithms:** `S2Cell`, `RectangleCollisionChecker`, `ApproximateTerrainHeights`, `GroundPolylineGeometry`, and Hilbert levels above 32.
8. ⬜ **Asset and texture processing:** compressed texture buffers, KTX2 transcoding/loading, texture packing, vector and Google Earth Enterprise decoding, and image/canvas helpers through explicit host adapters where needed.
9. ⬜ **Resource and execution infrastructure:** requests, errors, scheduling, resources, trusted servers, task processing, proxy contracts and credits. Inject I/O and host capabilities so behavior remains testable in native Rust and WebAssembly.
10. ⬜ **Terrain providers and service clients:** heightmap/ellipsoid/quantized and 3D Tiles terrain providers, Google Earth Enterprise metadata and terrain, Ion resources, geocoders, platform clients and world-terrain factories. Validate parsing, request construction and response/error behavior using deterministic fixtures.
11. ⬜ **Remaining contracts and host behavior:** interpolation and packing interfaces, geometry factories, screen-space events and video synchronization, plus terrain sampling APIs. Audit every remaining Core tracker row before claiming completion.

Core remains the priority through 100% of applicable modules. Progress requires implemented behavior and reference validation; placeholder APIs do not count. Existing n/a entries describe JavaScript-specific idioms and must retain explicit Rust replacement notes. The viewer already runs on Core — it was migrated ahead of 100% Core so the two implementations could not drift apart. After Core, proceed to Renderer and Scene.
