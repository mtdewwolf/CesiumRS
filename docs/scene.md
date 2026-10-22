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
`tests/scene_gltf_pipeline.rs` replays the resulting 406 cases:

- 405 are compared, exactly except `updateVersion`, which allows a 1e-12
  relative difference for the `pow`.
- One debug-only precondition is skipped.

```bash
cargo test --test scene_gltf_pipeline
TZ=UTC node tools/cesium/fixtures/generate.mjs gltf_pipeline/updateVersion
```

## Next

The next batches build on these:

- the glTF and 3D Tiles resource loaders on top of `GltfPipeline`;
- the 3D Metadata schema and table classes;
- 3D Tiles content parsers;
- then the frame-driven modules (`FrameState`, `Camera`, `Primitive`, the
  globe and the tile traversal) on top of the Renderer's typed frame
  snapshots.
