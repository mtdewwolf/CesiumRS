# Scene voxel-utilities workstream completion report

Workstream: `voxel-utilities` (see `docs/scene-completion.json`).
Baseline: `055bf7cd104f9eb221fe9e3a134c038aaf4455dc`.

## Modules owned and status

### Scene/VoxelCell
- Status: CPU complete (adaptation).
- Evidence: `src/scene/voxel_cell.rs`; pinned reference cases in `tests/scene_mz_cpu.rs` (`padded_voxel_cells_keep_typed_metadata_and_bounds`); 6 padded sample scenarios.
- Owned behavior present: typed metadata slices, property name/access, padded linear sample-to-UV conversion, sample OBB.
- Unresolved gaps: Scene.pickVoxel integration, cylinder/ellipsoid sample bounds (depend on VoxelCylinderShape/VoxelEllipsoidShape and Scene integration).
- Dependency requests: VoxelCylinderShape and VoxelEllipsoidShape ports; Scene picking integration (parent/integration workstream).

### Scene/Tileset3DTileContent
- Status: CPU complete (adaptation).
- Evidence: `src/scene/tileset_3d_tile_content.rs`; pinned callback fixture in `tests/scene_mz_cpu.rs` (`structural_tileset_content_loads_before_ready_and_propagates_failure`); new test `tileset_3d_tile_content_metadata_and_group_setters`.
- Owned behavior present: synchronous external-tree load callback before readiness, resource URL, tile identity, metadata/group (now settable), zero metrics, no features/pick, lifecycle.
- Unresolved gaps: retained owning tileset reference and automatic tree integration remain with existing TilesetResourceLoader/StructuralContent adapter.

### Scene/PerformanceDisplay
- Status: CPU complete (adaptation).
- Evidence: `src/scene/performance_display.rs`; pinned reference cases in `tests/scene_mz_cpu.rs` (`performance_sampling_matches_reference`).
- Owned behavior present: strict 1000/200 ms sample windows, frame counts, rendered/N/A text, throttled indicator; host-supplied timestamps/text.
- Unresolved gaps: no DOM presentation (host-owned by design); JS `toFixed` decimal tie rounding not exhaustively validated beyond pinned cases.

### Scene/VoxelContent
- Status: CPU partial (metadata path complete; glTF path not implemented).
- Evidence: `src/scene/voxel_content.rs`; pinned cases in `tests/scene_mz_cpu.rs` (`voxel_content_requests_one_more_frame_and_rejects_use_after_destroy`).
- Owned behavior present: typed metadata-array construction, deferred ready transition, one request-render callback, idempotent ready update, destruction.
- Unresolved gaps: glTF loading, attribute reordering (`processAttributes`) require GltfLoader integration.
- Dependency requests: GltfLoader and VoxelPrimitive provider integration (integration workstream).

### Scene/VoxelBoxShape
- Status: CPU substantially complete (render-bound planes + shader metadata added).
- Evidence: `src/scene/voxel_box_shape.rs`; pinned 24 scenarios in `tests/scene_mz_cpu.rs` (`voxel_bounds_and_transformed_boxes_match_reference`, `clipped_degenerate_and_singular_boxes`); new test `voxel_box_shape_render_bounds_and_shader_metadata`.
- Owned behavior present: clipped bounds and degeneracy rules, affine tile/sample OBBs, bound transform/sphere, UV scale/translation, view rotation transform, render min/max bounds, render-bound plane distances, shader intersection count.
- Unresolved gaps: VoxelBoundsCollection texture/GPU integration (plane collection upload) and full shader uniform binding remain host/GPU-owned; real GPU proof pending.
- Dependency requests: GPU validation harness for VoxelBoundsCollection texture upload.

### Scene/SpatialNode
- Status: CPU complete (adaptation).
- Evidence: `src/scene/spatial_node.rs`; pinned 8 ancestor/temporal scenarios in `tests/scene_mz_cpu.rs` (`spatial_selection_and_error_match_reference`); new test `spatial_node_visibility_and_construct_children`.
- Owned behavior present: box octree bounds, approximate voxel size, distance-based screen error, visit/renderability state, sorted unique keyframes, weighted temporal selection across ancestors, child construction, visibility plane mask.
- Unresolved gaps: megatexture allocation/removal, asynchronous keyframe lifecycle, visibility adapter wiring, owned parent graph remain (adaptation uses caller-supplied ancestor snapshots to avoid cycles).
- Dependency requests: Megatexture type and VoxelPrimitive integration.

### Scene/VoxelShapeType
- Status: CPU partial.
- Evidence: `src/scene/voxel_shape_type.rs`; pinned defaults in `tests/scene_mz_cpu.rs`.
- Owned behavior present: all identifiers and default min/max bounds checked against source.
- Unresolved gaps: cylinder/ellipsoid constructor dispatch absent (VoxelCylinderShape/VoxelEllipsoidShape not yet ported).
- Dependency requests: port VoxelCylinderShape (868 lines) and VoxelEllipsoidShape (1174 lines) upstream.

