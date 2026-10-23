# Scene

`src/scene` ports the CesiumJS `Scene` package (`@cesium/engine` 23.0.0). It
has one Rust module per Cesium module, as `core` does. `Scene/Model/...`
modules live under `scene::model`.

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
- 3D Tiles content parsers;
- then the frame-driven modules (`FrameState`, `Camera`, `Primitive`, the
  globe and the tile traversal) on top of the Renderer's typed frame
  snapshots.
