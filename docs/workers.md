# Workers

The first Workers completion batch adds all **31 geometry creation entry points**
and **`createGeometry`**. Together with the five previously validated terrain and
KTX2 workers, later terrain, vector-tile, and Gaussian splat workers,
`decodeI3S`, `decodeDraco`, `combineGeometry`, and `createTaskProcessorWorker`,
all **52 applicable Workers modules** are ported; `transferTypedArrayTest` is
n/a. No worker is marked partial or counted as complete through a placeholder.

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
enums. The Scene `PrimitivePipeline::combine_geometry` entry point reuses this
module's geometry kernel and pick-offset computation.

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

## I3S geometry

`workers::decode_i3s::decode_i3s` is the CesiumJS `decodeI3S` worker. It walks an
I3S binary buffer in schema or `bufferInfo` order, and it decodes payloads that
start with the `DRACO` magic through `workers::draco`. That module is shared
with `decodeDraco`. The helper checks for a triangular-mesh header before
`MeshDecoder`, catches a decoder panic as an empty geometry, and leaves
64-bit attributes undefined for I3S. I3S still applies semantic names and
`i3s-scale_*` / `i3s-attribute-type` metadata itself. A point cloud is still
empty geometry.

The result is the worker's glTF document. Bytes that Cesium stores behind blob
URLs are owned buffers. Position accessor `min` and `max` stay swapped. Colors
are converted from sRGB. Feature ids, structural metadata, and
`CESIUM_primitive_outline` are emitted with the same buffer order and the same
JavaScript object-key order for welded vertices. A short header still throws
before the binary decoder's try/catch; a later typed-array failure keeps the
attributes already read.

`init_decode_i3s` is the WebAssembly initialization message. There is no Draco
module to compile, so it returns `true`.

```bash
cargo test --test workers_i3s
TZ=UTC node tools/cesium/fixtures/generate.mjs decodeI3S
```

## Other worker modules

These modules are also ported, each with direct CesiumJS worker fixtures:

- Terrain and packets: `createVerticesFromCesium3DTilesTerrain`,
  `upsampleVerticesFromCesium3DTilesTerrain`,
  `createVerticesFromGoogleEarthEnterpriseBuffer`,
  `decodeGoogleEarthEnterprisePacket`.
- Vector tiles: `createVectorTilePoints`, `createVectorTilePolylines`,
  `createVectorTileClampedPolylines`, `createVectorTilePolygons`,
  `createVectorTileGeometries`.
- Gaussian splats: `gaussianSplatSorter`, `gaussianSplatTextureGenerator`.
- Scene combination and worker messaging: `combineGeometry` runs the existing
  geometry pipeline on packed instances and posts Float32 bounding spheres.
  `createTaskProcessorWorker` adapts a calculation to the TaskProcessor
  `onmessage` / `onmessageerror` contract, including the `formatError` strings
  posted when `postMessage` fails. Object keys in those strings follow
  `JSON.stringify`. `transferTypedArrayTest` is n/a: its `onmessage` handler
  only transfers a typed array through `postMessage`. Owned Rust buffers
  replace that transfer.

Existing Core helpers are reused only after their worker boundaries have direct
reference validation.

## decodeDraco

`workers::decode_draco` decodes Draco 1.x meshes and point clouds through
`workers::draco` and `draco-core` =2.3.2 (decoder, point-cloud, EdgeBreaker
valence, and legacy bitstream features). The helper is the geometry-type
check, the panic boundary, the face indices, and the mapped-index walk taken
out of `decode_i3s`. The WebAssembly initialization message returns `true`.

Attributes, including non-float and 64-bit values, index width, and the two
geometry-type errors match the CesiumJS worker on 61 direct cases.

`dequantizeInShader` is supported. Upstream `draco-core` 2.3.2 always
inverse-transforms, so the crate is vendored in `third_party/draco-core` with
an equivalent of C++ Draco's `Decoder::SetSkipAttributeTransform`; see its
`PATCHES.md`. For meshes, each name in `attributesToSkipTransform` is skipped.
As in Cesium, an unknown name reaches Draco as `undefined`, which Draco reads as
POSITION, and a missing list throws. Point clouds skip POSITION and NORMAL. A
skipped attribute returns its quantized integers and
`{quantizationBits, minValues, range, octEncoded}`. Values of up to 8 bits come
back as `Uint8Array`, values of up to 16 bits as `Uint16Array`, and wider ones
as `Float32Array`; like Cesium's getters, the integer arrays stop at the first
out-of-range value. Integer-encoded attributes keep their `Int32Array`.
Fixtures cover sequential, EdgeBreaker and KD-tree encodings at several speeds
and quantization widths.

```bash
cargo test --test workers_decode_draco
TZ=UTC node tools/cesium/fixtures/generate.mjs decodeDraco
```
