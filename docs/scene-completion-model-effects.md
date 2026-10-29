# Model-effects implementation evidence

Scope: all 18 entries assigned to `model-effects` in `docs/scene-completion.json`.
Reference: locally installed **@cesium/engine 23.0.0** (the checkout initially
had no node_modules; source inspection used the adjacent project's matching
pinned installation). Scope and shared progress files are unchanged.

This work does **not** establish completion of all 18 modules. No status is
promoted. The table distinguishes delivered implementation/proof from work
still necessary to meet the original completion objective.

| Module | Implementation/evidence in this change | Unresolved owned behavior/proof |
| --- | --- | --- |
| TranslucentTileClassification | Reviewed existing owned framebuffer/depth/accumulation implementation and Scene routing. | Independent classification GPU comparison remains; no new classification proof. |
| PickedMetadataInfo | Reviewed existing owned shader/class property/value transform representation in pick_object.rs. | End-to-end model metadata render/read comparison remains. |
| ShadowVolumeAppearance | Fixed transitive shader dependency propagation; added color/pick vertex synthesis with high/low east/west projection extents. 16 pinned fixtures compare color, pick and vertex defines. | Upstream static planar/spherical texture-coordinate geometry helpers remain absent; material-input shader combinations and real GPU classification proof remain. |
| Model/Model | Existing scene_model_commands and scene_model_runtime suites exercise lifecycle, loader factories, stage dispatch, ready-event deferral, styles, instancing, picking and submission. | Full WebGL model rendering across supported formats and external resources remains unchecked. Terrain/environment services still require host integration. No model implementation changes claimed here. |
| Model/ModelDrawCommands | Existing scene_model_commands suite exercises owned draw/classification wrappers. | Independent full pipeline GPU comparison remains. No implementation changes claimed here. |
| Model/Model3DTileContent | Existing scene_model_commands includes real-frame process/readiness/submission/destruction regression; reader/features tests cover native format data. | Format-by-format GPU rendering/loading parity (glTF/GLB/b3dm/i3dm/pnts/GeoJSON) remains. No implementation changes claimed here. |
| AutoExposure | Existing five post-processing tests include pyramid reduction/resource behavior. Added three-frame real-GPU comparison harness. | Real WebGL2 three-frame luminance output matches pinned engine exactly (12 float values); additional sizes/HDR inputs remain to validate. |
| PostProcessStage | Real WebGL2 black-and-white factory shader, texture binding, execution and copy have identical 8x8 pixels to pinned engine. | Selection, URL uniforms, all formats, log depth and shader mutation need broader GPU proof. |
| PostProcessStageCollection | Added concrete SceneEffect adapter with owned/shared collection and resolved input handles. Fixed arbitrary-depth lookup, mutable lookup, selection detection and leaf cache allocation. Single and four-level nested execution match pinned GPU bytes. | Scene integration owner must connect resolved view attachments to adapter. Full builtin/HDR/selection pipeline GPU parity remains. |
| PostProcessStageComposite | Four-level nested composite now gets framebuffers for every leaf and executes with identical GPU pixels. | Full multi-input/dependency/selection mutation GPU comparisons remain. |
| PostProcessStageLibrary | Black-and-white factory validated through actual GPU rendering; existing factory suite covers CPU options/names. | Other factory effects need independent GPU comparisons. |
| PostProcessStageTextureCache | Collection now recursively enumerates all leaf stages, matching execution recursion; nested GPU test proves actual target allocation/wiring. Existing cache suite covers reuse/resize. | Full arbitrary dependency graph allocation/reuse GPU proof remains. |
| GaussianSplatSorter | Added usable native TaskWorker adapter backed by existing Rust radix algorithm. Initialization rejects false/non-true results and preserves errors. Dedicated test initializes, schedules, polls and checks actual sorted indexes. | Browser worker backing still must be supplied by host. Native implementation is polled, not a spawned background thread. Native worker algorithms already existed; not a new Gaussian renderer. |
| GaussianSplatTextureGenerator | Added native TaskWorker adapter backed by actual covariance/half-float texture packer, with strict initialization success and native task result test. | Browser worker backing still must be supplied by host; transferable/worker parity and independent generated-attribute GPU rendering proof remain. |
| GaussianSplatRenderResources | Added missing dynamic uniform provider map; preserved Clone/Debug support (Debug lists names without evaluating callbacks). Existing defaults/log-depth tests available. | Full splat pipeline GPU proof remains; no Gaussian primitive implementation claimed. |
| BrdfLutGenerator | Releases one-shot shader program after execution. Publishes texture only on success, supports retry after failure, rejects update after destruction. Real WebGL2 comparison checks every byte of 256x256 RGBA8 LUT: **262144 bytes, difference 0**. | Final extended harness rebuild and GPU rerun succeeded. No owned loading/rendering gap identified for this generator. |
| ShadowMapShader | Implemented receive fragment derivation: directional, point, spot, cascades, soft/depth/cube defines, normal shading and offset, terrain/polygon bias, fade/debug colors. **4096 pinned variants** compare defines and FNV-1a-64 of exact JSON source arrays. Existing 32 cast/receive-vertex fixtures retained. | Real shadow-map GPU lighting proof remains. |
| ImageBasedLighting | Frame/context deduplication, default SH/specular transitions, retention/invalidation of explicitly uploaded environment resource; URL alone no longer means specular texture is ready. Dedicated lifecycle/state test. | Automatic KTX2 resource loading/convolution/error events and automatic BRDF generator update remain absent. Model uses separate ImageBasedLightingOptions; connecting these requires further integration. |