### Scene/JobScheduler
- Status: CPU complete.
- Evidence: `src/scene/job_scheduler.rs` (unit tests in module).
- Owned behavior present: per-type budgets, stealing, starvation tracking, disable/reset, execution timing.
- Unresolved gaps: none at CPU level.

### Scene/createTangentSpaceDebugPrimitive
- Status: CPU partial.
- Evidence: `src/scene/create_tangent_space_debug_primitive.rs` (unit tests in module); new smoke test.
- Owned behavior present: creates colored debug line primitive for normal/tangent/bitangent attributes.
- Unresolved gaps: geometry computation fallback (`geometry.constructor.createGeometry`) not implemented for uncomputed geometry; relies on pre-computed attributes.

### Scene/Empty3DTileContent
- Status: CPU complete.
- Evidence: `src/scene/empty_3d_tile_content.rs` (unit tests in module); new smoke test.
- Owned behavior present: empty tile content with zero metrics, ready, has_property/get_feature false, metadata/group errors, destruction.

### Scene/VoxelBoundsCollection
- Status: implemented (CPU + recording GL); real GPU validation pending.
- Evidence: `src/scene/voxel_bounds_collection.rs`; recording GL tests in `tests/scene_nz_contracts.rs`.
- Owned behavior present: plane events/indexing, value membership, transformed normalized float packing, RGBA float texture grow/shrink/upload, destruction.
- Unresolved gaps: real browser GPU validation.
- Dependency requests: GPU validation harness.

### Scene/VoxelShape
- Status: CPU substantially complete (trait extended).
- Evidence: `src/scene/voxel_shape.rs`; exercised via VoxelBoxShape tests.
- Owned behavior present: dynamic shape contract backed by VoxelBoxShape; bounds, transforms, UV, tile/sample OBBs; min/max/render bounds and shader intersection count now exposed.
- Unresolved gaps: shader uniforms/defines/intersections full trait surface and clipping renderer remain; cylinder/ellipsoid implementations separate.

### Scene/Cesium3DTileContentFactory
- Status: CPU complete (classification scope).
- Evidence: `src/scene/cesium_3d_tile_content_factory.rs`; new test `cesium_content_factory_classify`.
- Owned behavior present: maps magic headers/format identifiers to content types; classifies raw payloads.
- Unresolved gaps: full factory creating content instances requires tileset/content type integration (integration workstream); current scope is classification only.

### Scene/DebugInspector
- Status: CPU partial.
- Evidence: `src/scene/debug_inspector.rs`; new test `debug_inspector_frustum_color`.
- Owned behavior present: fragment shader rewrite, frustum color computation, cached debug shaders.
- Unresolved gaps: shader program creation from cache and debug command execution require renderer/scene integration (DrawCommand, ShaderProgram, pass state).

### Scene/FrameRateMonitor
- Status: CPU complete (adaptation).
- Evidence: `src/scene/frame_rate_monitor.rs`; new smoke tests.
- Owned behavior present: sliding window with quiet/warmup periods and threshold events (pop-based instead of Event).
- Unresolved gaps: Event-based API replaced with pop methods (adaptation); scene integration not wired.

### Scene/GlobeTranslucency
- Status: CPU complete.
- Evidence: `src/scene/globe_translucency.rs`; new test `globe_translucency_bounds`.
- Owned behavior present: front/back face translucency, distance-based alpha fading, cartographic bounding rectangle, validation.
- Unresolved gaps: shader integration (GLOBE_TRANSLUCENT define) is renderer/scene-owned.

## Validation
- `cargo +1.99.1 test --offline --test scene_voxel_utilities` — 12 passed.
- `cargo +1.99.1 test --offline --test scene_mz_cpu` — 10 passed.
- `cargo +1.99.1 test --offline --test shaders_voxels` — 1 passed.
- `cargo +1.99.1 check` — passed.

## Dependency requests for parent/integration workstream
1. Port VoxelCylinderShape and VoxelEllipsoidShape (or provide) to complete VoxelShapeType dispatch and VoxelCell curved shapes.
2. GltfLoader + VoxelPrimitive provider integration for VoxelContent.from_gltf and attribute reordering.
3. Megatexture type and VoxelPrimitive integration for SpatialNode keyframe lifecycle.
4. Scene integration for pickVoxel, VoxelPrimitive rendering, and DebugInspector command execution.
5. Real GPU validation harness for VoxelBoundsCollection texture upload and VoxelBoxShape shader binding.
6. Cesium3DTileContentFactory full content instantiation (depends on content types + tileset).