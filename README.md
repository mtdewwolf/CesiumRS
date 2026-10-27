# CesiumRS

A port of **CesiumJS** (`@cesium/engine` 23.0.0, CesiumJS 1.139) to Rust. It is a rewrite, not a binding to the JavaScript library, with validated modules checked against real CesiumJS output. Implementations awaiting those checks are listed separately.

![Port progress by package](docs/progress.svg)

![Overall progress over time](docs/progress-history.svg)

**Status: 70.2% implemented; 62.4% validated** (1,043 validated modules, 6 implemented awaiting validation, and 50 partial of 1,265 applicable modules, weighted by JavaScript lines). **Core: 100.0% implemented; 100.0% validated** (270 validated applicable modules; no remaining partial or todo modules). [PORTING.md](PORTING.md) lists every module and the [Plan](PORTING.md#plan).

## What is ported

The Core modules below live in `src/core`, one Rust module per Cesium module:

| Area | Cesium modules |
| --- | --- |
| Math | `Math` (incl. `MersenneTwister` random numbers), `Cartesian2/3/4`, `Matrix2/3/4`, `Quaternion`, `HeadingPitchRoll`, `HeadingPitchRange`, `Spherical`, `Cartographic`, `TranslationRotationScale`, `Color` (CSS parsing, named colors) |
| Ellipsoid and projections | `Ellipsoid` (WGS84, Moon, Mars, unit sphere), `scaleToGeodeticSurface`, `Rectangle`, `GeographicProjection`, `WebMercatorProjection`, `MapProjection`, `EllipsoidTangentPlane` |
| Curves | `EllipsoidGeodesic` (Vincenty), `EllipsoidRhumbLine`, `PolylinePipeline` (geodesic and rhumb densification, antimeridian wrapping) |
| Animation and interpolation | `Spline` (shared trait and time operations), `ConstantSpline`, `LinearSpline`, `SteppedSpline`, `QuaternionSpline`, `MorphWeightSpline`, `HermiteSpline` (incl. natural and clamped cubics), `CatmullRomSpline`, `TridiagonalSystemSolver`, `EasingFunction`, `LagrangePolynomialApproximation`, `LinearApproximation`, `HermitePolynomialApproximation` |
| Intersections | `Ray`, `Plane`, `IntersectionTests`, `Intersect`, `Interval`, `QuadraticRealPolynomial`, `CubicRealPolynomial`, `QuarticRealPolynomial` |
| Spatial ordering | `MortonOrder` (2D/3D Z-order encode/decode), `HilbertOrder` (Number-coordinate encode through level 1023, BigInt/decimal decode through level 511), `S2Cell` (64-bit IDs/tokens, ancestry, geodetic centers and vertices) |
| Vertex packing | `AttributeCompression` (oct normals, texture coordinates, ZigZag terrain decoding, dequantization, RGB), `EncodedCartesian3` (high/low position splitting) |
| Bounding volumes and culling | `BoundingSphere`, `BoundingRectangle`, `AxisAlignedBoundingBox`, `OrientedBoundingBox`, `CullingVolume`, `Occluder`, `EllipsoidalOccluder`, `Visibility` |
| Frustums | `PerspectiveFrustum`, `PerspectiveOffCenterFrustum`, `OrthographicFrustum`, `OrthographicOffCenterFrustum` |
| Enums and helpers | `ArcType`, `CornerType`, `ReferenceFrame`, `isBitSet`, `NearFarScalar`, `DistanceDisplayCondition`, `WebGLConstants` |
| Tiling | `TilingScheme` (trait), `GeographicTilingScheme`, `WebMercatorTilingScheme`, `Stereographic` |
| Terrain | `TerrainEncoding` (12-bit and uncompressed vertex codecs, optional normals/Mercator coordinates, layout descriptors, exaggeration), `HeightmapTessellator` (all numeric sample types, packed sample layouts, geographic and Mercator grids, edge skirts, bounds and horizon culling), `TerrainProvider` (native asynchronous contract, cached grid/edge/skirt indices, geometric error), `TileAvailability` (quadtree ranges, position and rectangle coverage, child masks), `TerrainMesh`, `TerrainPicker` (incremental quadtree picking, culling, exaggeration, 3D/2D/Columbus View transforms and antimeridian wrapping), `TerrainData` (native contract), `HeightmapTerrainData` (raw/mesh interpolation, packed samples, LERC1/2 decoding, mesh creation and upsampling), `QuantizedMeshTerrainData` (mesh creation, skirt bounds, clipping and normal interpolation during upsampling, typed credits and child masks), `ApproximateTerrainHeights` (bundled height table, curvature bounds and bounding spheres) |
| Terrain providers | `EllipsoidTerrainProvider` (zero-height tiles), `CustomHeightmapTerrainProvider` (typed samples, callbacks, deferred/error futures and typed host credits); ArcGIS, VR-TheWorld, Cesium heightmap/quantized mesh, Google Earth Enterprise and implicit 3D Tiles terrain, with world terrain/bathymetry factories |
| Mesh data | `ComponentDatatype`, `IndexDatatype`, `PrimitiveType` (typed arrays as one `TypedArray` enum), `Tipsify`, `WireframeIndexGenerator`, `Intersections2D` |
| Geometries | `Geometry`, `GeometryAttribute(s)`, `GeometryType`, `GeometryOffsetAttribute`, `VertexFormat`; `BoxGeometry`, `PlaneGeometry`, `EllipsoidGeometry`, `SphereGeometry`, `CylinderGeometry`, `FrustumGeometry` and their outlines; `EllipseGeometry`, `CircleGeometry`, `EllipseOutlineGeometry`, `CircleOutlineGeometry`, `EllipseGeometryLibrary`; `RectangleGeometry`, `RectangleOutlineGeometry`, `RectangleGeometryLibrary`; `PolygonGeometry`, `PolygonOutlineGeometry`, `PolygonGeometryLibrary` (holes, extrusion, per-position heights, rhumb lines, equator splitting), `CoplanarPolygonGeometry`, `CoplanarPolygonOutlineGeometry`, `CoplanarPolygonGeometryLibrary`; `PolylineGeometry`, `SimplePolylineGeometry`, `GroundPolylineGeometry` (terrain-bounded volumes, geodesic/rhumb subdivision, miter breaks and 2D attributes); `WallGeometry`, `WallOutlineGeometry`, `WallGeometryLibrary`; `PolylineVolumeGeometry`, `PolylineVolumeOutlineGeometry`, `PolylineVolumeGeometryLibrary`; `CorridorGeometryLibrary`, `CorridorGeometry`, `CorridorOutlineGeometry` (rounded, mitered and beveled corners, surface frames, texture coordinates, extrusion, offsets, bounds and shadow volumes). Full vertex and index buffers are compared with CesiumJS. |
| Polygons | `PolygonPipeline` (area, winding order, triangulation, chord and rhumb-line subdivision, geodetic height scaling) on `earcut`, a line-for-line port of earcut 3.2.4 whose triangle order matches the JavaScript library |
| Geometry instances and pipeline | `GeometryInstance`, `GeometryInstanceAttribute`, `ColorGeometryInstanceAttribute`, `ShowGeometryInstanceAttribute`, `OffsetGeometryInstanceAttribute`, `DistanceDisplayConditionGeometryInstanceAttribute`; `GeometryPipeline` (wireframes, vertex-cache reordering, 16-bit index splitting, 2D projection, high/low encoding, world transforms, combining instances, normals and tangents, vertex compression, and splitting triangles, lines and polylines at the antimeridian) |
| Utilities | `Heap`, `DoubleEndedPriorityQueue`, `Queue`, `DoublyLinkedList`, `mergeSort`, associative and managed arrays, typed-array strings, URI parsing and helpers, query strings, GUIDs, `PixelFormat`, `PixelDatatype`, and Core's enums |
| Frames | `Transforms`: local frames, heading-pitch-roll, window coordinates, 2D model matrices, and the time-dependent TEME, ICRF and Moon-fixed frames |
| Time | `JulianDate` (with Cesium's runtime-extendable leap-second table), `GregorianDate`, `LeapSecond`, `Iso8601`, `TimeInterval`, `TimeIntervalCollection`, `TimeStandard`, `TimeConstants`, `Clock`, `ClockStep`, `ClockRange`, `Event` |
| Astronomy and Earth orientation | `Simon1994PlanetaryPositions` (Sun and Moon positions), `EarthOrientationParameters`, `Iau2006XysData` (Cesium's XYS tables are in `assets/IAU2006_XYS`; the `bundled-xys` feature compiles them in), `Iau2000Orientation`, `IauOrientationAxes`, `IauOrientationParameters` |

### Renderer complete

All **46 Renderer modules** are implemented and checked against CesiumJS.
The final 13 add cube/volume textures, async image loading and atlases,
vertex-array facades, shared canvases, uniform factories and complete Scene-frame
uniform updates. Context now includes picking, default resources, viewport
commands, cleanup and WebGL1/WebGL2 execution with extension fallbacks.

Validation covers **1,549 pinned Renderer cases** and **1,720 Chromium assertions**,
including all 90 automatic uniforms, texture pixels/mips, atlas repacking,
facade GPU buffers and shared-canvas presentation. Native hosts implement `Gl`.
[Renderer API and validation](docs/renderer-foundation.md) documents ownership,
frame inputs and the remaining viewer/native-driver integration work.

### Workers complete

All **52 applicable Workers modules** are ported, including geometry, terrain,
vector tiles, Gaussian splats, `decodeI3S`, `decodeDraco`, `combineGeometry`, and
`createTaskProcessorWorker`. The I3S worker decodes binary and Draco payloads
into an owned glTF document. `decodeDraco` matches CesiumJS with and without
`dequantizeInShader`, using a vendored `draco-core` that can skip attribute
transforms. `combineGeometry` runs the existing geometry pipeline on packed
instances. `createTaskProcessorWorker` adapts a calculation to the
TaskProcessor message contract. `transferTypedArrayTest` is n/a.
[Workers API and validation](docs/workers.md).

### Scene underway

Scene includes validated Camera and CameraFlightPath ports. Current module counts and line-weighted progress are in [PORTING.md](PORTING.md#scene).

The first covers the enums and stateless helpers the rest of Scene builds on:
- the blend, depth and stencil constants and presets, returned as renderer
  render-state values;
- scene, globe and 3D Tiles state;
- glTF attribute semantics;
- 3D Metadata types, with BigInt-aware `MetadataComponentType`;
- lights and `Splitter`.

The second ports all 24 `GltfPipeline` utilities: `parseGlb`, the glTF 0.8
and 1.0 to 2.0 upgrade, `addDefaults`, `removeUnusedElements`, `ForEach` and
the accessor readers.

Work on the largest modules starts with Camera, a prerequisite of Scene's
frame loop. It supports 3D/2D/Columbus navigation, reference frames, view
fitting, picking, change events, timed flights, callbacks, preload culling and
position correction, checked against 1,161 CesiumJS scenarios. Hosts drive
the flight clock and controller input gate. See [Scene](docs/scene.md#camera).

The third, 3D Metadata, has 24 modules so far:
- the schema classes and the `MetadataEntity` helpers;
- the binary and JSON property tables;
- the 3D Tiles 1.0 batch table hierarchy;
- property attributes;
- the tileset, tile, content, group and implicit subtree entities, and the
  lookups that find them.

All are replayed against direct CesiumJS cases. Model dependencies now include typed ModelComponents, initialized Draco workers, shared resource cache leases, glTF JSON/buffer views and frame-driven Draco/GPU index loading. Image, texture and SPZ Gaussian-splat loaders are validated, completing ResourceCache and vertex loading; the SPZ decoder matches the pinned `@spz-loader/core` WebAssembly bit for bit. GltfLoader now assembles geometry, materials, feature IDs, skins, animations and metadata through these caches. ModelUtility, render resources and thirteen shader pipeline stages are validated; the complete Model renderer and scene graph remain in progress. The 3D Tiles binary content parsers are in: `Cesium3DTileFeatureTable`, `B3dmParser`, `I3dmParser` and `PntsParser`. So is the `NGA_gpm_local` extension: its data classes and `GltfGpmLoader`. 3D Metadata now covers property textures, `StructuralMetadata` and its three parsers (`EXT_structural_metadata`, `EXT_feature_metadata`, batch tables). [Scene](docs/scene.md).

### Shaders complete

All **307 Shaders modules** are ported. `shaders::shader(id)` returns the exact
GLSL text of every builtin, appearance, material, root, Model,
PostProcessStages and Voxels module, and `shaders::builtin::czm_builtins()`
is the `CzmBuiltins.js` table. Each is compared byte for byte with the
`@cesium/engine` 23.0.0 default export. The renderer still uses its pinned
builtin table. [Shaders](docs/shaders.md).

### Ion and platform clients

`Ion`, `IonResource`, `IonGeocoderService`, `GoogleMaps` and `ITwinPlatform` now
have Rust implementations checked against CesiumJS. Configuration is owned by
instances. Ion assets refresh credentials through the shared resource client and
restrict bearer headers to their asset domain; static geocoder credits are handed
to the host. The Core tracker now has 270 validated modules and 18 documented n/a entries.

### Validated Core foundations

The 35 formerly partial Core modules now have their missing implementation and backend work: resource execution/retries, all HTTP verbs, JSONP/XML/image loading, request statistics, dynamic terrain sampling, geocoder transport/parsing, R-tree collision behavior, IDN normalization, credits, image/video/task adapters, and portable Basis decoding for KTX2. Packing, interpolation and geometry contracts connect to the existing numeric and geometry codecs.

These 35 Core modules and the KTX2 worker calculation now pass reference and runtime checks: 617 new CesiumJS cases, native HTTP/worker lifecycle tests and browser runtime checks. Validation fixed GEE decoding, Basis output/configuration differences and terrain error propagation. [Validation details and commands](docs/core-validation.md) record the coverage and host adaptations. The final 20 Core modules are also complete: network terrain/GEE/3D Tiles processing, Ion/platform clients, ground polylines, screen-space events and text/pin rendering. The full suite now covers 300 Rust tests, 63 Core Chromium checks and 1,720 Renderer Chromium assertions.

Native applications enable `native-host` for `NativeTransport`, `NativeTimer` and cooperative worker threads. `ResourceClient` drives its shared scheduler when a response future is polled. `Resource` fetch methods return an optional future for scheduling deferral and an optional result for no-content or deferred retries. Missing sampled terrain heights use NaN because `Cartographic.height` is numeric.

The `browser` feature supplies concrete canvas/image, DOMPurify credit, media-event, HTTP/JSONP and module-worker adapters. It is optional so the existing raw C-ABI viewer keeps its build path. Browser bindings are packaged with:

```bash
cargo install wasm-bindgen-cli --version 0.2.129 --locked
scripts/build-browser-core.sh  # writes target/browser-core, including JS modules
```

The package exports `BrowserCore` for JSON/image loading, credits, RGBA canvases and native KTX2 transcoding, plus `writeTextToCanvas`, `BrowserPinBuilder` and `BrowserScreenSpaceEventHandler`. Rust applications can use the individual adapters, including `BrowserVideoBinding` for automatic clock/seek subscriptions. See [Core host adapters](docs/core-hosts.md) for usage and threading contracts.

### Mapping CesiumJS to Rust

- `result` out-parameters become return values.
- `core::js_math` computes `sin`, `cos`, `log`, `atan2` and the other `Math` functions bit for bit as V8 does (fdlibm ports plus the `libm` crate), so rounding matches CesiumJS even where an algorithm branches on it, as `BoundingSphere.fromPoints` does.
- Where Cesium returns `undefined`, Rust returns `Option`. Where Cesium throws a `RuntimeError` or rejects bad input (such as an invalid ISO 8601 string), Rust returns `Result`.
- Checks that only exist in Cesium's debug builds (`DeveloperError`) are not ported. Rust behaves like Cesium's release build, except where the check prevents a crash or an endless loop; those cases return `Option`.
- Cesium's quirks are kept on purpose and documented where they occur. For example, `Date.UTC` maps years 0–99 to 1900–1999, `fastSlerp` rounds to `Float32Array`, and the cubic root solver has a known bug.
- Quantized component types are native Rust integer slices. Dequantized attributes and RGB565 colors return `Vec<f32>` to preserve `Float32Array` rounding. Buffer writes return `None` on invalid lengths or offsets without modifying the buffer.
- `HilbertOrder` preserves Cesium's Number rounding, NaN and signed bitwise behavior; its encode/decode pair does not always round-trip. Number-coordinate encoding supports levels 1–1023; arbitrary-precision/decimal decoding supports levels 1–511, where Cesium's range check stays finite. The original integer decode convenience method supports levels 1–32. `S2Cell` uses native 64-bit IDs and preserves Cesium's level-zero face-construction quirk.
- Where Cesium builds attributes or indices in plain JavaScript arrays (such as `PolygonPipeline`'s subdivisions), Rust stores the values as `Float64` and the indices as `Uint32`, so no precision is lost.
- Terrain providers use associated data, request and credit types, with futures replacing promises and `None` indicating throttled requests or unknown availability. Cached terrain topology is shared through immutable `Arc` values per thread, and heightmap quality is an explicit argument. `TileAvailability` preserves Cesium's range insertion order and shared-boundary quirks.
- Terrain picker refinement runs synchronously in Rust using the same triangle-distribution calculation as Cesium's worker. The reference fixtures execute that worker through a message adapter and wait for refinement between picks. Terrain buffers are borrowed by the picker, so meshes need no duplicate storage.
- Heightmap terrain tasks return ready futures around synchronous native calculations. `lerc-reader` decodes compressed input in pure Rust on native and WebAssembly targets. Height interpolation preserves Cesium's different scale/offset behavior before and after mesh creation; successful meshing releases the source buffer.
- Quantized terrain processing preserves triangle and vertex order, recursive clipping keys, oct-normal interpolation and skirt index-width promotion. It also preserves Cesium's post-mesh interpolation quirk: query coordinates use a different UV scale from decoded mesh coordinates.
- Approximate terrain heights embed the pinned Cesium lookup table in `assets/approximateTerrainHeights.json`; owned instances can load a supplied JSON table. Custom terrain providers accept credits from host/UI code and callbacks with optional futures; `None` defers the tile.
- Spline constructors check data shape and finite, strictly increasing times. Import the `Spline` trait for evaluation and time operations. Point types are checked at compile time; scalar and Cartesian3 values support linear interpolation, and quaternion animation uses `QuaternionSpline`.

## How parity is checked

`tools/cesium` installs the real `@cesium/engine` 23.0.0. Node scripts call it with seeded random inputs and edge cases and save the inputs and outputs as JSON in `tests/fixtures`. The Rust replay suites load all **49,493 cases**, skipping debug-only `DeveloperError` preconditions, and compare the remaining results with tolerances chosen per suite: typically 1e-12, or 1e-7 relative for geometry suites with Float32 attributes. Hilbert indices use decimal strings to preserve BigInt precision in JSON. A fixture function that has no Rust counterpart fails the test, and `fixture_coverage` checks that every fixture file has a Rust replay suite.

```bash
cargo test --all-features                    # all fixtures + native host integration
bash scripts/check-browser-core.sh          # build and validate Core browser WASM in Chromium
bash scripts/check-renderer.sh              # compare Rust WebGL2 bytes/pixels/state with CesiumJS
cd tools/cesium && npm ci && npm run fixtures   # regenerate fixtures from CesiumJS
npm run tracker                              # refresh README metrics, PORTING.md and charts
npm run milestone -- "Name"                  # same, starting a new point on the history chart
```

The tracker reads module status from `tools/cesium/status.json` and the hand-written plan from `tools/cesium/plan.md`. It updates the progress numbers in both this README and `PORTING.md`, and counts the fixture files to refresh the README's case total. Capability descriptions and roadmap details are updated with each porting batch.

For a focused fixture refresh, pass module names, for example
`bun run fixtures LinearSpline QuaternionSpline` from `tools/cesium`.
This leaves other fixture files untouched.

## DataSources properties

`src/data_sources` provides typed constant, callback, array, sampled and interval
properties, property bags, entities, collections, composite collections, references,
sixteen complete graphics classes, a partial ModelGraphics and a Color material: 41 validated modules
with 809 pinned CesiumJS cases, including position frames, velocity and orientation. Sampling
supports Linear, Lagrange and Hermite interpolation, derivatives, quaternion
conversion and bounded extrapolation. Definition events support local Rc
callbacks and release child subscriptions with their owners. Graphics slots start
undefined; raw options create ConstantProperties and clone/merge share property
objects, including nested Entity references and composite composition. Visualizers,
the remaining material types, `NodeTransformationProperty` and the
CZML/KML/GeoJSON/GPX loaders remain ahead. See
[DataSources API and validation](docs/data-sources.md).

## Viewer

![The CesiumRS viewer](docs/viewer.png)

`web/` is a [Lakebed](https://lakebed.dev) capsule that draws a WGS84 globe with WebGL. It runs on the wasm build of this crate and provides places, Vincenty geodesics, tap-to-measure, picking, a graticule, geographic and Web Mercator tile grids, and sun lighting. The imagery is Natural Earth II, the same set CesiumJS bundles. A live build is at https://frosted-ridge-959d00d06e.lakebed.app.

```bash
scripts/build-web.sh          # cargo build --target wasm32-unknown-unknown, embedded into web/
scripts/check-generated.sh    # fail if web/client/generated is stale vs a fresh build
node scripts/check-web.mjs    # exercise the bundled wasm geodesic API
cd web && npx lakebed dev
```

The viewer runs on `src/core`: its camera, picking, geodesics, tile grids, and sun direction are thin layers (`src/camera.rs`, `src/geodesic.rs`, `src/mesh.rs`, `src/sun.rs`) over the ported Core modules — `Transforms`, `Matrix4`, `Ellipsoid`, `EllipsoidGeodesic`, the tiling schemes, `IntersectionTests`, and the Simon1994 ephemeris. There are no separate first-generation implementations. The live build still needs a redeploy to pick this up.

Near-antipodal routes use a deterministic great-circle approximation when Vincenty's method is unsuitable. These fallback distances are spherical approximations; interpolation preserves the geodetic endpoints, including repeated points.

## Roadmap

1. ✅ **Core at 100%:** all 270 applicable modules implemented and validated; 18 JavaScript idioms have documented Rust replacements.
2. **Renderer complete:** all 46 modules, including Context, complete Scene-frame uniforms, cube/volume textures, atlases and shared contexts. Next: Scene and viewer integration; see [Renderer validation](docs/renderer-foundation.md).
3. ✅ **Shaders complete:** all 307 `Source/Shaders` modules match their CesiumJS exports byte for byte; see [Shaders](docs/shaders.md).
4. ✅ **Workers complete:** all 52 applicable modules, including `decodeDraco` with `dequantizeInShader`; see [Workers](docs/workers.md).
5. **Scene:** See [the tracker](PORTING.md#scene) for current validated, partial and unchecked module counts. The model resource loaders (ResourceCache, Draco, SPZ, vertex/index, image/texture), the b3dm/i3dm/pnts parsers, 3D Metadata parsing and the structural metadata and GPM loaders are validated. GltfLoader now builds model components; ModelUtility, render resources and the alpha, color, splitter, skinning and morph stages are validated. Next: the remaining Model rendering stages/runtime and the 3D Tiles content loaders; see [Scene](docs/scene.md).
6. **DataSources:** 41 of 108 modules validated and two partial. Properties, positions/motion, bags, entities, collections, composite collections, references, the concrete graphics classes and Color material are available; next are the remaining material types, `NodeTransformationProperty`, data-source clocks/collections and document loaders. See [DataSources](docs/data-sources.md).

## Examples and benchmarks

`examples/` runs against the public API: `geodesic_route` (Vincenty routes and the near-antipodal fallback), `camera_and_picking` (the viewer camera), and `benchmark` (a dependency-free timing harness). Native numbers should use the bench profile — the release profile is size-tuned for the wasm viewer:

```
cargo run --example geodesic_route
cargo run --profile bench --example benchmark
```

`scripts/bench.sh` compares CesiumRS against CesiumJS itself, natively and in Chromium. It runs 14 workloads (Core math, geometry, terrain tessellation and Draco decoding) on the same inputs, checks every output against the CesiumJS fixtures, and reports time ratios. See [Parity benchmark](docs/benchmarks.md).

## Conventions for new ports

- Mirror Cesium's API shape first; the fixture suites are the contract.
- A function taking more than four parameters, or several same-typed parameters that are easy to transpose, gets an options struct (several geometry ports already do this). Existing multi-argument functions keep their shape until their Cesium counterpart changes or they are touched for another reason — no flag-day rewrites.
- Global state follows the contracts in the crate-level docs ("Global state and threading"): Cesium's semantics, explicit synchronization, per-thread where Cesium's single global would be a footgun in Rust.
- The crate is deliberately not published to crates.io yet (`publish = false`): the native and browser API is still evolving. Versions are marked with git tags and GitHub releases instead; see `CHANGELOG.md`.

## License

Apache-2.0. The algorithms follow CesiumJS ([Apache-2.0](https://github.com/CesiumGS/cesium)). This project is not affiliated with Cesium GS, Inc.
