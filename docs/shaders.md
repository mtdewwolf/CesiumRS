# Shaders

`src/shaders` stores the GLSL text of each module in `@cesium/engine` 23.0.0
`Source/Shaders`. Every upstream subdirectory is registered. This port's three
directories are complete: all 140 Builtin modules, the `CzmBuiltins.js` table,
14 Appearances and 19 Materials, 174 modules in all. The other port's
directories are still empty.

The renderer does not read this registry. [`renderer::shader_source`](../src/renderer/shader_source.rs)
still assembles programs from the pinned `assets/renderer/glsl-builtins.json`
table. A module here is the exact default-export string of the upstream file.

## Registration

Each upstream subdirectory is one Rust module. It appears in `src/shaders/mod.rs`
as two lines, and those lines are only ever added:

1. `pub mod <directory>;` with the other module declarations.
2. `<directory>::register(&mut shaders);` inside `registry`.

Append both lines. Do not edit, reorder, or delete another directory's line.
Two people can add directories without editing the same block. Lookup is by
shader id and does not depend on registration order.

Inside a directory, each shader is one `shaders.push(&SHADER);` line in that
directory's `register` function. A new group under a directory (`Constants`,
`Functions`, `Structs`) is the same pair of lines in that directory's `mod.rs`,
not in the crate-root shader module. An empty directory calls `register_empty`
so its signature stays `&mut Vec` and the first shader is a `push` line.

| Directory | Upstream | Owner |
| --- | --- | --- |
| `builtin` | `Builtin/` (`Constants`, `Functions`, `Structs`, `CzmBuiltins.js`) | this port |
| `appearances` | `Appearances/` | this port |
| `materials` | `Materials/` | this port |
| `root` | `Shaders/*.js` | the other port |
| `model` | `Model/` | the other port |
| `post_process_stages` | `PostProcessStages/` | the other port |
| `voxels` | `Voxels/` | the other port |

`builtin`, `appearances`, and `materials` are this port. `root`, `model`,
`post_process_stages`, and `voxels` are empty stubs for the other port. Do not
add shader text to the other port's directories.

`registry()` builds that list once, behind a `OnceLock`. Lookup is by id.

## Layout

Each module's text is a `.glsl` file that `include_str!` embeds in one
`pub static` [`ShaderModule`](../src/shaders/mod.rs). The static's name is the
file stem in upper snake case (`czm_HSBToRGB` is `HSB_TO_RGB`), and the file
keeps the upstream stem:

| Upstream | Rust statics | Text |
| --- | --- | --- |
| `Builtin/Constants/*.js` | `shaders::builtin::constants` | `src/shaders/builtin/constants/<stem>.glsl` |
| `Builtin/Structs/*.js` | `shaders::builtin::structs` | `src/shaders/builtin/structs/<stem>.glsl` |
| `Builtin/Functions/*.js` | `shaders::builtin::functions` | `src/shaders/builtin/functions/<stem>.glsl` |
| `Builtin/CzmBuiltins.js` | `shaders::builtin::czm_builtins()` | the 140 builtins above |
| `Appearances/*.js` | `shaders::appearances` | `src/shaders/appearances/glsl/<stem>.glsl` |
| `Materials/*.js` | `shaders::materials` | `src/shaders/materials/glsl/<stem>.glsl` |

Each `.glsl` file is the module's default export, not a copy of the upstream
`.glsl` file. In 23.0.0 the two are identical for all 173 modules.
`Functions/textureCube` and `Functions/unpackClippingExtents` have no trailing
newline upstream, and neither do their files here.

`CzmBuiltins.js` exports an object, not a GLSL string, so it is not in
`registry()`. `czm_builtins()` returns the builtins in its key order:
constants, then structs, then functions, each sorted by file name. That is the
order `builtin::register` pushes them in.

## Names

`ShaderModule::name` for a builtin is its `CzmBuiltins.js` key, `czm_<stem>`.
That is also the GLSL identifier and, where the file declares one, its
`@alias`. Most builtins have no `@alias`; `czm_pi` does
(`Builtin/Constants/pi.js`).

