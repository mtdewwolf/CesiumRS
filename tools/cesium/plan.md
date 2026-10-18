Batches toward 75% Core, in order. Each module is ported with CesiumJS fixtures before it is marked ported.

1. ✅ **Small utilities:** containers, URI and query helpers, typed-array strings, `PixelFormat`, `PixelDatatype`, and Core's enums.
2. 🟡 **Polygon and instance foundations:**
   - ✅ earcut 3.2.4 and `PolygonPipeline`
   - 🟡 `GeometryInstance` and its Color, Show, Offset and DistanceDisplayCondition attributes: written, waiting on fixtures
   - ⬜ `GeometryPipeline`: next, source read and port started
3. ⬜ **Surface geometries:** `EllipseGeometry`, `CircleGeometry`, `RectangleGeometry` (with its library and outline), `PolygonGeometryLibrary`, `PolygonGeometry`, `PolygonOutlineGeometry`, and the coplanar polygon geometries.
4. ⬜ **Line and volume geometries:** `PolylinePipeline`, `PolylineGeometry`, `SimplePolylineGeometry`, the wall, corridor and polyline-volume geometries, then terrain helpers (`HeightmapTessellator`, `TerrainEncoding`) as needed.

After 75% Core: move the viewer onto `src/core` and redeploy it, then finish the rest of Core and start Renderer and Scene.
