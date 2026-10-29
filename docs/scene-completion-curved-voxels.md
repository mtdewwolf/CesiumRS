# Scene curved-voxel shapes completion report

Workstream: curved-voxels (VoxelCylinderShape / VoxelEllipsoidShape).

## Modules owned and status

### Scene/VoxelCylinderShape
- Status: CPU substantially complete.
- Evidence: src/scene/voxel_cylinder_shape.rs; fixture tests/fixtures/scene/VoxelCylinderState.json; test voxel_cylinder_shape_matches_reference.
- Owned behavior: default bounds, update visibility/degeneracy, render bounds, OBB for shape/tile/sample, UV transform, shader intersection count and defines/uniforms, view-transform update.

### Scene/VoxelEllipsoidShape
- Status: CPU substantially complete.
- Evidence: src/scene/voxel_ellipsoid_shape.rs; fixture tests/fixtures/scene/VoxelEllipsoidState.json; test voxel_ellipsoid_shape_matches_reference.
- Owned behavior: default WGS84 bounds, update/clamping, OBB for shape/tile/sample, UV transform via nearest point on ellipse, shader intersection count and defines/uniforms, view-transform update.

### Scene/VoxelShapeType
- Status: constructor dispatch added.
- Evidence: src/scene/voxel_shape_type.rs (create_shape/new_shape/create).

### Scene/VoxelCell
- Status: adapted to shape trait.

### Scene/SpatialNode
- Status: adapted to shape trait.

### Scene/VoxelShape
- Status: trait implemented by cylinder and ellipsoid.

## Missing renderer dependencies
- Full shader uniform binding, VoxelBoundsCollection texture upload, VoxelPrimitive rendering, and GPU validation remain renderer/scene-owned.

## Needed exports (not edited per instruction)
- src/scene/mod.rs should export VoxelCylinderShape and VoxelEllipsoidShape (currently reachable via crate::scene::voxel_shape).

## Validation
- cargo +1.99.1 check --offline passed on 1.99.1.
- cargo +1.99.1 test --offline --test scene_voxel_utilities passed: 14 tests.
- cargo +1.99.1 test --offline --test scene_mz_cpu passed: 10 tests.

## Known local rustc failures
- Default cargo test can SIGSEGV in rustc codegen/debuginfo on this machine; use separate target dir and CARGO_PROFILE_*_DEBUG=0 with RUST_MIN_STACK=134217728.
