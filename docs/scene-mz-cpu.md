# Scene M–Z CPU batch

Reference: `@cesium/engine` **23.0.0**, CesiumJS **1.139**. Each of these ten
modules was TODO in PORTING.md at the start of this batch. No browser or GPU
parity is claimed. The fixture generator imports the pinned source modules;
expected outputs are not calculated by Rust.

Run reference generation after `npm ci` in `tools/cesium`:

```sh
node tools/cesium/generate-scene-mz.mjs
RUST_MIN_STACK=134217728 CARGO_INCREMENTAL=0 CARGO_PROFILE_DEV_DEBUG=0 cargo +stable test --offline --test scene_mz_cpu --test fixture_coverage
```

The compiler's default thread stack SIGSEGVed on this checkout; a 64 MiB
`RUST_MIN_STACK` allowed `cargo check` to finish. This setting changes the build
process, not the library. Rust 1.97 also failed during test code generation
(with an internal span-index panic and a dependency SIGSEGV). The successful
replay used installed stable Rust 1.99.1, a 128 MiB stack, disabled incremental
compilation and disabled development debug info. All 10 CPU tests passed.

| Module | Implemented behavior | Remaining scope / adaptation |
| --- | --- | --- |
| OrderedGroundPrimitiveCollection | Stable order within z-index, moving between groups, show gate, ownership-preserving removal, destruction, ScenePrimitive update | Rust owns primitives and uses collection-local handles; finite z-indices required. Missing handles return false rather than adding an unknown object. |
| PerformanceDisplay | Strict 1000/200 ms sample windows, frame counts, rendered/N/A text, throttled indicator | Partial: host supplies timestamps and presents text; no DOM container. Rust decimal formatting has not been exhaustively compared with JS `toFixed` rounding ties. |
| Terrain | Pending future, success/provider state, ready/error events exactly once, default error logging; world terrain and bathymetry factories | Futures are explicitly polled by caller's executor; factories delegate to existing Core clients. Live Ion authentication/network requests are not tested in this batch. |
| Tileset3DTileContent | Synchronous external-tree loader callback before readiness, resource URL, tile identity, metadata/group, zero metrics, no features/pick, destruction | Partial: callback supplies owning tileset integration; existing TilesetResourceLoader still uses its StructuralContent adapter. No retained owning tileset reference or new automatic tree integration. |
| Vector3DTileBatch | Index offset/count, color, batch IDs | Owned vectors replace JS array references. No geometry rendering is implied by this record. |
| VoxelContent | Typed metadata-array construction, deferred ready transition, one request-render callback, idempotent ready update, destruction | Partial: glTF loading, attribute mapping and loader resource ownership remain absent. |
| VoxelShapeType | All string constants, parsing, default min/max bounds | Partial: cylinder/ellipsoid constructor dispatch remains absent; no unsupported shape silently maps to a box. |
| VoxelBoxShape | Clipped bounds and degeneracy rules, affine tile/sample OBBs, bound transform/sphere, UV scale/translation, view rotation transform | Partial: render plane collection, shader defines/uniform binding and GPU intersection are absent. |
| SpatialNode | Box octree children, approximate voxel size, distance-based screen error, visit/renderability state, sorted unique keyframes, weighted temporal selection across ancestors | Partial: caller supplies nearest-first ancestor snapshots and sorted renderable frames; only box bounds are supported. Megatexture allocation/removal, asynchronous keyframe lifecycle, visibility adapter and persistent parent/child graph remain absent. |
| VoxelCell | Typed metadata slices, property names/access, padded linear sample-to-UV conversion, sample OBB | Partial: resolved node/content and primitive identity supplied by caller; no Scene.pickVoxel integration, cylinder or ellipsoid bounds. Borrowed property slices replace mutable JS references; OBB returns a copy. |

`scene_mz_cpu` compares 24 translated/rotated/scaled (including reflected) box
scenarios, 8 ancestor/temporal scenarios and 6 padded sample scenarios, plus
shape defaults, sampling windows, external-content loading, vector batches,
collection update/destruction traces and content readiness against the source.
Additional Rust regressions cover clipping, degenerate/singular transforms,
keyframe uniqueness, stale visits, camera inside the box, terrain pending/error
handling, transfer without destruction, invalid z-index, failed external-tree
loading and use after destruction. These establish the covered CPU behavior,
not full voxel rendering or browser parity.

Validation result: all **22 tests** passed across `scene_mz_cpu` (10),
`fixture_coverage` (1), `scene_primitive_runtime` (4), and `scene_quadtree` (7).
The full repository test suite, browser validation, and live terrain services
were not run.