## Integration requests / exports

No edits to `src/scene/mod.rs` were made. New types are already accessible under
existing public modules. Integration may re-export:

- `post_process_stage_collection::{PostProcessSceneEffect, PostProcessSceneInputs}`
- `shadow_map_shader::{ReceiveBias, ReceiveOptions}` (and `receive_fragment` if desired).

Install `PostProcessSceneEffect` into `Scene.effects`. Keep the public Rc handles
to its collection and optional inputs. The View/Scene integration owner must
populate color/depth/id/target/pass **after MSAA/depth resolution**, and refresh
handles on resize; adapter render executes clear, collection pipeline and copy.
Missing render inputs return an explicit error. Inputs are released on destroy.
Do not point input color at the output framebuffer attachment (feedback loop).

IBL needs a concrete specular environment loader and connection from its uploaded
atlas to model texture-uniform providers. `set_specular_environment_map` retains
the actual uploaded resource with its matching URL; it is not a URL-ready flag.

## Reproduction

Use separate checkout-local artifact directories; existing linker/compiler
configuration is preserved. No global toolchain installs/config edits.

```sh
node tools/cesium/generate-scene-model-effects.mjs
RUST_MIN_STACK=33554432 CARGO_TARGET_DIR=target/model-effects-ci cargo +1.97.0 test \
  --test scene_model_effects_completion --test scene_post_processing \
  --test scene_model_commands --test scene_model_runtime --test scene_nz_contracts \
  --test scene_model_features --test scene_model_reader --test scene_mz_cpu
RUST_MIN_STACK=33554432 CARGO_TARGET_DIR=target/model-effects-wasm cargo +1.97.0 build \
  --target wasm32-unknown-unknown --example model_effects_validation --features browser
wasm-bindgen target/model-effects-wasm/wasm32-unknown-unknown/debug/examples/model_effects_validation.wasm \
  --out-dir target/model-effects-gpu --out-name effects --target web
node tools/cesium/model-effects-check.mjs
```

GPU runner uses headless Chromium `/opt/helium/chrome` (override with
CHROMIUM_PATH), WebGL2 and SwiftShader. This is actual shader execution/readback,
not the recording Gl test adapter. Successful first run is recorded in
`model-effects-evidence/webgl2-brdf-postprocess.json`: BRDF, single stage and
four-level nested stage are byte-identical. Final extended GPU run also passes
three-frame auto-exposure: 12 float values, exact equality. Cesium half-float
readbacks are decoded from Uint16 bits before comparison; Rust readbacks already
convert float attachments to Float32. Logs preserve missing 1.99.1
Wasm target, 1.99.1 native compiler crashes and 1.97.0 Wasm linker/compiler
crashes. Retried both installed compilers, increased RUST_MIN_STACK, tried a larger
process stack limit and serial compilation. CI-pinned 1.97.0 serial compilation
finally succeeded for native and Wasm builds. No replacement linker or global
configuration.


Final validation: **69 native tests passed** (68 across the eight suites, followed
by five dedicated tests after adding initialization-failure coverage; one new
test increases the distinct total to 69). `cargo +1.97.0 check --target
wasm32-unknown-unknown --example model_effects_validation --features browser`
also passed. GPU evidence is from the final source build, four comparisons with
zero differences. Artifacts were moved beneath ignored `target/model-effects*`
after execution; logs retain the original paths as captured.