An appearance or material is named by its file stem, which is how Scene
imports it (`import GridMaterial from "../Shaders/Materials/GridMaterial.js"`).
None of those modules declare an `@alias`.

A root `Shaders/*.js` module's name is its file stem. In `@cesium/engine`
23.0.0 there are 55 of those modules. Each default export is the GLSL string
alone, and call sites import the stem, for example
`import PolylineCommon from "../Shaders/PolylineCommon.js"`. `AdjustTranslucentFS.js`
is `AdjustTranslucentFS`. Those files do not declare an `@alias`.

## Fixtures

Each directory that has shader cases adds one file,
`tools/cesium/fixtures/shaders-<directory>.mjs`. `generate.mjs` reads every
file matching that name, in filename order, and writes

`tests/fixtures/shaders/<directory>/<stem>.json`.

Adding a directory does not edit `generate.mjs`. The directory name is the
text between `shaders-` and `.mjs`, so `shaders-builtin.mjs` writes
`tests/fixtures/shaders/builtin/constants.json`. Stems can repeat across
directories because the path includes the directory. Select one fixture with
`TZ=UTC node tools/cesium/fixtures/generate.mjs builtin/constants`.

A fixture holds one upstream group, not one module, so each needs one
`common::run` call:

| Fixture | Cases |
| --- | ---: |
| `shaders/builtin/constants.json` | 40 |
| `shaders/builtin/structs.json` | 8 |
| `shaders/builtin/functions.json` | 92 |
| `shaders/builtin/CzmBuiltins.json` | 1 |
| `shaders/appearances/appearances.json` | 14 |
| `shaders/materials/materials.json` | 19 |

`tools/cesium/fixtures/shader-cases.mjs` lists the `.js` files of a group in
the installed package and records one `shader` case per file. A module that
upstream adds and the port does not register therefore fails its replay. The
helper's name does not start with `shaders-`, so `generate.mjs` does not load
it as a directory.

## Validation

A ported shader matches the default export of the `@cesium/engine` 23.0.0
module, including the trailing newline. The fixture imports that module and
records the string. The Rust replay loads the fixture and compares with
`EXACT`, so the source text has to be identical.

`czm_pi` shows the path of one module:

- Upstream module: `Source/Shaders/Builtin/Constants/pi.js`.
- Rust source: `src/shaders/builtin/constants/pi.glsl`, included by `shaders::builtin::constants::PI`.
- Tracker id: `Shaders/Builtin/Constants/pi`.
- Fixture: `tools/cesium/fixtures/shaders-builtin.mjs` records
  `{ name: "czm_pi", source }` for function `shader` and argument
  `["Shaders/Builtin/Constants/pi"]`.
- Generated file: `tests/fixtures/shaders/builtin/constants.json`.
- Replay: `tests/shaders_builtin.rs` passes `common::shader_module`, which
  calls `shaders::shader(id)` and returns `{ name, source }`.

The `CzmBuiltins` fixture records the whole table as `[{ name, source }]` in
key order, for function `czmBuiltins`. Its replay builds the same array from
`czm_builtins()`. Appearances and Materials replay in
`tests/shaders_appearances.rs` and `tests/shaders_materials.rs`.

`shaders::shader` returns `None` for an unknown id. The fixture harness treats
an unknown function name as a missing port, which fails the test.

A unit test checks the registry against `tools/cesium/status.json`: every
registered id has an entry, and every `Shaders/` entry marked `ported` is
registered. `Shaders/Builtin/CzmBuiltins` is the one exception, since it is a
table; `builtin::CZM_BUILTINS_ID` names it.

## Tracker

`PORTING.md`, the README status line, and the progress charts come from
`tools/cesium/tracker.mjs`. Two shader ports both edit `status.json` and
`plan.md`. Whoever merges second reruns the tracker so the generated files
include both sides:

```bash
TZ=UTC node tools/cesium/tracker.mjs --milestone "Shaders …"
```

Omitting `--milestone` rewrites the previous milestone in place.
