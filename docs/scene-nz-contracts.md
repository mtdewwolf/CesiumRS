# Scene N–Z contracts and runtime behavior

Reference: `@cesium/engine` **23.0.0**, pinned in `tools/cesium/package-lock.json`.
Ten modules were previously todo in the tracker. This batch includes upstream
abstract interfaces; it does not claim ten complete renderers.

| Module | Delivered | Remaining / adaptation |
| --- | --- | --- |
| Panorama | Required show getter/setter, transform and optional credit trait | Abstract upstream; concrete image renderers are separate modules |
| PanoramaProvider | Async factory/load contracts with associated options, output and errors | Abstract upstream; Rust futures replace promises |
| ParticleSystem | Emission carry, bursts, lifetime/loop/completion events, velocity transforms, randomized particle attributes, aging/callbacks/removal, color/scale billboard snapshots | GPU billboard submission, sizeInMeters, reuse pool/memory estimation and JS option aliases remain |
| QuadtreeTileProvider | Extracted the existing working traversal provider trait and geometric-error helper into its own module, preserving old imports; six new pinned geometric-error cases | Existing optional hook defaults preserved; explicit quadtree back-reference and provider error-event API remain absent. This is partly tracker reconciliation, not a new traversal implementation |
| ShadowMapShader | Exact cast vertex/fragment GLSL, receive vertex defines, cast/receive keyword generation | Receive fragment lighting and actual shadow-map execution remain |
| SkyBox | Mode/render-pass filtering, HDR command delegation, sources, destruction, Earth face URL construction | Requires a caller-supplied panorama renderer; no built-in CubeMapPanorama loader. Pinned update ignores public show; retained and documented |
| ViewportQuad | Executable viewport commands from Material GLSL, alpha blending/overlay pass, numeric/bool/matrix uniforms, material identity shader replacement, resource destruction | Caller submits returned commands and binds resolved texture samplers. Automatic material texture update/Scene integration and fractional rectangle coordinates remain |
| VoxelBoundsCollection | Plane membership/removal/index shifts/events, transformed normalized float packing, grow/shrink RGBA float texture upload and destruction | GL calls tested with recording host, not a real browser GPU. Empty unallocated texture resolution returns [0,0] instead of JS NaN height; removal unlinks the stored handle when supplied a value-equal plane |
| VoxelProvider | All upstream property/request contracts, typed metadata, request coordinates/keyframe defaults, deferred/error/success futures | Abstract upstream; concrete providers/loaders remain separate |
| VoxelShape | Dynamic CPU bounds/UV/view-transform contract implemented by the existing VoxelBoxShape | Shader uniforms/defines/intersection limits and clipping renderer omitted; cylinder/ellipsoid implementations remain separate |

Tracker marks only the three complete abstract contracts as ported; six modules
are partial, and VoxelBoundsCollection is implemented (unchecked) pending real
GPU validation. No renderer readiness is synthesized for missing integrations.

## Proof

Regenerate reference cases:

```sh
node tools/cesium/generate-scene-nz-contracts.mjs
```

`tests/fixtures/scene/NzContracts.json` contains 32 exact shadow source variants,
six geometric errors and nine particle frame snapshots. Particle fixtures run
the original upstream update method with a billboard host adapter; emission,
aging, burst and completion logic are unchanged. Tests additionally cover
provider factories/deferred futures/errors, dynamic shape dispatch, hidden and
reverse particle time, mutable planes, GPU texture uploads/release and viewport
draw execution/recompilation through a recording GL host.

```sh
cargo test --test scene_nz_contracts --test scene_quadtree \
  --test scene_quadtree_substantive --test scene_mz_cpu --test fixture_coverage
cargo check --tests
```

The targeted suites pass **36 tests** (9 new + 26 existing + fixture coverage). The local environment
has no C compiler and its configured `/tmp/opencode/bin/cc` is missing. Validation
used an untracked `/tmp/nz-linker` wrapper around system `ld`, system CRT objects
and libgcc_s, plus `RUST_MIN_STACK=33554432` for this environment's compiler stack.
No build configuration or dependency changes are included in this commit.
