# Workers

The first Workers completion batch adds all **31 geometry creation entry points**
and **`createGeometry`**. Together with the five previously validated terrain and
KTX2 workers, **37 of 53 Workers modules** are now ported. No remaining worker is
marked partial or counted as complete through a placeholder.

## Geometry entry points

`workers::geometry` exports `create_box_geometry`, `create_polygon_geometry` and
the other geometry worker functions. They use the complete Core algorithms for
boxes, circles, coplanar polygons, corridors, cylinders, ellipses, ellipsoids,
frustums, ground polylines, planes, polygons, polylines, polyline volumes,
rectangles, simple polylines, spheres and walls, including every outline variant
present in CesiumJS Workers.

```rust
use cesium_rs::{core::*, workers::*};

let description = BoxGeometry::from_dimensions(
    Cartesian3::new(2.0, 4.0, 6.0), VertexFormat::ALL, None,
).unwrap();
let geometry = create_box_geometry(GeometryInput::Description(&description));

let mut packed = vec![0.0; 3 + BoxGeometry::PACKED_LENGTH];
description.pack(&mut packed, 3);
let from_buffer = create_box_geometry(GeometryInput::Packed {
    values: &packed, offset: 3,
});
assert_eq!(geometry, from_buffer);
```

Packed descriptions follow the existing Core packing contract: the slice must
contain a complete, valid description at the supplied offset. `None` represents
Cesium's undefined geometry for degenerate inputs. The batch also fills in the
missing Core `CircleOutlineGeometry::unpack` and the zero-length plane-outline
pack/unpack operations.

Rust centers, ellipsoids and nested descriptions retain their types when moved
between threads, replacing Cesium's reconstruction of structured-cloned objects.
Ground polylines use the pinned, bundled approximate terrain-height table and
need no runtime fetch before calculation.

## Batches and buffers

`GeometrySubTask` carries an owned typed `GeometryDescription`, an offset-packed
buffer, or an already-created optional geometry. `create_geometry(tasks).await`
dispatches built-in modules by their Cesium worker names. The default registry
also accepts the corresponding `./create…Geometry.js` paths.

A reusable `GeometryWorker` accepts custom asynchronous factories with
`register(name_or_path, factory)`. Explicit host registration replaces JavaScript
dynamic import; the same factory is retained across batches. Native applications
can execute a batch in a `core::native_host::NativeTaskWorker` callback; browser
applications can call these portable calculations from their Wasm worker entry
point. This batch does not add a JavaScript message transport or mark
`createTaskProcessorWorker` complete.

Polling a batch starts every subtask. Successful results retain input order even
when completion order differs. A failure rejects the batch without waiting for
an unrelated pending task. Dropping the batch drops its outstanding Rust futures.
Specifying both `module_name` and `module_path`, an unknown module, or a typed
input that does not match its selected module returns `RuntimeError`.

`PackedGeometryResults` implements the `PrimitivePipeline` geometry-result wire
format: a shared attribute-name table and one owned `Vec<f64>`. Ownership moves
replace JavaScript's transfer list. Packing retains missing geometries, primitive
and geometry types, offsets, both bounding spheres, component types,
normalization, attributes and indices. Unpacking reconstructs typed arrays and
16/32-bit indices and rejects truncated buffers, invalid lengths and unknown
enums. Only these result-packing helpers are implemented here; the Scene
`PrimitivePipeline` module remains unported.

## Reference validation

`tools/cesium/fixtures/workers-geometry.mjs` imports the actual pinned CesiumJS
worker entry points. It generates **474 cases**: 15 for each of the 31 geometry
workers and nine batch/wire cases. Inputs cover typed descriptions, Float64
buffers at offsets zero and three, extrusion, alternate ellipsoids, antimeridian
paths, empty geometry, perspective/orthographic frustums, near-plane selection,
ground projections, and repeated/mixed modules. Batch output is compared with
Cesium's packed buffer and its own unpacked result. Synthetic wire cases exercise
all eight component datatypes, normalized attributes, CV bounding spheres and
the 65,536-vertex index-width boundary.

Rust checks geometry metadata and indices exactly. Double attributes and bounds
use 1e-12 relative / 1e-7 absolute tolerance; Float32 attributes allow rounding at
2e-7 relative / absolute tolerance. Additional regressions cover custom module
reuse, concurrent start and reversed completion, failure with a pending sibling,
selector conflicts, buffer corruption, and a real native worker thread through
`TaskProcessor`.

```bash
cargo test --test workers_geometry --features native-host
cargo test --all-features
cargo clippy --lib --all-features
cargo check --lib --target wasm32-unknown-unknown --features browser

# Regenerate this batch's reference files without rewriting other fixtures.
TZ=UTC node tools/cesium/fixtures/generate.mjs createGeometry \
  $(node --input-type=module -e 'import fs from "node:fs"; for (const f of fs.readdirSync("tools/cesium/node_modules/@cesium/engine/Source/Workers")) if (/^create.+Geometry.js$/.test(f)) process.stdout.write(f.slice(0,-3)+" ")')
```

## Remaining 16 modules

The next batches cover:

- Terrain and packets: `createVerticesFromCesium3DTilesTerrain`,
  `upsampleVerticesFromCesium3DTilesTerrain`,
  `createVerticesFromGoogleEarthEnterpriseBuffer`,
  `decodeGoogleEarthEnterprisePacket`.
- Vector tiles: `createVectorTilePoints`, `createVectorTilePolylines`,
  `createVectorTileClampedPolylines`, `createVectorTilePolygons`,
  `createVectorTileGeometries`.
- Gaussian splats: `gaussianSplatSorter`, `gaussianSplatTextureGenerator`.
- Specialized codecs: `decodeDraco`, `decodeI3S`.
- Scene combination and worker messaging: `combineGeometry`,
  `createTaskProcessorWorker`. `transferTypedArrayTest` is n/a: its `onmessage`
  handler only transfers a typed array through `postMessage`. Owned Rust buffers
  replace that transfer.

Existing Core helpers are reused only after their worker boundaries have direct
reference validation.

## decodeDraco

`workers::decode_draco` decodes Draco 1.x meshes and point clouds with
`draco-core` 2.3.2 (decoder, point-cloud, EdgeBreaker valence, and legacy
bitstream features). It is the same crate the I3S worker uses and is not a
shared helper. The WebAssembly initialization message returns `true`.

Dequantized attributes, index width, and the two geometry-type errors match
the CesiumJS worker on 18 direct cases. `dequantizeInShader` does not: the
crate always inverse-transforms and does not keep the portable integers or
the quantization parameters `SkipAttributeTransform` would return. Callers
that set the flag still receive the dequantized values and `quantization: null`.
